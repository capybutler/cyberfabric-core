//! Unit tests for REST handlers and `emitter_error_to_problem`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use authz_resolver_sdk::constraints::{Constraint, InPredicate, Predicate};
use authz_resolver_sdk::models::{
    EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError, DenyReason};
use axum::Extension;
use axum::Json;
use axum::extract::{Path, Query};
use chrono::Utc;
use http::StatusCode;
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::outbox_migrations;
use modkit_db::{ConnectOpts, connect_db};
use modkit_security::SecurityContext;
use modkit_security::access_scope::pep_properties;
use usage_collector_sdk::{
    AggregationFn, AggregationQuery, AggregationResult, AllowedMetric, Cursor, GroupByDimension,
    ModuleConfig, PagedResult, RawQuery, UsageCollectorClientV1, UsageCollectorError,
    UsageCollectorPluginClientV1, UsageKind, UsageRecord,
};
use usage_emitter::{UsageEmitter, UsageEmitterError, UsageEmitterV1};
use uuid::Uuid;

use super::emitter_error_to_problem;
use super::handle_create_usage_record;
use super::handle_get_module_config;
use super::handle_query_aggregated;
use super::handle_query_raw;
use crate::api::rest::dto::{
    AggregatedQueryParams, CreateUsageRecordRequest, RawQueryParams, cursor_encode,
};
use crate::config::{DEFAULT_PAGE_SIZE, MAX_FILTER_STRING_LEN, MAX_PAGE_SIZE, MAX_QUERY_TIME_RANGE};

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

// ── handle_query_aggregated ───────────────────────────────────────────────────

/// Mock AuthZ that allows all requests with a single tenant constraint.
struct AllowAuthZ {
    tenant_id: Uuid,
}

#[async_trait]
impl AuthZResolverClient for AllowAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Ok(EvaluationResponse {
            decision: true,
            context: EvaluationResponseContext {
                constraints: vec![Constraint {
                    predicates: vec![Predicate::In(InPredicate::new(
                        pep_properties::OWNER_TENANT_ID,
                        [self.tenant_id],
                    ))],
                }],
                ..EvaluationResponseContext::default()
            },
        })
    }
}

/// Mock AuthZ that denies all requests.
struct DenyAuthZ;

#[async_trait]
impl AuthZResolverClient for DenyAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Ok(EvaluationResponse {
            decision: false,
            context: EvaluationResponseContext {
                deny_reason: Some(DenyReason {
                    error_code: "POLICY_DENIED".to_owned(),
                    details: None,
                }),
                ..EvaluationResponseContext::default()
            },
        })
    }
}

/// Mock AuthZ that returns a network/infrastructure error.
struct NetworkErrorAuthZ;

#[async_trait]
impl AuthZResolverClient for NetworkErrorAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Err(AuthZResolverError::ServiceUnavailable(
            "PDP unreachable".to_owned(),
        ))
    }
}

/// Mock plugin that returns an empty aggregation result.
struct OkAggPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for OkAggPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult { items: vec![], next_cursor: None })
    }
}

/// Mock plugin that returns `QueryResultTooLarge`.
struct TooLargePlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for TooLargePlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Err(UsageCollectorError::query_result_too_large(10_001, 10_000))
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult { items: vec![], next_cursor: None })
    }
}

/// Mock plugin that returns an internal storage error.
struct InternalErrorPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for InternalErrorPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Err(UsageCollectorError::internal("storage backend unavailable"))
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult { items: vec![], next_cursor: None })
    }
}

fn test_ctx() -> SecurityContext {
    SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .expect("valid SecurityContext")
}

fn valid_params() -> AggregatedQueryParams {
    let from = Utc::now() - chrono::Duration::hours(1);
    let to = Utc::now();
    AggregatedQueryParams {
        fn_: AggregationFn::Sum,
        from,
        to,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
        source: None,
    }
}

/// TEST-FDESIGN-001 (inst-agg-9): PDP allows, plugin returns empty vec → 200 with empty array.
#[tokio::test]
async fn test_aggregated_200_empty_result() {
    // scenario: inst-agg-9
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let result = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(valid_params()),
    )
    .await;

    let Json(body) = result.expect("handler should succeed");
    assert!(body.is_empty(), "empty plugin result must yield empty array response");
}

