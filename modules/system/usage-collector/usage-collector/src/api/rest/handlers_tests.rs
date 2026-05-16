//! Unit tests for REST handlers and `domain_error_to_problem` / `canonical_error_to_problem`.

use std::collections::HashMap;
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
use modkit::client_hub::{ClientHub, ClientScope};
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::outbox_migrations;
use modkit_db::{ConnectOpts, connect_db};
use modkit_odata::SortDir;
use modkit_security::SecurityContext;
use modkit_security::access_scope::pep_properties;
use types_registry_sdk::testing::make_test_instance;
use types_registry_sdk::{
    GtsInstance, GtsTypeSchema, InstanceQuery, RegisterResult, TypeSchemaQuery,
    TypesRegistryClient, TypesRegistryError,
};
use usage_collector_sdk::{
    AggregationFn, AggregationQuery, AggregationResult, AllowedMetric, UsageCollectorError, CursorV1,
    GroupByDimension, ModuleConfig, Page, PageInfo, RawQuery, UsageCollectorClientV1,
    UsageCollectorPluginClientV1, UsageCollectorPluginSpecV1, UsageKind, UsageRecord,
    UsageRecordError,
};
use usage_emitter::{UsageEmitterFactory, UsageEmitterFactoryV1};
use uuid::Uuid;

use super::canonical_error_to_problem;
use super::domain_error_to_problem;
use super::handle_create_usage_record;
use super::handle_get_module_config;
use super::handle_query_aggregated;
use super::handle_query_raw;
use super::{DEFAULT_PAGE_SIZE, MAX_FILTER_STRING_LEN, MAX_PAGE_SIZE, MAX_QUERY_TIME_RANGE};
use crate::api::rest::dto::{AggregatedQueryParams, CreateUsageRecordRequest, RawQueryParams};
use crate::config::{MetricConfig, UsageCollectorConfig};
use crate::domain::{DomainError, Service};

// ── canonical_error_to_problem ──────────────────────────────────────

