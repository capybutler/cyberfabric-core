#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use authz_resolver_sdk::constraints::{Constraint, InPredicate, Predicate};
use authz_resolver_sdk::models::{
    EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError, DenyReason};
use axum::routing::{get, post};
use axum::{Extension, Router};
use chrono::Utc;
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
use usage_collector::api::rest::handlers::{
    handle_create_usage_record, handle_get_module_config, handle_query_aggregated, handle_query_raw,
};
use usage_collector::config::{MetricConfig, UsageCollectorConfig};
use usage_collector::domain::Service;
use usage_collector_sdk::{
    AggregationQuery, AggregationResult, AllowedMetric, UsageCollectorError, CursorV1, ModuleConfig,
    Page, PageInfo, RawQuery, UsageCollectorClientV1, UsageCollectorPluginClientV1,
    UsageCollectorPluginSpecV1, UsageKind, UsageRecord, UsageRecordError,
};
use usage_emitter::{UsageEmitter, UsageEmitterConfig, UsageEmitterFactory, UsageEmitterFactoryV1};
use uuid::Uuid;

// ── AuthZ mocks ───────────────────────────────────────────────────────────────

pub struct MockAuthZResolverClient {
    allow: bool,
    tenant_id: Uuid,
}

impl MockAuthZResolverClient {
    pub fn allow(tenant_id: Uuid) -> Self {
        Self {
            allow: true,
            tenant_id,
        }
    }

    #[allow(dead_code)]
    pub fn deny() -> Self {
        Self {
            allow: false,
            tenant_id: Uuid::nil(),
        }
    }
}

#[async_trait]
impl AuthZResolverClient for MockAuthZResolverClient {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        if self.allow {
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
        } else {
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
}

// ── Collector mocks (used by the embedded emitter only) ───────────────────────

pub struct EmitterCollector {
    config: ModuleConfig,
}

impl EmitterCollector {
    fn new() -> Self {
        Self {
            config: ModuleConfig {
                allowed_metrics: vec![AllowedMetric {
                    name: "test.gauge".to_owned(),
                    kind: UsageKind::Gauge,
                }],
            },
        }
    }
}

#[async_trait]
impl UsageCollectorClientV1 for EmitterCollector {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn get_module_config(&self, _: &str) -> Result<ModuleConfig, UsageCollectorError> {
        Ok(self.config.clone())
    }
}

// ── Plugin mock ───────────────────────────────────────────────────────────────

pub struct MockUsageCollectorPluginClientV1 {
    too_large: bool,
    with_cursor: bool,
}

impl MockUsageCollectorPluginClientV1 {
    pub fn new() -> Self {
        Self {
            too_large: false,
            with_cursor: false,
        }
    }

    #[allow(dead_code)]
    pub fn too_large() -> Self {
        Self {
            too_large: true,
            with_cursor: false,
        }
    }

    #[allow(dead_code)]
    pub fn with_raw_cursor() -> Self {
        Self {
            too_large: false,
            with_cursor: true,
        }
    }
}

#[async_trait]
impl UsageCollectorPluginClientV1 for MockUsageCollectorPluginClientV1 {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        if self.too_large {
            return Err(UsageRecordError::resource_exhausted(
                "query result too large: 10001 rows exceeds limit of 10000",
            )
            .with_quota_violation("rows", "10001 rows exceeds limit of 10000")
            .create());
        }
        Ok(vec![])
    }

    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        if self.with_cursor {
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
            return Ok(Page::new(
                vec![record],
                PageInfo {
                    next_cursor: Some(next_cursor),
                    prev_cursor: None,
                    limit: 100,
                },
            ));
        }
        Ok(Page::empty(100))
    }
}

// ── Service builder ───────────────────────────────────────────────────────────

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

