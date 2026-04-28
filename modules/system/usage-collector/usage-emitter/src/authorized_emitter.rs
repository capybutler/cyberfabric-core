use std::sync::Arc;
use std::time::Instant;

use modkit_db::Db;
use modkit_db::outbox::Outbox;
use modkit_db::secure::DBRunner;
use tracing::debug;
use usage_collector_sdk::models::{AllowedMetric, UsageKind, UsageRecord};
use uuid::Uuid;

use crate::config::UsageEmitterConfig;
use crate::error::UsageEmitterError;
use crate::usage_builder::UsageRecordBuilder;

/// Emitter state after successful PDP authorization; call [`Self::enqueue`] or
/// [`Self::enqueue_in`] on the returned handle.
///
/// Constructed via [`crate::ScopedUsageEmitter::authorize_for`]; callers cannot forge handles.
pub struct AuthorizedUsageEmitter {
    config: Arc<UsageEmitterConfig>,
    pub(crate) db: Db,
    outbox: Arc<Outbox>,
    pub(crate) module: String,
    pub(crate) tenant_id: Uuid,
    pub(crate) resource_id: Uuid,
    pub(crate) resource_type: String,
    pub(crate) allowed_metrics: Vec<AllowedMetric>,
    pub(crate) subject_id: Option<Uuid>,
    pub(crate) subject_type: Option<String>,
    issued_at: Instant,
}