#[test]
fn canonical_internal_maps_to_500() {
    let err = UsageCollectorError::internal("something broke").create();
    let p = canonical_error_to_problem(&err);
    assert_eq!(p.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn canonical_not_found_maps_to_404() {
    let err = UsageRecordError::not_found("module not configured")
        .with_resource("test-module")
        .create();
    let p = canonical_error_to_problem(&err);
    assert_eq!(p.status, StatusCode::NOT_FOUND);
    assert_eq!(p.detail, "module not configured");
}

#[test]
fn canonical_service_unavailable_maps_to_503() {
    let err = UsageCollectorError::service_unavailable()
        .with_detail("transport error")
        .create();
    let p = canonical_error_to_problem(&err);
    assert_eq!(p.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(p.detail, "transport error");
}

#[test]
fn canonical_deadline_exceeded_maps_to_504() {
    let err = UsageRecordError::deadline_exceeded("plugin call timed out").create();
    let p = canonical_error_to_problem(&err);
    assert_eq!(p.status, StatusCode::GATEWAY_TIMEOUT);
}

#[test]
fn canonical_resource_exhausted_maps_to_429() {
    let err = UsageRecordError::resource_exhausted("query result too large")
        .with_quota_violation("rows", "row count exceeds limit")
        .create();
    let p = canonical_error_to_problem(&err);
    assert_eq!(p.status, StatusCode::TOO_MANY_REQUESTS);
}

// ── domain_error_to_problem ─────────────────────────────────────────

#[test]
fn domain_module_not_configured_maps_to_404() {
    let p = domain_error_to_problem(&DomainError::ModuleNotConfigured {
        module: "unknown".to_owned(),
    });
    assert_eq!(p.status, StatusCode::NOT_FOUND);
}

#[test]
fn domain_timeout_maps_to_504() {
    let p = domain_error_to_problem(&DomainError::Timeout);
    assert_eq!(p.status, StatusCode::GATEWAY_TIMEOUT);
}

#[test]
fn domain_circuit_open_maps_to_503() {
    let p = domain_error_to_problem(&DomainError::CircuitOpen);
    assert_eq!(p.status, StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn domain_plugin_unavailable_maps_to_503() {
    let p = domain_error_to_problem(&DomainError::PluginUnavailable {
        gts_id: "x".to_owned(),
        reason: "down".to_owned(),
    });
    assert_eq!(p.status, StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn domain_internal_maps_to_500() {
    let p = domain_error_to_problem(&DomainError::Internal("boom".to_owned()));
    assert_eq!(p.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn domain_plugin_preserves_canonical_status() {
    let canonical = UsageRecordError::resource_exhausted("query result too large")
        .with_quota_violation("rows", "exceeds limit")
        .create();
    let p = domain_error_to_problem(&DomainError::Plugin(canonical));
    assert_eq!(p.status, StatusCode::TOO_MANY_REQUESTS);
}

// ── service builder helpers ─────────────────────────────────────────

struct StubRegistry {
    instances: Vec<GtsInstance>,
}

#[async_trait]
impl TypesRegistryClient for StubRegistry {
    async fn register(
        &self,
        _: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
    async fn register_type_schemas(
        &self,
        _: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
    async fn get_type_schema(&self, _: &str) -> Result<GtsTypeSchema, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_type_schema_by_uuid(
        &self,
        _: Uuid,
    ) -> Result<GtsTypeSchema, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_type_schemas(
        &self,
        _: Vec<String>,
    ) -> HashMap<String, Result<GtsTypeSchema, TypesRegistryError>> {
        unimplemented!()
    }
    async fn get_type_schemas_by_uuid(
        &self,
        _: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsTypeSchema, TypesRegistryError>> {
        unimplemented!()
    }
    async fn list_type_schemas(
        &self,
        _: TypeSchemaQuery,
    ) -> Result<Vec<GtsTypeSchema>, TypesRegistryError> {
        unimplemented!()
    }
    async fn register_instances(
        &self,
        _: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
    async fn get_instance(&self, _: &str) -> Result<GtsInstance, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_instance_by_uuid(&self, _: Uuid) -> Result<GtsInstance, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_instances(
        &self,
        _: Vec<String>,
    ) -> HashMap<String, Result<GtsInstance, TypesRegistryError>> {
        unimplemented!()
    }
    async fn get_instances_by_uuid(
        &self,
        _: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsInstance, TypesRegistryError>> {
        unimplemented!()
    }
    async fn list_instances(
        &self,
        _: InstanceQuery,
    ) -> Result<Vec<GtsInstance>, TypesRegistryError> {
        Ok(self.instances.clone())
    }
}

fn plugin_content(gts_id: &str, vendor: &str) -> serde_json::Value {
    serde_json::json!({
        "id": gts_id,
        "vendor": vendor,
        "priority": 0,
        "properties": {},
    })
}

fn service_with(
    plugin: Arc<dyn UsageCollectorPluginClientV1>,
    metrics: HashMap<String, MetricConfig>,
) -> Arc<Service> {
    let instance_id = format!(
        "{}test.usage.mock.handlers_test.v1",
        UsageCollectorPluginSpecV1::gts_schema_id()
    );
    let hub = Arc::new(ClientHub::default());
    hub.register::<dyn TypesRegistryClient>(Arc::new(StubRegistry {
        instances: vec![make_test_instance(
            &instance_id,
            plugin_content(&instance_id, "cyberfabric"),
        )],
    }));
    hub.register_scoped::<dyn UsageCollectorPluginClientV1>(
        ClientScope::gts_id(&instance_id),
        plugin,
    );
    Arc::new(Service::new(
        UsageCollectorConfig {
            metrics,
            ..UsageCollectorConfig::default()
        },
        hub,
    ))
}

fn service_with_plugin(plugin: Arc<dyn UsageCollectorPluginClientV1>) -> Arc<Service> {
    service_with(plugin, HashMap::new())
}

struct OkPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for OkPlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }
    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        Ok(Page::empty(DEFAULT_PAGE_SIZE as u64))
    }
}

// ── handle_get_module_config ──────────────────────────────────────

fn metrics_with(name: &str, kind: UsageKind) -> HashMap<String, MetricConfig> {
    let mut m = HashMap::new();
    m.insert(
        name.to_owned(),
        MetricConfig {
            kind,
            modules: None,
        },
    );
    m
}

#[tokio::test]
async fn get_module_config_handler_returns_allowed_metrics() {
    let svc = service_with(Arc::new(OkPlugin), metrics_with("cpu.usage", UsageKind::Gauge));
    let result = handle_get_module_config(Path("my-module".to_owned()), Extension(svc)).await;

    let axum::Json(resp) = result.expect("handler should succeed");
    assert_eq!(resp.allowed_metrics.len(), 1);
    assert_eq!(resp.allowed_metrics[0].name, "cpu.usage");
}

#[tokio::test]
async fn get_module_config_handler_returns_404_for_unknown_module() {
    let svc = service_with_plugin(Arc::new(OkPlugin));
    let result =
        handle_get_module_config(Path("unknown-module".to_owned()), Extension(svc)).await;

    let err = result.expect_err("handler should return 404");
    assert_eq!(err.status, StatusCode::NOT_FOUND);
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

async fn build_handler_emitter(authz: Arc<dyn AuthZResolverClient>) -> Arc<dyn UsageEmitterFactoryV1> {
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
    let emitter = UsageEmitterFactory::build(
        usage_emitter::UsageEmitterConfig::default(),
        db,
        authz,
        Arc::new(FixedConfigCollector),
    )
    .await
    .unwrap();
    Arc::new(emitter) as Arc<dyn UsageEmitterFactoryV1>
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
        subject_id: Some(subject_id),
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
        subject_id: Some(subject_id),
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

/// Mock `AuthZ` that allows all requests with a single tenant constraint.
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

/// Mock `AuthZ` that denies all requests.
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

/// Mock `AuthZ` that returns a network/infrastructure error.
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

/// Mock plugin that returns `ResourceExhausted` (query result too large).
struct TooLargePlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for TooLargePlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Err(UsageRecordError::resource_exhausted(
            "query result too large: 10001 rows exceeds limit of 10000",
        )
        .with_quota_violation("rows", "10001 rows exceeds limit of 10000")
        .create())
    }
    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        Ok(Page::empty(DEFAULT_PAGE_SIZE as u64))
    }
}

/// Mock plugin that returns a service unavailable storage error from `query_aggregated`.
struct InternalErrorPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for InternalErrorPlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Err(UsageCollectorError::service_unavailable()
            .with_detail("storage backend unavailable")
            .create())
    }
    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        Ok(Page::empty(DEFAULT_PAGE_SIZE as u64))
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

#[tokio::test]
async fn test_aggregated_200_empty_result() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let result = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_params()),
    )
    .await;

    let Json(body) = result.expect("handler should succeed");
    assert!(body.is_empty());
}

#[test]
fn test_aggregated_400_missing_fn() {
    let json = serde_json::json!({
        "from": "2026-01-01T00:00:00Z",
        "to": "2026-02-01T00:00:00Z",
    });
    let result = serde_json::from_value::<AggregatedQueryParams>(json);
    assert!(result.is_err());
}

#[test]
fn test_aggregated_400_missing_from() {
    let json = serde_json::json!({
        "fn": "sum",
        "to": "2026-02-01T00:00:00Z",
    });
    let result = serde_json::from_value::<AggregatedQueryParams>(json);
    assert!(result.is_err());
}

#[test]
fn test_aggregated_400_missing_to() {
    let json = serde_json::json!({
        "fn": "sum",
        "from": "2026-01-01T00:00:00Z",
    });
    let result = serde_json::from_value::<AggregatedQueryParams>(json);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_aggregated_400_time_range_not_ascending() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let now = Utc::now();
    let mut params = valid_params();
    params.from = now;
    params.to = now - chrono::Duration::hours(1);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
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
    );
}

#[tokio::test]
async fn test_aggregated_400_time_range_too_wide() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let from = Utc::now() - chrono::Duration::hours(1);
    let mut params = valid_params();
    params.from = from;
    params.to = from
        + chrono::Duration::from_std(MAX_QUERY_TIME_RANGE).unwrap()
        + chrono::Duration::seconds(1);

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("time range exceeding MAX_QUERY_TIME_RANGE must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_aggregated_400_bucket_size_absent_with_time_bucket() {
    use usage_collector_sdk::BucketSize;

    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_params();
    params.group_by = vec![GroupByDimension::TimeBucket(BucketSize::Day)];
    params.bucket_size = None;

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("missing bucket_size with time_bucket must return error");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_aggregated_400_filter_string_too_long() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_params();
    params.usage_type = Some("a".repeat(257));

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("usage_type exceeding max length must return error");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_aggregated_403_pdp_deny() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(DenyAuthZ);
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_params()),
    )
    .await
    .expect_err("PDP deny must return 403");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
}

#[tokio::test]
async fn test_aggregated_403_pdp_non_denied_error() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(NetworkErrorAuthZ);
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_params()),
    )
    .await
    .expect_err("PDP network error must return 403 (fail-closed)");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
}