/// TEST-FDESIGN-001 (inst-agg-3): absent `fn` parameter fails serde deserialization before handler.
#[test]
fn test_aggregated_400_missing_fn() {
    // scenario: inst-agg-3 — fn is mandatory; deserialization fails when absent
    let json = serde_json::json!({
        "from": "2026-01-01T00:00:00Z",
        "to": "2026-02-01T00:00:00Z",
    });
    let result = serde_json::from_value::<AggregatedQueryParams>(json);
    assert!(result.is_err(), "missing fn must fail serde deserialization");
}

/// TEST-FDESIGN-001 (inst-agg-3): absent `from` parameter fails serde deserialization.
#[test]
fn test_aggregated_400_missing_from() {
    // scenario: inst-agg-3 — from is mandatory; deserialization fails when absent
    let json = serde_json::json!({
        "fn": "sum",
        "to": "2026-02-01T00:00:00Z",
    });
    let result = serde_json::from_value::<AggregatedQueryParams>(json);
    assert!(result.is_err(), "missing from must fail serde deserialization");
}

/// TEST-FDESIGN-001 (inst-agg-3): absent `to` parameter fails serde deserialization.
#[test]
fn test_aggregated_400_missing_to() {
    // scenario: inst-agg-3 — to is mandatory; deserialization fails when absent
    let json = serde_json::json!({
        "fn": "sum",
        "from": "2026-01-01T00:00:00Z",
    });
    let result = serde_json::from_value::<AggregatedQueryParams>(json);
    assert!(result.is_err(), "missing to must fail serde deserialization");
}

/// TEST-FDESIGN-001 (inst-agg-3a): from >= to returns 400 VALIDATION_ERROR.
#[tokio::test]
async fn test_aggregated_400_time_range_not_ascending() {
    // scenario: inst-agg-3a
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let now = Utc::now();
    let mut params = valid_params();
    params.from = now;
    params.to = now - chrono::Duration::hours(1); // to < from

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(params),
    )
    .await
    .expect_err("from >= to must return error");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("strictly ascending")),
        "validation detail must mention ascending time range"
    );
}

/// TEST-FDESIGN-001 (inst-agg-3b): time range exceeds MAX_QUERY_TIME_RANGE → 400.
#[tokio::test]
async fn test_aggregated_400_time_range_too_wide() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let from = Utc::now() - chrono::Duration::hours(1);
    let mut params = valid_params();
    params.from = from;
    // to = from + MAX_QUERY_TIME_RANGE + 1s — strictly exceeds the limit
    params.to = from
        + chrono::Duration::from_std(MAX_QUERY_TIME_RANGE).unwrap()
        + chrono::Duration::seconds(1);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(params),
    )
    .await
    .expect_err("time range exceeding MAX_QUERY_TIME_RANGE must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
}

/// TEST-FDESIGN-001 (inst-agg-3): group_by includes time_bucket but bucket_size absent → 400.
#[tokio::test]
async fn test_aggregated_400_bucket_size_absent_with_time_bucket() {
    // scenario: inst-agg-3
    use usage_collector_sdk::BucketSize;

    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_params();
    params.group_by = vec![GroupByDimension::TimeBucket(BucketSize::Day)];
    params.bucket_size = None; // required but absent

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(params),
    )
    .await
    .expect_err("missing bucket_size with time_bucket must return error");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("bucket_size")),
        "validation detail must mention bucket_size"
    );
}

/// TEST-FDESIGN-001 (inst-agg-3): usage_type exceeding MAX_FILTER_STRING_LEN → 400.
#[tokio::test]
async fn test_aggregated_400_filter_string_too_long() {
    // scenario: inst-agg-3
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_params();
    params.usage_type = Some("a".repeat(257)); // MAX_FILTER_STRING_LEN is 256

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(params),
    )
    .await
    .expect_err("usage_type exceeding max length must return error");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("usage_type")),
        "validation detail must mention usage_type"
    );
}

/// TEST-FDESIGN-002 (inst-agg-6a): PDP returns Denied → 403 with generic body.
#[tokio::test]
async fn test_aggregated_403_pdp_deny() {
    // scenario: inst-agg-6a
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(DenyAuthZ);
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(valid_params()),
    )
    .await
    .expect_err("PDP deny must return 403");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
    // Must NOT contain PDP error details, constraint names, or policy names.
    assert!(!err.detail.contains("POLICY_DENIED"));
    assert!(!err.detail.contains("constraint"));
    assert!(!err.detail.contains("policy"));
}

/// TEST-FDESIGN-002 (inst-authz-3b): PDP returns non-Denied error → 403 (fail-closed).
#[tokio::test]
async fn test_aggregated_403_pdp_non_denied_error() {
    // scenario: inst-authz-3b
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(NetworkErrorAuthZ);
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(valid_params()),
    )
    .await
    .expect_err("PDP network error must return 403 (fail-closed)");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
    // Must NOT contain PDP error details.
    assert!(!err.detail.contains("PDP"));
    assert!(!err.detail.contains("unreachable"));
}

