use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use modkit_db::Db;
use modkit_db::outbox::WorkerTuning;
use modkit_db::outbox::{Outbox, OutboxHandle, Partitions};
use usage_collector_sdk::UsageCollectorClientV1;

use crate::api::UsageEmitterFactoryV1;
use crate::config::UsageEmitterConfig;
use crate::domain::emitter::UsageEmitter;
use crate::infra::delivery_handler::DeliveryHandler;

/// Builds the per-process [`UsageEmitter`] pipeline and owns the outbox worker.
///
/// Constructed via [`UsageEmitterFactory::build`] and registered in `ClientHub` as
/// `dyn UsageEmitterFactoryV1`. Each call to [`UsageEmitterFactoryV1::for_module`] hands out a
/// module-bound [`UsageEmitter`] handle that source modules use to authorize and
/// enqueue usage records.
pub struct UsageEmitterFactory {
    config: Arc<UsageEmitterConfig>,
    db: Db,
    authz: Arc<dyn AuthZResolverClient>,
    collector: Arc<dyn UsageCollectorClientV1>,
    outbox_handle: OutboxHandle,
}

impl UsageEmitterFactory {
    /// Build a [`UsageEmitterFactory`] and start the background outbox worker.
    ///
    /// Registers the `usage-records` queue, attaches `delivery_handler` for async delivery to
    /// `collector`, and wires the [`authz_resolver_sdk::pep::PolicyEnforcer`] for per-call PDP checks.
    ///
    /// # Errors
    ///
    /// Returns an error if the outbox worker fails to start (e.g. DB unavailable or
    /// queue registration fails).
    pub async fn build(
        config: UsageEmitterConfig,
        db: Db,
        authz: Arc<dyn AuthZResolverClient>,
        collector: Arc<dyn UsageCollectorClientV1>,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let delivery_handler = DeliveryHandler::new(Arc::clone(&collector));

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-6a
        let processor_tuning =
            WorkerTuning::processor_default().retry_max(config.outbox_backoff_max);
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-6a

        let outbox_handle = Outbox::builder(db.clone())
            .processor_tuning(processor_tuning)
            .queue(
                &config.outbox_queue,
                Partitions::of(config.outbox_partition_count),
            )
            .leased(delivery_handler)
            .start()
            .await
            .map_err(anyhow::Error::from)?;

        Ok(Self {
            config: Arc::new(config),
            db,
            authz,
            collector,
            outbox_handle,
        })
    }
}

impl UsageEmitterFactoryV1 for UsageEmitterFactory {
    fn for_module(&self, module_name: &str) -> UsageEmitter {
        UsageEmitter::new(
            module_name.to_owned(),
            Arc::clone(&self.authz),
            Arc::clone(&self.collector),
            self.db.clone(),
            Arc::clone(&self.config),
            Arc::clone(self.outbox_handle.outbox()),
        )
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "factory_tests.rs"]
mod factory_tests;