#[tokio::test]
async fn test_aggregated_503_plugin_error() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(InternalErrorPlugin));

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_params()),
    )
    .await
    .expect_err("plugin internal error must return 503");

    assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(err.detail.contains("storage backend"));
}

#[tokio::test]
async fn test_aggregated_429_query_result_too_large() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(TooLargePlugin));

    let err = handle_query_aggregated(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_params()),
    )
    .await
    .expect_err("ResourceExhausted must return 429");

    assert_eq!(err.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(err.detail.contains("query result too large"));
}

// ── handle_query_raw ──────────────────────────────────────────────────────────

/// Mock plugin that returns a non-empty Page with a `next_cursor`.
struct OkRawWithCursorPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for OkRawWithCursorPlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }
    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
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
        let cursor = CursorV1 {
            k: vec![Utc::now().to_rfc3339(), Uuid::new_v4().to_string()],
            o: SortDir::Asc,
            s: "+timestamp,+id".to_owned(),
            f: None,
            d: "fwd".to_owned(),
        };
        let next_cursor = cursor
            .encode()
            .expect("CursorV1 encode is infallible for valid data");
        Ok(Page::new(
            vec![record],
            PageInfo {
                next_cursor: Some(next_cursor),
                prev_cursor: None,
                limit: DEFAULT_PAGE_SIZE as u64,
            },
        ))
    }
}

