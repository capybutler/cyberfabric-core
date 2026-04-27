use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use authz_resolver_sdk::models::BarrierMode;
use authz_resolver_sdk::pep::{AccessRequest, PolicyEnforcer};
use modkit_db::Db;
use modkit_db::outbox::Outbox;
use modkit_security::{SecurityContext, pep_properties};
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use uuid::Uuid;

use crate::authorized_emitter::AuthorizedUsageEmitter;
use crate::config::UsageEmitterConfig;
use crate::domain::authz;
use crate::emitter::enforcer_error_to_emitter_error;
use crate::error::UsageEmitterError;

/// A usage emitter scoped to a specific module.
///
/// Constructed via [`crate::UsageEmitterV1::for_module`]. Call [`Self::authorize_for`] or
/// [`Self::authorize`] to obtain a time-limited [`AuthorizedUsageEmitter`] after PDP authorization
/// and allowed-metrics retrieval for this module.
///
/// # Example
///
/// ```ignore
/// // In init():
/// let scoped = emitter.for_module(Self::MODULE_NAME);
///
/// // In a handler:
/// let authorized = scoped
///     .authorize_for(&ctx, tenant_id, resource_id, resource_type)
///     .await?;
/// authorized.build_usage_record("requests", 1.0).enqueue().await?;
/// ```
#[derive(Clone)]
pub struct ScopedUsageEmitter {
    module: String,
    authz: Arc<dyn AuthZResolverClient>,
    collector: Arc<dyn UsageCollectorClientV1>,
    db: Db,
    config: Arc<UsageEmitterConfig>,
    outbox: Arc<Outbox>,
}

impl ScopedUsageEmitter {
    pub(crate) fn new(
        module: String,
        authz: Arc<dyn AuthZResolverClient>,
        collector: Arc<dyn UsageCollectorClientV1>,
        db: Db,
        config: Arc<UsageEmitterConfig>,
        outbox: Arc<Outbox>,
    ) -> Self {
        Self {
            module,
            authz,
            collector,
            db,
            config,
            outbox,
        }
    }

    /// Obtain a time-limited [`AuthorizedUsageEmitter`] by calling the PDP for `USAGE_RECORD`/`CREATE`
    /// and fetching the allowed metrics for this module from the collector.
    ///
    /// The returned handle is bound to the module name, tenant, and metered resource; every enqueued
    /// record must match and its metric must be in the allowed list.
    ///
    /// # Errors
    ///
    /// Returns [`UsageEmitterError`] if PDP denies, the module is not configured, or the collector
    /// call fails.
    // @cpt-algo:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1
    pub async fn authorize_for(
        &self,
        ctx: &SecurityContext,
        tenant_id: Uuid,
        resource_id: Uuid,
        resource_type: String,
    ) -> Result<AuthorizedUsageEmitter, UsageEmitterError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-2
        let request = AccessRequest::new()
            .require_constraints(false)
            .barrier_mode(BarrierMode::Ignore)
            .resource_property(pep_properties::OWNER_TENANT_ID, tenant_id)
            .resource_property(authz::properties::RESOURCE_ID, resource_id)
            .resource_property(authz::properties::RESOURCE_TYPE, resource_type.as_str());
        let pdp_result = PolicyEnforcer::new(Arc::clone(&self.authz))
            .access_scope_with(
                ctx,
                &authz::USAGE_RECORD,
                authz::actions::CREATE,
                None,
                &request,
            )
            .await;
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-2

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-3
        if let Err(e) = pdp_result {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-3a
            return Err(enforcer_error_to_emitter_error(e));
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-3a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-3

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-4
        // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-1
        let module_cfg_result = self.collector.get_module_config(&self.module).await;
        // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-1
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-4

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-5
        let module_cfg = match module_cfg_result {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-5a
            Err(UsageCollectorError::ModuleNotFound { module_name }) => {
                return Err(UsageEmitterError::module_not_configured(module_name));
            }
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-5a
            Err(e) => return Err(UsageEmitterError::authorization_failed(e.to_string())),
            Ok(cfg) => cfg,
        };
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-5

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-6
        let authorized = AuthorizedUsageEmitter::new(
            Arc::clone(&self.config),
            self.db.clone(),
            Arc::clone(&self.outbox),
            self.module.clone(),
            tenant_id,
            resource_id,
            resource_type,
            module_cfg.allowed_metrics,
            ctx.subject_id(),
            ctx.subject_type().unwrap_or("").to_owned(),
        );
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-6

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-7
        Ok(authorized)
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for:p1:inst-authz-7
    }

    /// Same as [`Self::authorize_for`] using the subject's home tenant from `ctx`.
    ///
    /// # Errors
    ///
    /// Returns [`UsageEmitterError`] if PDP denies, the module is not configured, or the collector
    /// call fails.
    pub async fn authorize(
        &self,
        ctx: &SecurityContext,
        resource_id: Uuid,
        resource_type: String,
    ) -> Result<AuthorizedUsageEmitter, UsageEmitterError> {
        self.authorize_for(ctx, ctx.subject_tenant_id(), resource_id, resource_type)
            .await
    }
}