impl AuthorizedUsageEmitter {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        config: Arc<UsageEmitterConfig>,
        db: Db,
        outbox: Arc<Outbox>,
        module: String,
        tenant_id: Uuid,
        resource_id: Uuid,
        resource_type: String,
        allowed_metrics: Vec<AllowedMetric>,
        subject_id: Option<Uuid>,
        subject_type: Option<String>,
    ) -> Self {
        Self {
            config,
            db,
            outbox,
            module,
            tenant_id,
            resource_id,
            resource_type,
            allowed_metrics,
            subject_id,
            subject_type,
            issued_at: Instant::now(),
        }
    }

    /// Enqueue a usage record using this emitter's database connection.
    ///
    /// # Errors
    ///
    /// Returns [`UsageEmitterError`] when obtaining a connection fails, the authorization handle
    /// expired, or the outbox enqueue fails.
    pub async fn enqueue(&self, record: UsageRecord) -> Result<(), UsageEmitterError> {
        let conn = self
            .db
            .conn()
            .map_err(|e| UsageEmitterError::internal(e.to_string()))?;
        self.enqueue_in(&conn, record).await
    }

    /// Enqueue a usage record on the given database runner (connection or transaction).
    ///
    /// # Errors
    ///
    /// Returns [`UsageEmitterError`] when the authorization handle expired, the record does not
    /// match the authorized tenant/resource, the metric is not allowed for this module, a counter
    /// record has a negative value, or the outbox enqueue fails.
    pub async fn enqueue_in(
        &self,
        db: &(dyn DBRunner + Sync),
        record: UsageRecord,
    ) -> Result<(), UsageEmitterError> {
        self.validate_authorization_freshness()?;
        self.validate_authorized_tenant(&record)?;
        self.validate_authorized_resource(&record)?;
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-6
        self.validate_authorized_module(&record)?;
        self.validate_authorized_subject(&record)?;
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-6
        self.validate_allowed_metric(&record)?;
        self.validate_metric_kind(&record)?;
        Self::validate_counter_value(&record)?;
        Self::validate_counter_idempotency_key(&record)?;
        Self::validate_metadata_size(&record)?;

        // Derive partition from the first UUID byte for even load distribution across tenants.
        let bytes = record.tenant_id.as_bytes();
        let partition = u32::from(bytes[0]) % u32::from(self.config.outbox_partition_count);

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-8
        let payload = serde_json::to_vec(&record).map_err(|e| {
            UsageEmitterError::internal(format!("payload serialization failed: {e}"))
        })?;
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-8

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-9
        self.outbox
            .enqueue(
                db,
                &self.config.outbox_queue,
                partition,
                payload,
                "usage-collector.record.v1",
            )
            .await?;
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-9

        debug!(%self.tenant_id, "usage record enqueued");

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-10
        Ok(())
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-10
    }

    fn validate_authorization_freshness(&self) -> Result<(), UsageEmitterError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-1
        let elapsed = self.issued_at.elapsed();
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-1

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-2
        if elapsed > self.config.authorization_max_age {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-2a
            return Err(UsageEmitterError::authorization_expired());
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-2a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-2
        Ok(())
    }

    fn validate_authorized_tenant(&self, record: &UsageRecord) -> Result<(), UsageEmitterError> {
        if record.tenant_id != self.tenant_id {
            return Err(UsageEmitterError::authorization_failed(
                "usage record tenant_id does not match authorized tenant",
            ));
        }
        Ok(())
    }

    fn validate_authorized_resource(&self, record: &UsageRecord) -> Result<(), UsageEmitterError> {
        if record.resource_id != self.resource_id {
            return Err(UsageEmitterError::authorization_failed(
                "usage record resource_id does not match authorized resource",
            ));
        }

        if record.resource_type != self.resource_type {
            return Err(UsageEmitterError::authorization_failed(
                "usage record resource_type does not match authorized resource",
            ));
        }

        Ok(())
    }

    fn validate_authorized_module(&self, record: &UsageRecord) -> Result<(), UsageEmitterError> {
        if record.module != self.module {
            return Err(UsageEmitterError::invalid_record(
                "record module does not match authorized token",
            ));
        }
        Ok(())
    }

    fn validate_authorized_subject(&self, record: &UsageRecord) -> Result<(), UsageEmitterError> {
        if record.subject_id != self.subject_id
            || record.subject_type.as_deref() != self.subject_type.as_deref()
        {
            return Err(UsageEmitterError::invalid_record(
                "record subject does not match authorized token",
            ));
        }
        Ok(())
    }

    fn validate_allowed_metric(&self, record: &UsageRecord) -> Result<(), UsageEmitterError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-3
        let metric_allowed = self.allowed_metrics.iter().any(|m| m.name == record.metric);
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-3

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-4
        if !metric_allowed {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-4a
            return Err(UsageEmitterError::metric_not_allowed(&record.metric));
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-4a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-4
        Ok(())
    }

    fn validate_metric_kind(&self, record: &UsageRecord) -> Result<(), UsageEmitterError> {
        if let Some(allowed) = self
            .allowed_metrics
            .iter()
            .find(|m| m.name == record.metric)
            && allowed.kind != record.kind
        {
            return Err(UsageEmitterError::metric_kind_mismatch(
                &record.metric,
                allowed.kind,
                record.kind,
            ));
        }
        Ok(())
    }

    fn validate_counter_value(record: &UsageRecord) -> Result<(), UsageEmitterError> {
        if record.kind == UsageKind::Counter && record.value < 0.0 {
            return Err(UsageEmitterError::negative_counter_value(record.value));
        }
        Ok(())
    }

    fn validate_counter_idempotency_key(record: &UsageRecord) -> Result<(), UsageEmitterError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-5
        if record.kind == UsageKind::Counter && record.idempotency_key.trim().is_empty() {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-5a
            return Err(UsageEmitterError::invalid_record(
                "counter records require a non-empty idempotency key",
            ));
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-5a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-5
        Ok(())
    }

    fn validate_metadata_size(record: &UsageRecord) -> Result<(), UsageEmitterError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-7
        if let Some(ref metadata) = record.metadata {
            let len = serde_json::to_vec(metadata)
                .map_err(|e| {
                    UsageEmitterError::internal(format!("metadata serialization failed: {e}"))
                })?
                .len();
            if len > 8192 {
                // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-7a
                return Err(UsageEmitterError::metadata_too_large(len));
                // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-7a
            }
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-7
        Ok(())
    }

    /// Starts a [`UsageRecordBuilder`] with `module`, `tenant_id`, `resource_id`, and
    /// `resource_type` from this authorization handle.
    #[must_use]
    pub fn build_usage_record(
        &self,
        metric: impl Into<String>,
        value: f64,
    ) -> UsageRecordBuilder<'_> {
        UsageRecordBuilder::new(self, metric, value)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "authorized_emitter_tests.rs"]
mod authorized_emitter_tests;
