//! Configuration for the usage-collector gateway module.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use usage_collector_sdk::UsageKind;
use usage_emitter::UsageEmitterConfig;

/// Per-metric allowed configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricConfig {
    /// Gauge vs counter semantics.
    pub kind: UsageKind,
    /// Modules allowed to emit this metric. If absent, all modules are allowed.
    pub modules: Option<Vec<String>>,
}

/// Module configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageCollectorConfig {
    /// Vendor selector used to pick a storage plugin instance from types-registry.
    pub vendor: String,

    /// Timeout for each storage plugin `create_usage_record()` call.
    /// Valid range: 100ms–30s. Default: 5s.
    #[serde(with = "modkit_utils::humantime_serde")]
    pub plugin_timeout: Duration,

    /// Number of consecutive failures within `circuit_breaker_window` that will open the circuit.
    /// Valid range: 1–100. Default: 5.
    pub circuit_breaker_failure_threshold: u32,

    /// Rolling window for counting consecutive failures.
    /// Default: 10s.
    #[serde(with = "modkit_utils::humantime_serde")]
    pub circuit_breaker_window: Duration,

    /// Duration to wait in the open state before allowing a half-open probe.
    /// Valid range: 1s–5m. Default: 30s.
    #[serde(with = "modkit_utils::humantime_serde")]
    pub circuit_breaker_recovery_timeout: Duration,

    /// Outbox/authorization tuning for the embedded usage emitter.
    pub emitter: UsageEmitterConfig,

    /// Allowed metrics configuration. Key is the metric name.
    pub metrics: HashMap<String, MetricConfig>,
}

impl Default for UsageCollectorConfig {
    fn default() -> Self {
        Self {
            vendor: "hyperspot".to_owned(),
            plugin_timeout: Duration::from_secs(5),
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_window: Duration::from_secs(10),
            circuit_breaker_recovery_timeout: Duration::from_secs(30),
            emitter: UsageEmitterConfig::default(),
            metrics: HashMap::new(),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "config_tests.rs"]
mod config_tests;
