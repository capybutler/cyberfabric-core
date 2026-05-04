//! [`UsageRecordBuilder`] for chaining optional fields and enqueueing through an
//! [`AuthorizedUsageEmitter`](crate::AuthorizedUsageEmitter). Tenant, resource, module,
//! and kind are taken from the authorized handle; kind is resolved from the allowed metrics list.
//!
//! Construct via [`AuthorizedUsageEmitter::build_usage_record`](crate::AuthorizedUsageEmitter::build_usage_record).

use chrono::{DateTime, Utc};
use modkit_db::secure::DBRunner;
use serde_json::Value as JsonValue;
use usage_collector_sdk::models::{UsageKind, UsageRecord};

use crate::authorized_emitter::AuthorizedUsageEmitter;
use crate::error::UsageEmitterError;

/// Fluent builder tied to an [`AuthorizedUsageEmitter`]. Optionally set idempotency key or
/// timestamp, then [`Self::enqueue`] or [`Self::enqueue_in`].
///
/// `kind` is resolved automatically from the allowed metrics list; no `.counter()` or `.gauge()`
/// call is required.
pub struct UsageRecordBuilder<'a> {
    emitter: &'a AuthorizedUsageEmitter,
    metric: String,
    idempotency_key: Option<String>,
    value: f64,
    timestamp: Option<DateTime<Utc>>,
    metadata: Option<JsonValue>,
}

impl<'a> UsageRecordBuilder<'a> {
    /// Starts a builder with the required `metric` and `value` set.
    #[must_use]
    pub fn new(emitter: &'a AuthorizedUsageEmitter, metric: impl Into<String>, value: f64) -> Self {
        Self {
            emitter,
            metric: metric.into(),
            idempotency_key: None,
            value,
            timestamp: None,
            metadata: None,
        }
    }

    /// Sets a caller-provided idempotency key. If omitted, a new UUID string is used on enqueue.
    #[must_use]
    pub fn with_idempotency_key(self, key: impl Into<String>) -> Self {
        Self {
            idempotency_key: Some(key.into()),
            ..self
        }
    }

    /// Sets the observation timestamp. If omitted, the current UTC time is used on enqueue.
    #[must_use]
    pub fn with_timestamp(self, timestamp: DateTime<Utc>) -> Self {
        Self {
            timestamp: Some(timestamp),
            ..self
        }
    }

    /// Sets optional metadata JSON. Must serialize to ≤ 8192 bytes or enqueue will return
    /// [`UsageEmitterError::MetadataTooLarge`].
    #[must_use]
    pub fn with_metadata(self, metadata: JsonValue) -> Self {
        Self {
            metadata: Some(metadata),
            ..self
        }
    }

    /// Builds, then enqueues on this emitter's database connection.
    ///
    /// # Errors
    ///
    /// Returns [`UsageEmitterError::InvalidRecord`] if the metric is not in the allowed list,
    /// or any error from [`AuthorizedUsageEmitter::enqueue_in`].
    pub async fn enqueue(self) -> Result<(), UsageEmitterError> {
        let conn = self
            .emitter
            .db
            .conn()
            .map_err(|e| UsageEmitterError::internal(e.to_string()))?;
        self.enqueue_in(&conn).await
    }

    /// Builds, then enqueues on the given database runner (connection or transaction).
    ///
    /// # Errors
    ///
    /// Returns [`UsageEmitterError::InvalidRecord`] if the metric is not in the allowed list,
    /// or any error from [`AuthorizedUsageEmitter::enqueue_in`].
    // @cpt-algo:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1
    pub async fn enqueue_in(self, db: &(dyn DBRunner + Sync)) -> Result<(), UsageEmitterError> {
        let kind = self
            .emitter
            .allowed_metrics
            .iter()
            .find(|m| m.name == self.metric)
            .map_or(UsageKind::Gauge, |m| m.kind);

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-6
        let subject_id = self.emitter.subject_id;
        let subject_type = self.emitter.subject_type.clone();
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-6

        let record = UsageRecord {
            tenant_id: self.emitter.tenant_id,
            module: self.emitter.module.clone(),
            resource_id: self.emitter.resource_id,
            resource_type: self.emitter.resource_type.clone(),
            subject_id,
            subject_type,
            metric: self.metric,
            kind,
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-5b
            idempotency_key: if kind == UsageKind::Gauge {
                String::new()
            } else {
                self.idempotency_key.unwrap_or_default()
            },
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue:p1:inst-enq-5b
            value: self.value,
            timestamp: self.timestamp.unwrap_or_else(Utc::now),
            metadata: self.metadata,
        };

        self.emitter.enqueue_in(db, record).await
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "usage_builder_tests.rs"]
mod usage_builder_tests;
