//! Error type for [`crate::ScopedUsageEmitter::authorize_for`],
//! [`crate::AuthorizedUsageEmitter::enqueue`], [`crate::AuthorizedUsageEmitter::enqueue_in`],
//! and [`crate::UsageRecordBuilder::enqueue`].

use modkit_db::outbox::OutboxError as ModkitOutboxError;
use usage_collector_sdk::models::UsageKind;

/// Errors returned from [`crate::ScopedUsageEmitter::authorize_for`],
/// [`crate::AuthorizedUsageEmitter::enqueue`], [`crate::AuthorizedUsageEmitter::enqueue_in`],
/// and [`crate::UsageRecordBuilder::enqueue`].
///
/// All variants are produced *before* any DB write unless otherwise noted.
#[derive(Debug, thiserror::Error)]
pub enum UsageEmitterError {
    /// The authorized emitter handle exceeded [`crate::UsageEmitterConfig::authorization_max_age`]
    /// at [`crate::AuthorizedUsageEmitter::enqueue`] / [`crate::AuthorizedUsageEmitter::enqueue_in`]
    /// evaluation time.
    #[error("emit authorization token has expired")]
    AuthorizationExpired,

    /// PDP explicitly denied the source module.
    #[error("authorization failed: {message}")]
    AuthorizationFailed { message: String },

    /// Required fields were missing when finishing a [`crate::UsageRecordBuilder`] enqueue path.
    #[error("invalid usage record: {message}")]
    InvalidRecord { message: String },

    /// The usage record's kind does not match the registered kind for that metric.
    #[error("metric '{metric}' expects kind {expected:?} but record specifies {actual:?}")]
    MetricKindMismatch {
        metric: String,
        expected: UsageKind,
        actual: UsageKind,
    },

    /// The metric is not in the allowed metrics list for this module.
    #[error("metric not allowed for this module: {metric}")]
    MetricNotAllowed { metric: String },

    /// A counter [`usage_collector_sdk::models::UsageRecord`] was submitted with a negative `value`.
    #[error("counter usage record has a negative value: {value}")]
    NegativeCounterValue { value: f64 },

    /// PDP communication failures and other unexpected conditions not covered by specific variants.
    #[error("internal error: {message}")]
    Internal { message: String },

    /// The module is not registered in the gateway's static metric configuration.
    #[error("module not configured: {module_name}")]
    ModuleNotConfigured { module_name: String },

    /// Metadata JSON exceeded the 8192-byte limit.
    #[error("metadata byte length {len} exceeds the 8192-byte limit")]
    MetadataTooLarge { len: usize },

    /// Transactional outbox error (DB write failure, queue not registered, etc.).
    #[error(transparent)]
    Outbox(#[from] ModkitOutboxError),
}

impl UsageEmitterError {
    #[must_use]
    pub fn authorization_expired() -> Self {
        Self::AuthorizationExpired
    }

    #[must_use]
    pub fn authorization_failed(message: impl Into<String>) -> Self {
        Self::AuthorizationFailed {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_record(message: impl Into<String>) -> Self {
        Self::InvalidRecord {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn metric_kind_mismatch(
        metric: impl Into<String>,
        expected: UsageKind,
        actual: UsageKind,
    ) -> Self {
        Self::MetricKindMismatch {
            metric: metric.into(),
            expected,
            actual,
        }
    }

    #[must_use]
    pub fn metric_not_allowed(metric: impl Into<String>) -> Self {
        Self::MetricNotAllowed {
            metric: metric.into(),
        }
    }

    #[must_use]
    pub fn negative_counter_value(value: f64) -> Self {
        Self::NegativeCounterValue { value }
    }

    #[must_use]
    pub fn module_not_configured(module_name: impl Into<String>) -> Self {
        Self::ModuleNotConfigured {
            module_name: module_name.into(),
        }
    }

    #[must_use]
    pub fn metadata_too_large(len: usize) -> Self {
        Self::MetadataTooLarge { len }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "error_tests.rs"]
mod error_tests;