/// TEST-FDESIGN-002 (inst-agg-8c): plugin returns internal error → 503 with correlation_id.
#[tokio::test]
async fn test_aggregated_503_plugin_error() {
    // scenario: inst-agg-8c
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(InternalErrorPlugin);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(valid_params()),
    )
    .await
    .expect_err("plugin internal error must return 503");

    assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["error"], "service_unavailable");
    let corr_id = detail["correlation_id"].as_str();
    assert!(corr_id.is_some(), "503 body must include correlation_id");
    assert!(!corr_id.unwrap().is_empty(), "correlation_id must not be empty");
    // Must NOT contain plugin error details or stack traces.
    assert!(!err.detail.contains("storage backend"));
}

/// TEST-FDESIGN-002 (inst-agg-8b): plugin returns QueryResultTooLarge → 400 'query too broad'.
#[tokio::test]
async fn test_aggregated_400_query_result_too_large() {
    // scenario: inst-agg-8b
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(TooLargePlugin);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(plugin),
        Query(valid_params()),
    )
    .await
    .expect_err("QueryResultTooLarge must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    assert_eq!(err.detail, r#"{"error":"query too broad"}"#);
}

// ── handle_query_raw ──────────────────────────────────────────────────────────

/// Mock plugin that returns a non-empty PagedResult with a next_cursor.
struct OkRawWithCursorPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for OkRawWithCursorPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        let record = UsageRecord {
            module: "test-module".to_owned(),
            tenant_id: Uuid::new_v4(),
            metric: "test.gauge".to_owned(),
            kind: UsageKind::Gauge,
            value: 1.0,
            resource_id: Uuid::new_v4(),
            resource_type: "test.resource".to_owned(),
            subject_id: None,
            subject_type: None,
            idempotency_key: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            metadata: None,
        };
        let cursor = Cursor { timestamp: Utc::now(), id: Uuid::new_v4() };
        Ok(PagedResult { items: vec![record], next_cursor: Some(cursor) })
    }
}

/// Mock plugin that returns an internal error from `query_raw`.
struct RawErrorPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for RawErrorPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Err(UsageCollectorError::internal("storage backend unavailable"))
    }
}

/// Mock plugin that captures the `page_size` field from the RawQuery.
struct CapturingRawPlugin {
    captured_page_size: Arc<std::sync::Mutex<Option<usize>>>,
}

#[async_trait]
impl UsageCollectorPluginClientV1 for CapturingRawPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        *self.captured_page_size.lock().unwrap() = Some(query.page_size);
        Ok(PagedResult { items: vec![], next_cursor: None })
    }
}

fn valid_raw_params() -> RawQueryParams {
    let from = Utc::now() - chrono::Duration::hours(1);
    let to = Utc::now();
    RawQueryParams {
        from,
        to,
        cursor: None,
        page_size: None,
        usage_type: None,
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
    }
}

/// TEST-FDESIGN-001 (inst-raw-9): valid request → 200 with empty items and no next_cursor.
#[tokio::test]
async fn test_raw_200_empty_final_page() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let result =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(valid_raw_params()))
            .await;

    let axum::Json(body) = result.expect("handler should succeed");
    assert!(body.items.is_empty(), "empty plugin result must yield empty items");
    assert!(body.next_cursor.is_none(), "absent next_cursor signals final page");
}

/// TEST-FDESIGN-001 (inst-raw-9): plugin returns items and cursor → 200 with next_cursor present.
#[tokio::test]
async fn test_raw_200_with_items_and_next_cursor() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkRawWithCursorPlugin);

    let result =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(valid_raw_params()))
            .await;

    let axum::Json(body) = result.expect("handler should succeed");
    assert!(!body.items.is_empty(), "response must contain items");
    let cursor_str = body.next_cursor.expect("next_cursor must be present");
    assert!(!cursor_str.is_empty(), "next_cursor must be a non-empty base64 string");
}

/// TEST-FDESIGN-001 (inst-raw-3): malformed cursor → 400 VALIDATION_ERROR.
#[tokio::test]
async fn test_raw_400_malformed_cursor() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_raw_params();
    params.cursor = Some("not-valid-base64!!!".to_owned());

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("malformed cursor must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("cursor")),
        "validation details must mention cursor"
    );
}

