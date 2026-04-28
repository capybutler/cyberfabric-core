//! Unit tests for REST handlers and `emitter_error_to_problem`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use authz_resolver_sdk::models::{
    EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError};
use axum::Extension;
use axum::Json;
use axum::extract::Path;
use chrono::Utc;
use http::StatusCode;
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::outbox_migrations;
use modkit_db::{ConnectOpts, connect_db};
use modkit_security::SecurityContext;
use usage_collector_sdk::{
    AllowedMetric, ModuleConfig, UsageCollectorClientV1, UsageCollectorError, UsageKind,
    UsageRecord,
};
use usage_emitter::{UsageEmitter, UsageEmitterError, UsageEmitterV1};
use uuid::Uuid;

use super::emitter_error_to_problem;
use super::handle_create_usage_record;
use super::handle_get_module_config;
use crate::api::rest::dto::CreateUsageRecordRequest;

// ── emitter_error_to_problem ──────────────────────────────────────

#[test]
fn authorization_failed_maps_to_forbidden() {
    let err = UsageEmitterError::authorization_failed("pdp denied");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::FORBIDDEN);
    assert_eq!(p.detail, "pdp denied");
}

#[test]
fn authorization_expired_maps_to_forbidden() {
    let err = UsageEmitterError::authorization_expired();
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::FORBIDDEN);
}

#[test]
fn invalid_record_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::invalid_record("missing metric");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(p.detail, "missing metric");
}

#[test]
fn metric_not_allowed_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::metric_not_allowed("cpu.usage");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        p.detail.contains("cpu.usage"),
        "detail should name the metric"
    );
}

#[test]
fn metric_kind_mismatch_maps_to_unprocessable_entity() {
    let err =
        UsageEmitterError::metric_kind_mismatch("req.count", UsageKind::Counter, UsageKind::Gauge);
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(p.detail.contains("req.count"));
}

#[test]
fn negative_counter_value_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::negative_counter_value(-1.5);
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(p.detail.contains("-1.5"));
}

#[test]
fn internal_error_maps_to_500() {
    let err = UsageEmitterError::internal("something broke");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn outbox_error_maps_to_500() {
    let err = UsageEmitterError::Outbox(modkit_db::outbox::OutboxError::QueueNotRegistered(
        "usage".to_owned(),
    ));
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ── handle_get_module_config ──────────────────────────────────────

struct MockCollector {
    config: ModuleConfig,
}

#[async_trait]
impl UsageCollectorClientV1 for MockCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Ok(self.config.clone())
    }
}

struct FailingCollector;

#[async_trait]
impl UsageCollectorClientV1 for FailingCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Err(UsageCollectorError::internal("unavailable"))
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Err(UsageCollectorError::internal("unavailable"))
    }
}

struct NotFoundCollector;

#[async_trait]
impl UsageCollectorClientV1 for NotFoundCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Err(UsageCollectorError::module_not_found(module_name))
    }
}

#[tokio::test]
async fn get_module_config_handler_returns_allowed_metrics() {
    let collector = Arc::new(MockCollector {
        config: ModuleConfig {
            allowed_metrics: vec![AllowedMetric {
                name: "cpu.usage".to_owned(),
                kind: UsageKind::Gauge,
            }],
        },
    }) as Arc<dyn UsageCollectorClientV1>;

    let result = handle_get_module_config(Path("my-module".to_owned()), Extension(collector)).await;

    let axum::Json(resp) = result.expect("handler should succeed");
    assert_eq!(resp.allowed_metrics.len(), 1);
    assert_eq!(resp.allowed_metrics[0].name, "cpu.usage");
}

#[tokio::test]
async fn get_module_config_handler_propagates_collector_error_as_500() {
    let collector = Arc::new(FailingCollector) as Arc<dyn UsageCollectorClientV1>;

    let result = handle_get_module_config(Path("my-module".to_owned()), Extension(collector)).await;

    let err = result.expect_err("handler should fail");
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn get_module_config_handler_returns_404_for_unknown_module() {
    let collector = Arc::new(NotFoundCollector) as Arc<dyn UsageCollectorClientV1>;

    let result =
        handle_get_module_config(Path("unknown-module".to_owned()), Extension(collector)).await;

    let err = result.expect_err("handler should return 404");
    assert_eq!(err.status, StatusCode::NOT_FOUND);
    assert!(err.detail.contains("unknown-module"));
}

#[test]
fn module_not_configured_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::module_not_configured("my-module");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(p.detail.contains("my-module"));
}

