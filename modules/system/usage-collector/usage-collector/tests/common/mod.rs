#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use async_trait::async_trait;
use authz_resolver_sdk::constraints::{Constraint, InPredicate, Predicate};
use authz_resolver_sdk::models::{
    EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError, DenyReason};
use axum::routing::{get, post};
use axum::{Extension, Router};
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::outbox_migrations;
use modkit_db::{ConnectOpts, connect_db};
use modkit_security::SecurityContext;
use modkit_security::access_scope::pep_properties;
use usage_collector::api::rest::handlers::{handle_create_usage_record, handle_get_module_config};
use usage_collector_sdk::{
    AllowedMetric, ModuleConfig, UsageCollectorClientV1, UsageCollectorError,
    UsageCollectorPluginClientV1, UsageKind, UsageRecord,
};
use usage_emitter::{ScopedUsageEmitter, UsageEmitter, UsageEmitterConfig, UsageEmitterV1};
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

// ── Collector mocks ───────────────────────────────────────────────────────────

pub struct MockUsageCollectorClientV1 {
    pub config: ModuleConfig,
}

#[async_trait]
impl UsageCollectorClientV1 for MockUsageCollectorClientV1 {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(&self, _: &str) -> Result<ModuleConfig, UsageCollectorError> {
        Ok(self.config.clone())
    }
}

#[allow(dead_code)]
pub struct NotFoundCollector;

#[async_trait]
impl UsageCollectorClientV1 for NotFoundCollector {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Err(UsageCollectorError::module_not_found(module_name))
    }
}

// ── Plugin mock ───────────────────────────────────────────────────────────────

pub struct MockUsageCollectorPluginClientV1;

impl MockUsageCollectorPluginClientV1 {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl UsageCollectorPluginClientV1 for MockUsageCollectorPluginClientV1 {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
}

// ── Emitter mock ──────────────────────────────────────────────────────────────

/// Wraps a real `UsageEmitter` because `ScopedUsageEmitter::new()` is `pub(crate)`.
pub struct MockUsageEmitterV1(UsageEmitter);

impl MockUsageEmitterV1 {
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

impl UsageEmitterV1 for MockUsageEmitterV1 {
    fn for_module(&self, module_name: &str) -> ScopedUsageEmitter {
        self.0.for_module(module_name)
    }
}

async fn build_real_emitter(authz: Arc<dyn AuthZResolverClient>) -> UsageEmitter {
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
    let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(MockUsageCollectorClientV1 {
        config: ModuleConfig {
            allowed_metrics: vec![AllowedMetric {
                name: "test.gauge".to_owned(),
                kind: UsageKind::Gauge,
            }],
        },
    });
    UsageEmitter::build(UsageEmitterConfig::default(), db, authz, collector)
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
            Arc::new(MockUsageEmitterV1::with_allow_authz().await) as Arc<dyn UsageEmitterV1>;
        let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(MockUsageCollectorClientV1 {
            config: ModuleConfig {
                allowed_metrics: vec![AllowedMetric {
                    name: "test.gauge".to_owned(),
                    kind: UsageKind::Gauge,
                }],
            },
        });
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        Self::build(emitter, collector, authz, plugin)
    }

    #[allow(dead_code)]
    pub fn with_emitter(emitter: Arc<dyn UsageEmitterV1>) -> Self {
        let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(MockUsageCollectorClientV1 {
            config: ModuleConfig {
                allowed_metrics: vec![AllowedMetric {
                    name: "test.gauge".to_owned(),
                    kind: UsageKind::Gauge,
                }],
            },
        });
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        Self::build(emitter, collector, authz, plugin)
    }

    #[allow(dead_code)]
    pub async fn with_collector(collector: Arc<dyn UsageCollectorClientV1>) -> Self {
        let emitter =
            Arc::new(MockUsageEmitterV1::with_allow_authz().await) as Arc<dyn UsageEmitterV1>;
        let authz: Arc<dyn AuthZResolverClient> =
            Arc::new(MockAuthZResolverClient::allow(Uuid::new_v4()));
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        Self::build(emitter, collector, authz, plugin)
    }

    #[allow(dead_code)]
    pub async fn with_deny_authz() -> Self {
        let emitter =
            Arc::new(MockUsageEmitterV1::with_deny_authz().await) as Arc<dyn UsageEmitterV1>;
        let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(MockUsageCollectorClientV1 {
            config: ModuleConfig {
                allowed_metrics: vec![AllowedMetric {
                    name: "test.gauge".to_owned(),
                    kind: UsageKind::Gauge,
                }],
            },
        });
        let authz: Arc<dyn AuthZResolverClient> = Arc::new(MockAuthZResolverClient::deny());
        let plugin: Arc<dyn UsageCollectorPluginClientV1> =
            Arc::new(MockUsageCollectorPluginClientV1::new());
        Self::build(emitter, collector, authz, plugin)
    }

    fn build(
        emitter: Arc<dyn UsageEmitterV1>,
        collector: Arc<dyn UsageCollectorClientV1>,
        authz: Arc<dyn AuthZResolverClient>,
        plugin: Arc<dyn UsageCollectorPluginClientV1>,
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
            .layer(Extension(emitter))
            .layer(Extension(collector))
            .layer(Extension(authz))
            .layer(Extension(plugin))
            .layer(Extension(ctx));
        Self { router }
    }
}