/// Mock plugin that returns a service unavailable error from `query_raw`.
struct RawErrorPlugin;

#[async_trait]
impl UsageCollectorPluginClientV1 for RawErrorPlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }
    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        Err(UsageCollectorError::service_unavailable()
            .with_detail("storage backend unavailable")
            .create())
    }
}

/// Mock plugin that captures the `page_size` field from the `RawQuery`.
struct CapturingRawPlugin {
    captured_page_size: Arc<std::sync::Mutex<Option<usize>>>,
}

#[async_trait]
impl UsageCollectorPluginClientV1 for CapturingRawPlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }
    async fn query_raw(&self, query: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        *self.captured_page_size.lock().unwrap() = Some(query.page_size);
        Ok(Page::empty(DEFAULT_PAGE_SIZE as u64))
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

#[tokio::test]
async fn test_raw_200_empty_final_page() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let result = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_raw_params()),
    )
    .await;

    let axum::Json(body) = result.expect("handler should succeed");
    assert!(body.items.is_empty());
    assert!(body.page_info.next_cursor.is_none());
}

#[tokio::test]
async fn test_raw_200_with_items_and_next_cursor() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkRawWithCursorPlugin));

    let result = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_raw_params()),
    )
    .await;

    let axum::Json(body) = result.expect("handler should succeed");
    assert!(!body.items.is_empty());
    let cursor_str = body
        .page_info
        .next_cursor
        .expect("next_cursor must be present");
    assert!(!cursor_str.is_empty());
}

#[tokio::test]
async fn test_raw_400_malformed_cursor() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_raw_params();
    params.cursor = Some("not-valid-base64!!!".to_owned());

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("malformed cursor must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_raw_400_cursor_timestamp_out_of_range() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let from = Utc::now() - chrono::Duration::hours(2);
    let to = Utc::now() - chrono::Duration::hours(1);
    let cursor_ts = Utc::now();
    let cursor = CursorV1 {
        k: vec![cursor_ts.to_rfc3339(), Uuid::new_v4().to_string()],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let cursor_str = cursor
        .encode()
        .expect("CursorV1 encode is infallible for valid data");

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

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("cursor outside [from, to] must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_raw_400_page_size_zero() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_raw_params();
    params.page_size = Some(0);

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("page_size=0 must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_raw_400_page_size_too_large() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_raw_params();
    params.page_size = Some(MAX_PAGE_SIZE + 1);

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("page_size > MAX_PAGE_SIZE must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_raw_400_time_range_too_wide() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let from = Utc::now() - chrono::Duration::hours(1);
    let mut params = valid_raw_params();
    params.from = from;
    params.to = from
        + chrono::Duration::from_std(MAX_QUERY_TIME_RANGE).unwrap()
        + chrono::Duration::seconds(1);

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("time range exceeding MAX_QUERY_TIME_RANGE must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_raw_200_max_page_size() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_raw_params();
    params.page_size = Some(MAX_PAGE_SIZE);

    let result = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_raw_200_default_page_size() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let captured = Arc::new(std::sync::Mutex::new(None));
    let plugin: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(CapturingRawPlugin {
        captured_page_size: Arc::clone(&captured),
    });
    let svc = service_with_plugin(plugin);

    let mut params = valid_raw_params();
    params.page_size = None;

    let result = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(*captured.lock().unwrap(), Some(DEFAULT_PAGE_SIZE));
}

#[tokio::test]
async fn test_raw_400_filter_string_too_long() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let mut params = valid_raw_params();
    params.usage_type = Some("a".repeat(MAX_FILTER_STRING_LEN + 1));

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("oversized filter string must return 400");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_raw_403_pdp_deny() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(DenyAuthZ);
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_raw_params()),
    )
    .await
    .expect_err("PDP deny must return 403");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
}

#[tokio::test]
async fn test_raw_403_pdp_non_denied_error() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(NetworkErrorAuthZ);
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_raw_params()),
    )
    .await
    .expect_err("PDP network error must return 403 (fail-closed)");

    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert_eq!(err.detail, r#"{"error":"forbidden"}"#);
}

