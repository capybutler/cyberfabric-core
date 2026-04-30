//! Usage-collector gateway `ModKit` module.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use authz_resolver_sdk::AuthZResolverClient;
use axum::Router;
use modkit::api::OpenApiRegistry;
use modkit::contracts::{DatabaseCapability, RestApiCapability};
use modkit::{Module, ModuleCtx};
use sea_orm_migration::MigrationTrait;
use tracing::info;
use types_registry_sdk::{RegisterResult, TypesRegistryClient};
use usage_collector_sdk::{
    UsageCollectorClientV1, UsageCollectorPluginClientV1, UsageCollectorStoragePluginSpecV1,
};
use usage_emitter::{UsageEmitter, UsageEmitterV1};

use crate::api::rest::routes;
use crate::config::UsageCollectorConfig;
use crate::domain::UsageCollectorLocalClient;

/// Usage collector gateway: registers storage plugin schema, resolves plugins via GTS,
/// exposes `dyn UsageCollectorClientV1` for outbox delivery, and wires REST endpoints
/// via `DatabaseCapability` and `RestApiCapability`.
#[modkit::module(
    name = "usage-collector",
    deps = ["authz-resolver", "types-registry"],
    capabilities = [db, rest],
)]
#[derive(Default)]
pub struct UsageCollectorModule {
    /// Gateway collector client stored during `init()` for use in `register_rest()`.
    collector: OnceLock<Arc<dyn UsageCollectorClientV1>>,
    /// Plugin proxy stored during `init()` for injection into query handlers.
    plugin_client: OnceLock<Arc<dyn UsageCollectorPluginClientV1>>,
}

#[async_trait]
impl Module for UsageCollectorModule {
    #[tracing::instrument(skip_all, fields(vendor))]
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        let cfg: UsageCollectorConfig = ctx.config_or_default()?;
        cfg.validate()?;
        tracing::Span::current().record("vendor", &cfg.vendor);
        info!(
            cfg.vendor,
            ?cfg.plugin_timeout,
            "Loaded {} configuration",
            Self::MODULE_NAME,
        );

        let registry = ctx.client_hub().get::<dyn TypesRegistryClient>()?;
        let schema_str = UsageCollectorStoragePluginSpecV1::gts_schema_with_refs_as_string();
        let schema_json: serde_json::Value = serde_json::from_str(&schema_str)?;
        let results = registry.register(vec![schema_json]).await?;
        RegisterResult::ensure_all_ok(&results)?;
        info!(
            schema_id = %UsageCollectorStoragePluginSpecV1::gts_schema_id(),
            "Registered {} storage plugin schema in types-registry",
            Self::MODULE_NAME,
        );

        let db = ctx
            .db_required()
            .map_err(|e| anyhow::anyhow!("{}: db not available: {e}", Self::MODULE_NAME))?
            .db();

        let authz = ctx
            .client_hub()
            .get::<dyn AuthZResolverClient>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "{}: AuthZResolverClient not registered: {e}",
                    Self::MODULE_NAME
                )
            })?;

        let local = Arc::new(UsageCollectorLocalClient::new(
            cfg.clone(),
            ctx.client_hub(),
        ));
        let plugin_client = UsageCollectorLocalClient::as_plugin_client(Arc::clone(&local));
        let collector = local as Arc<dyn UsageCollectorClientV1>;

        let emitter = UsageEmitter::build(cfg.emitter, db, authz, Arc::clone(&collector)).await?;
        ctx.client_hub()
            .register::<dyn UsageEmitterV1>(Arc::new(emitter));

        self.collector
            .set(collector)
            .map_err(|_| anyhow::anyhow!("{}: collector already initialized", Self::MODULE_NAME))?;

        self.plugin_client.set(plugin_client).map_err(|_| {
            anyhow::anyhow!("{}: plugin_client already initialized", Self::MODULE_NAME)
        })?;

        Ok(())
    }
}

impl DatabaseCapability for UsageCollectorModule {
    fn migrations(&self) -> Vec<Box<dyn MigrationTrait>> {
        info!("Providing {} database migrations", Self::MODULE_NAME);
        modkit_db::outbox::outbox_migrations()
    }
}

impl RestApiCapability for UsageCollectorModule {
    fn register_rest(
        &self,
        ctx: &ModuleCtx,
        router: Router,
        openapi: &dyn OpenApiRegistry,
    ) -> anyhow::Result<Router> {
        tracing::info!("Registering {} REST routes", Self::MODULE_NAME);

        let emitter = ctx.client_hub().get::<dyn UsageEmitterV1>().map_err(|e| {
            anyhow::anyhow!("{}: UsageEmitterV1 not registered: {e}", Self::MODULE_NAME)
        })?;

        let collector = self
            .collector
            .get()
            .ok_or_else(|| anyhow::anyhow!("{}: collector not initialized", Self::MODULE_NAME))?;

        let plugin_client = self.plugin_client.get().ok_or_else(|| {
            anyhow::anyhow!("{}: plugin_client not initialized", Self::MODULE_NAME)
        })?;

        let authz_client = ctx
            .client_hub()
            .get::<dyn AuthZResolverClient>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "{}: AuthZResolverClient not registered: {e}",
                    Self::MODULE_NAME
                )
            })?;

        let router = routes::register_routes(
            router,
            openapi,
            emitter,
            Arc::clone(collector),
            authz_client,
            Arc::clone(plugin_client),
        );

        tracing::info!("{} REST routes registered", Self::MODULE_NAME);
        Ok(router)
    }
}
