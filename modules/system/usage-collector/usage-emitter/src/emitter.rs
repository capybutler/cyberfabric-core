use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use authz_resolver_sdk::EnforcerError;
use modkit_db::Db;
use modkit_db::outbox::WorkerTuning;
use modkit_db::outbox::{Outbox, OutboxHandle, Partitions};
use usage_collector_sdk::UsageCollectorClientV1;

use crate::api::UsageEmitterV1;
use crate::config::UsageEmitterConfig;
use crate::infra::delivery_handler::DeliveryHandler;
use crate::scoped_emitter::ScopedUsageEmitter;

/// An emitter that starts the usage outbox worker and issues scoped emit handles.
///
/// Constructed via [`UsageEmitter::build`]. Call [`UsageEmitterV1::for_module`] to obtain a
/// [`ScopedUsageEmitter`] bound to a module name, then use its `authorize_for` / `authorize` to
/// get a time-limited [`crate::AuthorizedUsageEmitter`].
pub struct UsageEmitter {
    config: Arc<UsageEmitterConfig>,
    db: Db,
    authz: Arc<dyn AuthZResolverClient>,
    collector: Arc<dyn UsageCollectorClientV1>,
    outbox_handle: OutboxHandle,
}

impl UsageEmitter {
    /// Build a [`UsageEmitter`] and start the background outbox worker.
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

impl UsageEmitterV1 for UsageEmitter {
    fn for_module(&self, module_name: &str) -> ScopedUsageEmitter {
        ScopedUsageEmitter::new(
            module_name.to_owned(),
            Arc::clone(&self.authz),
            Arc::clone(&self.collector),
            self.db.clone(),
            Arc::clone(&self.config),
            Arc::clone(self.outbox_handle.outbox()),
        )
    }
}

pub fn enforcer_error_to_emitter_error(e: EnforcerError) -> crate::error::UsageEmitterError {
    use crate::error::UsageEmitterError;
    use authz_resolver_sdk::EnforcerError;
    match e {
        EnforcerError::Denied { deny_reason } => {
            let message = deny_reason.map_or_else(
                || "access denied by policy".to_owned(),
                |r| match r.details {
                    Some(details) => format!("{}: {}", r.error_code, details),
                    None => r.error_code,
                },
            );
            UsageEmitterError::authorization_failed(message)
        }
        EnforcerError::CompileFailed(_) => {
            UsageEmitterError::internal("authorization constraint compilation failed")
        }
        EnforcerError::EvaluationFailed(_) => {
            UsageEmitterError::internal("authorization evaluation failed")
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "emitter_tests.rs"]
mod emitter_tests;