#[tokio::test]
async fn test_raw_503_plugin_error() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(RawErrorPlugin));

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(valid_raw_params()),
    )
    .await
    .expect_err("plugin service unavailable must return 503");

    assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(err.detail.contains("storage backend"));
}

#[tokio::test]
async fn test_raw_200_cursor_within_range_succeeds() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let now = Utc::now();
    let cursor_ts = now - chrono::Duration::hours(1);
    let from = cursor_ts - chrono::Duration::hours(1);
    let to = now;
    let cursor = CursorV1 {
        k: vec![cursor_ts.to_rfc3339(), Uuid::new_v4().to_string()],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let cursor_str = cursor
        .encode()
        .expect("CursorV1 encode is infallible for valid data");

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

    let result = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_raw_200_cursor_not_expired_old_data() {
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let now = Utc::now();
    let cursor_ts = now - chrono::Duration::hours(48);
    let from = cursor_ts - chrono::Duration::hours(1);
    let to = now;
    let cursor = CursorV1 {
        k: vec![cursor_ts.to_rfc3339(), Uuid::new_v4().to_string()],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let cursor_str = cursor
        .encode()
        .expect("CursorV1 encode is infallible for valid data");

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

    let result = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_raw_400_cursor_single_key() {
    // Exercises the `cursor.k.len() < 2` guard in `decode_and_validate_cursor`.
    // An empty `k` would be rejected earlier by `CursorV1::decode` itself, so
    // we use exactly one key to ensure decode succeeds and the length guard fires.
    let ctx = test_ctx();
    let authz: Arc<dyn AuthZResolverClient> = Arc::new(AllowAuthZ {
        tenant_id: Uuid::new_v4(),
    });
    let svc = service_with_plugin(Arc::new(OkPlugin));

    let now = Utc::now();
    let cursor = CursorV1 {
        k: vec![now.to_rfc3339()],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let cursor_str = cursor
        .encode()
        .expect("CursorV1 encode is infallible for valid data");

    let params = RawQueryParams {
        from: now - chrono::Duration::hours(1),
        to: now,
        cursor: Some(cursor_str),
        page_size: None,
        usage_type: None,
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
    };

    let err = handle_query_raw(
        Extension(ctx),
        Extension(authz),
        Extension(svc),
        Query(params),
    )
    .await
    .expect_err("cursor with fewer than 2 keys must be rejected");
    assert_eq!(err.status, StatusCode::BAD_REQUEST);
}

#[test]
fn test_cursor_encode_decode_round_trip() {
    let timestamp = Utc::now();
    let id = Uuid::new_v4();
    let cursor = CursorV1 {
        k: vec![timestamp.to_rfc3339(), id.to_string()],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };

    let encoded = cursor
        .encode()
        .expect("CursorV1 encode is infallible for valid data");
    let decoded = CursorV1::decode(&encoded)
        .expect("CursorV1 decode must succeed for freshly encoded cursor");

    assert_eq!(decoded.k[1], id.to_string());
    assert_eq!(decoded.k[0], timestamp.to_rfc3339());
    assert!(matches!(decoded.o, SortDir::Asc));
    assert_eq!(decoded.s, "+timestamp,+id");
    assert_eq!(decoded.d, "fwd");
}