/// TEST-FDESIGN-001 (inst-raw-3): cursor timestamp outside [from, to] → 400 VALIDATION_ERROR.
#[tokio::test]
async fn test_raw_400_cursor_timestamp_out_of_range() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let from = Utc::now() - chrono::Duration::hours(2);
    let to = Utc::now() - chrono::Duration::hours(1);
    // cursor timestamp is after 'to' — outside [from, to]
    let cursor_ts = Utc::now();
    let cursor_str = cursor_encode(cursor_ts, Uuid::new_v4(), Utc::now());

    let params = RawQueryParams {
        from,
        to,
        cursor: Some(cursor_str),
        page_size: None,
        usage_type: None,
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
    };

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("cursor outside [from, to] must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
}

/// TEST-FDESIGN-001 (inst-raw-3): page_size = 0 → 400 VALIDATION_ERROR.
#[tokio::test]
async fn test_raw_400_page_size_zero() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_raw_params();
    params.page_size = Some(0);

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("page_size=0 must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("page_size")),
        "validation details must mention page_size"
    );
}

/// TEST-FDESIGN-001 (inst-raw-3): page_size > MAX_PAGE_SIZE → 400 VALIDATION_ERROR.
#[tokio::test]
async fn test_raw_400_page_size_too_large() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_raw_params();
    params.page_size = Some(MAX_PAGE_SIZE + 1);

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("page_size > MAX_PAGE_SIZE must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("page_size")),
        "validation details must mention page_size"
    );
}

/// TEST-FDESIGN-001 (inst-raw-3b): time range exceeds MAX_QUERY_TIME_RANGE → 400.
#[tokio::test]
async fn test_raw_400_time_range_too_wide() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let from = Utc::now() - chrono::Duration::hours(1);
    let mut params = valid_raw_params();
    params.from = from;
    params.to = from
        + chrono::Duration::from_std(MAX_QUERY_TIME_RANGE).unwrap()
        + chrono::Duration::seconds(1);

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("time range exceeding MAX_QUERY_TIME_RANGE must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
}

/// TEST-FDESIGN-001 (inst-raw-3): page_size = MAX_PAGE_SIZE (exact boundary) → 200.
#[tokio::test]
async fn test_raw_200_max_page_size() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_raw_params();
    params.page_size = Some(MAX_PAGE_SIZE); // exact boundary — must be accepted

    let result =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
            .await;

    assert!(
        result.is_ok(),
        "page_size = MAX_PAGE_SIZE must be accepted (validator uses >, not >=): {result:?}"
    );
}

/// TEST-FDESIGN-001 (inst-raw-3): absent page_size → DEFAULT_PAGE_SIZE used in RawQuery.
#[tokio::test]
async fn test_raw_200_default_page_size() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let captured = Arc::new(std::sync::Mutex::new(None));
    let plugin: Arc<dyn UsageCollectorPluginClientV1> =
        Arc::new(CapturingRawPlugin { captured_page_size: Arc::clone(&captured) });

    let mut params = valid_raw_params();
    params.page_size = None;

    let result =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params)).await;

    assert!(result.is_ok(), "absent page_size must not cause a validation error: {result:?}");
    assert_eq!(
        *captured.lock().unwrap(),
        Some(DEFAULT_PAGE_SIZE),
        "absent page_size must default to DEFAULT_PAGE_SIZE"
    );
}

/// TEST-FDESIGN-001 (inst-raw-3): string filter field exceeding MAX_FILTER_STRING_LEN → 400.
#[tokio::test]
async fn test_raw_400_filter_string_too_long() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let mut params = valid_raw_params();
    params.usage_type = Some("a".repeat(MAX_FILTER_STRING_LEN + 1));

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("oversized filter string must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["code"], "VALIDATION_ERROR");
    assert!(
        detail["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap_or("").contains("usage_type")),
        "validation details must mention usage_type"
    );
}

/// TEST-FDESIGN-002 (inst-raw-6a): PDP returns Denied → 403 with generic body.
#[tokio::test]
async fn test_raw_403_pdp_deny() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(DenyAuthZ);
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let err =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(valid_raw_params()))
            .await
            .expect_err("PDP deny must return 403");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
    assert!(!err.detail.contains("POLICY_DENIED"));
}

/// TEST-FDESIGN-002 (inst-authz-3b): PDP returns non-Denied error → 403 (fail-closed).
#[tokio::test]
async fn test_raw_403_pdp_non_denied_error() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(NetworkErrorAuthZ);
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let err =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(valid_raw_params()))
            .await
            .expect_err("PDP network error must return 403 (fail-closed)");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
    assert!(!err.detail.contains("unreachable"));
}

