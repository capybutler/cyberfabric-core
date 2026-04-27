//! `usage-collector-rest-client` `ModKit` module.

use std::sync::Arc;

use async_trait::async_trait;
use authn_resolver_sdk::AuthNResolverClient;
use authz_resolver_sdk::AuthZResolverClient;
use modkit::contracts::DatabaseCapability;
use modkit::{Module, ModuleCtx};
use sea_orm_migration::MigrationTrait;
use tracing::info;
use usage_emitter::{UsageEmitter, UsageEmitterV1};

use crate::config::UsageCollectorRestClientConfig;
use crate::infra::UsageCollectorRestClient;

/// Satisfies the `"usage-collector-client"` `ModKit` dependency for separate binaries
/// by sending usage records to a remote collector REST API with s2s bearer auth.
#[modkit::module(
    name = "usage-collector-rest-client",
    deps = ["authn-resolver", "authz-resolver"],
    capabilities = [db],
)]
#[derive(Default)]
struct UsageCollectorRestClientModule;

#[async_trait]
impl Module for UsageCollectorRestClientModule {
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        let cfg: UsageCollectorRestClientConfig = ctx.config_expanded()?;
        info!(
            %cfg.base_url,
            ?cfg.request_timeout,
            "Loaded {} configuration",
            Self::MODULE_NAME,
        );

        let db = ctx
            .db_required()
            .map_err(|e| anyhow::anyhow!("{}: db not available: {e}", Self::MODULE_NAME))?
            .db();

        let authentication = ctx
            .client_hub()
            .get::<dyn AuthNResolverClient>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "{}: AuthNResolverClient not registered: {e}",
                    Self::MODULE_NAME
                )
            })?;

        let authorization = ctx
            .client_hub()
            .get::<dyn AuthZResolverClient>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "{}: AuthZResolverClient not registered: {e}",
                    Self::MODULE_NAME
                )
            })?;

        let collector = UsageCollectorRestClient::new(&cfg, authentication).map_err(|e| {
            anyhow::anyhow!("{}: failed to build HTTP client: {e}", Self::MODULE_NAME)
        })?;
        let collector = Arc::new(collector);

        let emitter = UsageEmitter::build(cfg.emitter, db, authorization, collector).await?;
        let emitter = Arc::new(emitter);
        ctx.client_hub().register::<dyn UsageEmitterV1>(emitter);

        Ok(())
    }
}

impl DatabaseCapability for UsageCollectorRestClientModule {
    fn migrations(&self) -> Vec<Box<dyn MigrationTrait>> {
        info!("Providing {} database migrations", Self::MODULE_NAME);
        modkit_db::outbox::outbox_migrations()
    }
}