#[allow(dead_code)]
pub fn build_service(
    plugin: Arc<dyn UsageCollectorPluginClientV1>,
    metrics: HashMap<String, MetricConfig>,
) -> Arc<Service> {
    let instance_id = format!(
        "{}test.usage.mock.harness_test.v1",
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

// ── Emitter mock ──────────────────────────────────────────────────────────────

/// Wraps a real [`UsageEmitterFactory`] because [`UsageEmitter::new`] is `pub(crate)`.
pub struct MockUsageEmitterFactoryV1(UsageEmitterFactory);

impl MockUsageEmitterFactoryV1 {
    pub async fn with_allow_authz() -> Self {
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        Self(build_real_emitter(authz).await)
    }

    #[allow(dead_code)]
    pub async fn with_deny_authz() -> Self {
        let authz: Arc<dyn AuthZResolverClient> = Arc::new(MockAuthZResolverClient::deny());
        Self(build_real_emitter(authz).await)
    }
}

impl UsageEmitterFactoryV1 for MockUsageEmitterFactoryV1 {
    fn for_module(&self, module_name: &str) -> UsageEmitter {
        self.0.for_module(module_name)
    }
}

async fn build_real_emitter(authz: Arc<dyn AuthZResolverClient>) -> UsageEmitterFactory {
    let db_name = format!("uc_gw_{}", Uuid::new_v4().simple());
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
    let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(EmitterCollector::new());
    UsageEmitterFactory::build(UsageEmitterConfig::default(), db, authz, collector)
        .await
        .unwrap()
}

// ── AppHarness ────────────────────────────────────────────────────────────────

pub struct AppHarness {
    pub router: Router,
}

impl AppHarness {
    #[allow(dead_code)]
    pub async fn new() -> Self {
        let emitter =
            Arc::new(MockUsageEmitterFactoryV1::with_allow_authz().await) as Arc<dyn UsageEmitterFactoryV1>;
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        let service = build_service(plugin, HashMap::new());
        Self::build(emitter, service, authz)
    }

    #[allow(dead_code)]
    pub async fn with_plugin(plugin: Arc<dyn UsageCollectorPluginClientV1>) -> Self {
        let emitter =
            Arc::new(MockUsageEmitterFactoryV1::with_allow_authz().await) as Arc<dyn UsageEmitterFactoryV1>;
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let service = build_service(plugin, HashMap::new());
        Self::build(emitter, service, authz)
    }

    #[allow(dead_code)]
    pub fn with_emitter(emitter: Arc<dyn UsageEmitterFactoryV1>) -> Self {
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        let service = build_service(plugin, HashMap::new());
        Self::build(emitter, service, authz)
    }

    #[allow(dead_code)]
    pub async fn with_metrics(metrics: HashMap<String, MetricConfig>) -> Self {
        let emitter =
            Arc::new(MockUsageEmitterFactoryV1::with_allow_authz().await) as Arc<dyn UsageEmitterFactoryV1>;
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        let service = build_service(plugin, metrics);
        Self::build(emitter, service, authz)
    }

    #[allow(dead_code)]
    pub async fn with_deny_authz() -> Self {
        let emitter =
            Arc::new(MockUsageEmitterFactoryV1::with_deny_authz().await) as Arc<dyn UsageEmitterFactoryV1>;
        let authz: Arc<dyn AuthZResolverClient> = Arc::new(MockAuthZResolverClient::deny());
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        let service = build_service(plugin, HashMap::new());
        Self::build(emitter, service, authz)
    }

    fn build(
        emitter: Arc<dyn UsageEmitterFactoryV1>,
        service: Arc<Service>,
        authz: Arc<dyn AuthZResolverClient>,
    ) -> Self {
        let ctx = SecurityContext::builder()
            .subject_id(Uuid::new_v4())
            .subject_tenant_id(Uuid::new_v4())
            .build()
            .unwrap();
        let router = Router::new()
            .route(
                "/usage-collector/v1/records",
                post(handle_create_usage_record),
            )
            .route(
                "/usage-collector/v1/modules/{module_name}/config",
                get(handle_get_module_config),
            )
            .route(
                "/usage-collector/v1/aggregated",
                get(handle_query_aggregated),
            )
            .route("/usage-collector/v1/raw", get(handle_query_raw))
            .layer(Extension(emitter))
            .layer(Extension(service))
            .layer(Extension(authz))
            .layer(Extension(ctx));
        Self { router }
    }
}

/// Percent-encode a `DateTime<Utc>` for use in URL query strings.
#[allow(dead_code)]
pub fn encode_dt(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.to_rfc3339().replace('+', "%2B").replace(':', "%3A")
}
