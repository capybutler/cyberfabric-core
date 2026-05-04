use std::time::Duration;

use serde::Deserialize;

/// Configuration for [`crate::UsageEmitter`].
///
/// Host modules embed this inside their own config struct and forward it to
/// [`crate::UsageEmitter::build`]. All fields have sensible defaults so
/// `#[serde(default)]` on the embedding struct is sufficient for zero-config usage.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UsageEmitterConfig {
    /// Maximum age of an [`crate::AuthorizedUsageEmitter`] handle after
    /// [`crate::UsageEmitterV1::authorize`] before
    /// [`crate::AuthorizedUsageEmitter::enqueue`] / [`crate::AuthorizedUsageEmitter::enqueue_in`]
    /// reject it with [`crate::UsageEmitterError::AuthorizationExpired`].
    pub authorization_max_age: Duration,

    /// Outbox queue name for usage records delivered to the collector.
    pub outbox_queue: String,

    /// Number of outbox partitions. Must be a power of 2 in 1-64.
    pub outbox_partition_count: u16,

    /// Maximum exponential-backoff delay for outbox delivery retries.
    ///
    /// Maps to [`modkit_db::outbox::WorkerTuning::retry_max`].
    /// MUST remain below 15 minutes to satisfy `cpt-cf-usage-collector-nfr-recovery`
    /// (inst-dlv-6a / inst-emit-10a).
    pub outbox_backoff_max: Duration,
}

impl Default for UsageEmitterConfig {
    fn default() -> Self {
        Self {
            authorization_max_age: Duration::from_secs(30),
            outbox_queue: "usage-records".to_owned(),
            outbox_partition_count: 4,
            outbox_backoff_max: Duration::from_mins(10), // 10 minutes — well below the 15-minute NFR ceiling
        }
    }
}

impl UsageEmitterConfig {
    /// # Errors
    ///
    /// Returns an error when any configuration field is invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.outbox_queue.trim().is_empty(),
            "outbox_queue must not be empty"
        );
        anyhow::ensure!(
            (1..=64).contains(&self.outbox_partition_count)
                && self.outbox_partition_count.is_power_of_two(),
            "outbox_partition_count must be a power of 2 in 1-64, got {}",
            self.outbox_partition_count
        );
        anyhow::ensure!(
            !self.authorization_max_age.is_zero(),
            "authorization_max_age must be > 0"
        );
        anyhow::ensure!(
            self.outbox_backoff_max > Duration::ZERO,
            "outbox_backoff_max must be greater than zero"
        );
        anyhow::ensure!(
            self.outbox_backoff_max < Duration::from_mins(15),
            "outbox_backoff_max must be below 15 minutes (cpt-cf-usage-collector-nfr-recovery), got {:?}",
            self.outbox_backoff_max
        );

        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "config_tests.rs"]
mod config_tests;