// ── handle_create_usage_record ────────────────────────────────────────────────

/// PDP mock that captures the `subject_id` and `subject_type` resource properties from the
/// incoming evaluation request, then allows the request to proceed.
struct CapturingSubjectAuthZ {
    captured_subject_id: Arc<Mutex<Option<String>>>,
    captured_subject_type: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl AuthZResolverClient for CapturingSubjectAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        let subj_id = request
            .resource
            .properties
            .get("subject_id")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let subj_type = request
            .resource
            .properties
            .get("subject_type")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        *self.captured_subject_id.lock().unwrap() = subj_id;
        *self.captured_subject_type.lock().unwrap() = subj_type;
        Ok(EvaluationResponse {
            decision: true,
            context: EvaluationResponseContext::default(),
        })
    }
}

/// Collector that returns a fixed `ModuleConfig` with one allowed metric.
struct FixedConfigCollector;

#[async_trait]
impl UsageCollectorClientV1 for FixedConfigCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Ok(ModuleConfig {
            allowed_metrics: vec![AllowedMetric {
                name: "test.gauge".to_owned(),
                kind: UsageKind::Gauge,
            }],
        })
    }
}

async fn build_handler_emitter(authz: Arc<dyn AuthZResolverClient>) -> Arc<dyn UsageEmitterV1> {
    let db_name = format!("hw_{}", Uuid::new_v4().simple());
    let url = format!("sqlite:file:{db_name}?mode=memory&cache=shared");
    let db = connect_db(
        &url,
        ConnectOpts {
            max_conns: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    run_migrations_for_testing(&db, outbox_migrations())
        .await
        .unwrap();
    let emitter = UsageEmitter::build(
        usage_emitter::UsageEmitterConfig::default(),
        db,
        authz,
        Arc::new(FixedConfigCollector),
    )
    .await
    .unwrap();
    Arc::new(emitter) as Arc<dyn UsageEmitterV1>
}

#[tokio::test]
async fn ingest_handler_passes_subject_fields_to_authorize_for() {
    let subject_id = Uuid::new_v4();
    let subject_type = "test.service_account".to_owned();

    let captured_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_type: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let authz = Arc::new(CapturingSubjectAuthZ {
        captured_subject_id: Arc::clone(&captured_id),
        captured_subject_type: Arc::clone(&captured_type),
    });

    let emitter = build_handler_emitter(authz).await;

    let ctx = SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .unwrap();

    let req = CreateUsageRecordRequest {
        module: "test-module".to_owned(),
        tenant_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        resource_id: Uuid::new_v4(),
        subject_id: Some(subject_id.to_string()),
        subject_type: Some(subject_type.clone()),
        metric: "test.gauge".to_owned(),
        idempotency_key: None,
        value: 1.0,
        timestamp: Utc::now(),
        metadata: None,
    };

    let result = handle_create_usage_record(Extension(ctx), Extension(emitter), Json(req)).await;

    assert!(result.is_ok(), "handler should succeed: {result:?}");

    assert_eq!(
        captured_id.lock().unwrap().as_deref(),
        Some(subject_id.to_string().as_str()),
        "subject_id must be forwarded to the PDP request"
    );
    assert_eq!(
        captured_type.lock().unwrap().as_deref(),
        Some(subject_type.as_str()),
        "subject_type must be forwarded to the PDP request"
    );
}

#[tokio::test]
async fn ingest_handler_succeeds_when_subject_fields_absent() {
    // Both subject fields absent: handler must skip PDP and return 204.
    let captured_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_type: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let authz = Arc::new(CapturingSubjectAuthZ {
        captured_subject_id: Arc::clone(&captured_id),
        captured_subject_type: Arc::clone(&captured_type),
    });

    let emitter = build_handler_emitter(authz).await;

    let ctx = SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .unwrap();

    let req = CreateUsageRecordRequest {
        module: "test-module".to_owned(),
        tenant_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        resource_id: Uuid::new_v4(),
        subject_id: None,
        subject_type: None,
        metric: "test.gauge".to_owned(),
        idempotency_key: None,
        value: 1.0,
        timestamp: Utc::now(),
        metadata: None,
    };

    let result = handle_create_usage_record(Extension(ctx), Extension(emitter), Json(req)).await;

    assert!(
        result.is_ok(),
        "handler should succeed when subject is absent: {result:?}"
    );

    // PDP was not called with subject properties: captured values remain None.
    assert!(
        captured_id.lock().unwrap().is_none(),
        "subject_id must not be forwarded to PDP when absent"
    );
    assert!(
        captured_type.lock().unwrap().is_none(),
        "subject_type must not be forwarded to PDP when absent"
    );
}

#[tokio::test]
async fn ingest_handler_succeeds_when_subject_id_present_without_subject_type() {
    // subject_id present, subject_type absent: valid — must route to PDP and return 204.
    let subject_id = Uuid::new_v4();

    let captured_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_type: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let authz = Arc::new(CapturingSubjectAuthZ {
        captured_subject_id: Arc::clone(&captured_id),
        captured_subject_type: Arc::clone(&captured_type),
    });

    let emitter = build_handler_emitter(authz).await;

    let ctx = SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .unwrap();

    let req = CreateUsageRecordRequest {
        module: "test-module".to_owned(),
        tenant_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        resource_id: Uuid::new_v4(),
        subject_id: Some(subject_id.to_string()),
        subject_type: None,
        metric: "test.gauge".to_owned(),
        idempotency_key: None,
        value: 1.0,
        timestamp: Utc::now(),
        metadata: None,
    };

    let result = handle_create_usage_record(Extension(ctx), Extension(emitter), Json(req)).await;

    assert!(
        result.is_ok(),
        "handler should succeed when subject_type is absent: {result:?}"
    );
    assert_eq!(
        captured_id.lock().unwrap().as_deref(),
        Some(subject_id.to_string().as_str()),
        "subject_id must be forwarded to PDP when subject_type is absent"
    );
    assert!(
        captured_type.lock().unwrap().is_none(),
        "subject_type must not be forwarded to PDP when absent"
    );
}

#[tokio::test]
async fn ingest_handler_returns_error_when_only_subject_type_present() {
    // Only subject_type present: partial presence must return 422.
    let authz = Arc::new(CapturingSubjectAuthZ {
        captured_subject_id: Arc::new(Mutex::new(None)),
        captured_subject_type: Arc::new(Mutex::new(None)),
    });

    let emitter = build_handler_emitter(authz).await;

    let ctx = SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .unwrap();

    let req = CreateUsageRecordRequest {
        module: "test-module".to_owned(),
        tenant_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        resource_id: Uuid::new_v4(),
        subject_id: None,
        subject_type: Some("user".to_owned()),
        metric: "test.gauge".to_owned(),
        idempotency_key: None,
        value: 1.0,
        timestamp: Utc::now(),
        metadata: None,
    };

    let result = handle_create_usage_record(Extension(ctx), Extension(emitter), Json(req)).await;

    let err = result.expect_err("handler should return an error for partial subject");
    assert_eq!(
        err.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "partial subject presence must return 422"
    );
}

#[test]
fn ingest_handler_subject_fields_absent_deserializes_to_none() {
    // `subject_id` and `subject_type` now carry `#[serde(default)]`, so a JSON body
    // that omits them must deserialize successfully with both fields as `None`.
    let body_without_subject = serde_json::json!({
        "module": "test-module",
        "tenant_id": Uuid::new_v4(),
        "resource_type": "test.resource",
        "resource_id": Uuid::new_v4(),
        "metric": "test.gauge",
        "value": 1.0,
        "timestamp": Utc::now()
    });
    let result: Result<CreateUsageRecordRequest, _> = serde_json::from_value(body_without_subject);
    let req = result.expect("deserialization must succeed when subject fields are absent");
    assert!(
        req.subject_id.is_none(),
        "subject_id must be None when absent from JSON"
    );
    assert!(
        req.subject_type.is_none(),
        "subject_type must be None when absent from JSON"
    );
}