/// TEST-FDESIGN-002 (inst-raw-8b): plugin returns internal error → 503 with correlation_id.
#[tokio::test]
async fn test_raw_503_plugin_error() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(RawErrorPlugin);

    let err =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(valid_raw_params()))
            .await
            .expect_err("plugin internal error must return 503");

    assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["error"], "service_unavailable");
    let corr_id = detail["correlation_id"].as_str();
    assert!(corr_id.is_some(), "503 body must include correlation_id");
    assert!(!corr_id.unwrap().is_empty(), "correlation_id must not be empty");
    assert!(!err.detail.contains("storage backend"), "error details must not leak plugin errors");
}

/// TEST-FDESIGN-001 (inst-sdk-6a): issued_at age exceeds CURSOR_TTL (24h) → 410 Gone.
/// cursor_ts (data position) is within [from, to]; only issued_at is expired.
#[tokio::test]
async fn test_raw_410_cursor_expired() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let now = Utc::now();
    // cursor_ts is valid data within [from, to]; issued_at is 25 hours ago (exceeds CURSOR_TTL)
    let cursor_ts = now - chrono::Duration::hours(1);
    let from = cursor_ts - chrono::Duration::hours(1);
    let to = now;
    let issued_at = now - chrono::Duration::hours(25);
    let cursor_str = cursor_encode(cursor_ts, Uuid::new_v4(), issued_at);

    let params = RawQueryParams {
        from,
        to,
        cursor: Some(cursor_str),
        page_size: None,
        usage_type: None,
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
    };

    let err = handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params))
        .await
        .expect_err("expired cursor must return 410 Gone");

    assert_eq!(err.status, StatusCode::GONE);
    let detail: serde_json::Value =
        serde_json::from_str(&err.detail).expect("detail must be valid JSON");
    assert_eq!(detail["error"], "cursor expired");
    assert_eq!(detail["code"], "CURSOR_EXPIRED");
}

/// TEST-FDESIGN-001 (inst-sdk-6a): old data timestamp + fresh issued_at → not 410.
/// Verifies cursor TTL is based on issuance time, not data record age.
#[tokio::test]
async fn test_raw_200_cursor_not_expired_old_data() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> =
        Arc::new(AllowAuthZ { tenant_id: Uuid::new_v4() });
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(OkAggPlugin);

    let now = Utc::now();
    // cursor_ts is 48 hours ago (old data record position) but issued_at is 1 second ago (fresh)
    let cursor_ts = now - chrono::Duration::hours(48);
    let from = cursor_ts - chrono::Duration::hours(1);
    let to = now;
    let issued_at = now - chrono::Duration::seconds(1);
    let cursor_str = cursor_encode(cursor_ts, Uuid::new_v4(), issued_at);

    let params = RawQueryParams {
        from,
        to,
        cursor: Some(cursor_str),
        page_size: None,
        usage_type: None,
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
    };

    let result =
        handle_query_raw(Extension(ctx), Extension(authz), Extension(plugin), Query(params)).await;

    // Key assertion: old data timestamp with fresh issued_at must NOT return 410
    assert!(
        result.is_ok(),
        "cursor with old data timestamp but fresh issued_at must not return 410: {result:?}"
    );
}

/// TEST-FDESIGN-001 (inst-sdk-6): cursor encode/decode round-trip test.
#[test]
fn test_cursor_encode_decode_round_trip() {
    let timestamp = Utc::now();
    let id = Uuid::new_v4();
    let issued_at = Utc::now();

    let encoded = cursor_encode(timestamp, id, issued_at);
    let (decoded_ts, decoded_id, decoded_issued_at) =
        crate::api::rest::dto::cursor_decode(&encoded).expect("should decode successfully");

    assert_eq!(decoded_id, id, "UUID must survive encode/decode round-trip");
    // RFC 3339 round-trip preserves seconds and sub-seconds
    assert_eq!(
        decoded_ts.timestamp(),
        timestamp.timestamp(),
        "timestamp seconds must survive encode/decode round-trip"
    );
    assert_eq!(
        decoded_ts.timestamp_subsec_nanos(),
        timestamp.timestamp_subsec_nanos(),
        "timestamp nanoseconds must survive encode/decode round-trip"
    );
    assert_eq!(
        decoded_issued_at.timestamp(),
        issued_at.timestamp(),
        "issued_at seconds must survive encode/decode round-trip"
    );
}
