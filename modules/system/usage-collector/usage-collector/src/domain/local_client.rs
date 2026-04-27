//! Local client registered in `ClientHub` as `dyn UsageCollectorClientV1`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use modkit::client_hub::{ClientHub, ClientScope};
use modkit::plugins::{GtsPluginSelector, choose_plugin_instance};
use modkit::telemetry::ThrottledLog;
use modkit_macros::domain_model;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, info, warn};
use types_registry_sdk::{ListQuery, TypesRegistryClient};
use usage_collector_sdk::AllowedMetric;
use usage_collector_sdk::ModuleConfig;
use usage_collector_sdk::UsageCollectorClientV1;
use usage_collector_sdk::UsageCollectorError;
use usage_collector_sdk::UsageRecord;
use usage_collector_sdk::{UsageCollectorPluginClientV1, UsageCollectorStoragePluginSpecV1};

use crate::config::UsageCollectorConfig;

const UNAVAILABLE_LOG_THROTTLE: Duration = Duration::from_secs(10);

/// Circuit breaker state.
#[domain_model]
#[derive(Debug)]
enum CircuitState {
    /// Normal operation; plugin calls are forwarded.
    Closed,
    /// Circuit is open; plugin calls are rejected without attempting the plugin.
    Open {
        /// When the circuit was opened (used to compute half-open probe eligibility).
        opened_at: Instant,
    },
    /// A single probe call is in-flight; treat any failure as re-open.
    HalfOpen,
}

/// Minimal sliding-window circuit breaker.
///
/// Opens after `failure_threshold` consecutive failures within `window`.
/// Transitions to half-open after `recovery_timeout`.
#[domain_model]
#[derive(Debug)]
struct CircuitBreaker {
    /// Failure threshold before the circuit opens.
    failure_threshold: u32,
    /// Rolling window for failure counting.
    window: Duration,
    /// How long to wait in the open state before probing.
    recovery_timeout: Duration,
    /// Current circuit state.
    state: CircuitState,
    /// Timestamps of recent failures within the current window.
    failure_timestamps: Vec<Instant>,
}

impl CircuitBreaker {
    fn new(failure_threshold: u32, window: Duration, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            window,
            recovery_timeout,
            state: CircuitState::Closed,
            failure_timestamps: Vec::new(),
        }
    }

    /// Returns `true` if the circuit is currently open (caller should return `CircuitOpen`).
    fn is_open(&mut self) -> bool {
        match &self.state {
            CircuitState::Open { opened_at } => {
                if opened_at.elapsed() >= self.recovery_timeout {
                    // Transition to half-open to allow a probe.
                    info!("Circuit breaker transitioning from Open to HalfOpen for probe");
                    self.state = CircuitState::HalfOpen;
                    false
                } else {
                    true
                }
            }
            CircuitState::Closed | CircuitState::HalfOpen => false,
        }
    }

    /// Record a failure. Opens the circuit if threshold exceeded within the window.
    fn record_failure(&mut self) {
        let now = Instant::now();

        // Prune failures outside the rolling window.
        self.failure_timestamps
            .retain(|t| now.duration_since(*t) < self.window);

        self.failure_timestamps.push(now);

        // Failure threshold is at most u32::MAX; the window prunes old entries so this
        // count is bounded by `failure_threshold` which is u32.
        #[allow(clippy::cast_possible_truncation)]
        let consecutive = self.failure_timestamps.len() as u32;

        if consecutive >= self.failure_threshold {
            match self.state {
                CircuitState::Closed | CircuitState::HalfOpen => {
                    warn!(
                        consecutive_failures = consecutive,
                        threshold = self.failure_threshold,
                        "Circuit breaker opening after consecutive failures"
                    );
                    self.state = CircuitState::Open { opened_at: now };
                    self.failure_timestamps.clear();
                }
                CircuitState::Open { .. } => {
                    // Already open; update timestamp.
                    self.state = CircuitState::Open { opened_at: now };
                }
            }
        }
    }

    /// Record a successful call — reset failure window and close circuit.
    fn record_success(&mut self) {
        match self.state {
            CircuitState::HalfOpen => {
                info!(
                    "Circuit breaker transitioning from HalfOpen to Closed after successful probe"
                );
                self.state = CircuitState::Closed;
                self.failure_timestamps.clear();
            }
            CircuitState::Closed => {
                self.failure_timestamps.clear();
            }
            CircuitState::Open { .. } => {
                // Should not happen (success while open), but reset to closed.
                warn!("Circuit breaker received success while Open; resetting to Closed");
                self.state = CircuitState::Closed;
                self.failure_timestamps.clear();
            }
        }
    }
}

/// Local `ClientHub` implementation of [`UsageCollectorClientV1`].
///
/// Resolves the configured GTS storage plugin on first use and delegates
/// record creation to it, while also serving per-module metric config from
/// the static configuration.
#[domain_model]
pub struct UsageCollectorLocalClient {
    hub: Arc<ClientHub>,
    selector: GtsPluginSelector,
    config: UsageCollectorConfig,
    unavailable_log_throttle: ThrottledLog,
    circuit_breaker: Mutex<CircuitBreaker>,
}

impl UsageCollectorLocalClient {
    #[must_use]
    pub fn new(config: UsageCollectorConfig, hub: Arc<ClientHub>) -> Self {
        let cb = CircuitBreaker::new(
            config.circuit_breaker_failure_threshold,
            config.circuit_breaker_window,
            config.circuit_breaker_recovery_timeout,
        );
        Self {
            hub,
            selector: GtsPluginSelector::new(),
            config,
            unavailable_log_throttle: ThrottledLog::new(UNAVAILABLE_LOG_THROTTLE),
            circuit_breaker: Mutex::new(cb),
        }
    }

    async fn get_plugin(
        &self,
    ) -> Result<Arc<dyn UsageCollectorPluginClientV1>, UsageCollectorError> {
        let instance_id = self.selector.get_or_init(|| self.resolve_plugin()).await?;
        let scope = ClientScope::gts_id(instance_id.as_ref());

        if let Some(client) = self
            .hub
            .try_get_scoped::<dyn UsageCollectorPluginClientV1>(&scope)
        {
            Ok(client)
        } else {
            if self.unavailable_log_throttle.should_log() {
                warn!(
                    plugin_gts_id = %instance_id,
                    self.config.vendor,
                    "Plugin client not registered yet"
                );
            }
            Err(UsageCollectorError::internal(format!(
                "plugin client not registered for {instance_id}"
            )))
        }
    }

    #[tracing::instrument(skip_all, fields(vendor = %self.config.vendor))]
    async fn resolve_plugin(&self) -> Result<String, UsageCollectorError> {
        info!("Resolving usage-collector storage plugin");

        let registry = self
            .hub
            .get::<dyn TypesRegistryClient>()
            .map_err(|e| UsageCollectorError::internal(e.to_string()))?;

        let plugin_type_id = UsageCollectorStoragePluginSpecV1::gts_schema_id().clone();

        let instances = registry
            .list(
                ListQuery::new()
                    .with_pattern(format!("{plugin_type_id}*"))
                    .with_is_type(false),
            )
            .await
            .map_err(|e| UsageCollectorError::internal(e.to_string()))?;

        let gts_id = choose_plugin_instance::<UsageCollectorStoragePluginSpecV1>(
            &self.config.vendor,
            instances.iter().map(|e| (e.gts_id.as_str(), &e.content)),
        )
        .map_err(|e| UsageCollectorError::internal(e.to_string()))?;

        info!(plugin_gts_id = %gts_id, "Selected usage-collector storage plugin instance");
        Ok(gts_id)
    }
}

// @cpt-algo:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:p1
// @cpt-dod:cpt-cf-usage-collector-dod-sdk-and-ingest-core-gateway-crate:p1
#[async_trait]
impl UsageCollectorClientV1 for UsageCollectorLocalClient {
    /// # Errors
    ///
    /// Returns `UsageCollectorError::CircuitOpen` if the circuit breaker is open.
    /// Returns `UsageCollectorError::Internal` if no storage plugin is available or
    /// if the plugin call fails. Returns `UsageCollectorError::PluginTimeout` if the
    /// plugin does not respond within the configured deadline.
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-2
        let circuit_open = self.circuit_breaker.lock().await.is_open();
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-2

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-3
        if circuit_open {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-3a
            warn!("Circuit breaker is open; rejecting usage record without calling plugin");
            return Err(UsageCollectorError::circuit_open());
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-3a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-3

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-4
        let plugin = self.get_plugin().await?;
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-4

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-5
        let call = timeout(
            self.config.plugin_timeout,
            plugin.create_usage_record(record),
        );
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-5

        match call.await {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-6
            Err(_elapsed) => {
                // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-6a
                self.circuit_breaker.lock().await.record_failure();
                // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-6a
                // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-6b
                Err(UsageCollectorError::plugin_timeout())
                // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-6b
            }
            Ok(Err(e)) => {
                warn!(error = %e, "storage plugin call failed transiently; recording circuit breaker failure");
                self.circuit_breaker.lock().await.record_failure();
                Err(e)
            }
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-6
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-7
            Ok(Ok(())) => {
                debug!("usage record created successfully");
                self.circuit_breaker.lock().await.record_success();
                Ok(())
            } // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-7
        }
    }

    // @cpt-flow:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2
    // @cpt-algo:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2
    async fn get_module_config(
        &self,
        module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-1
        // Authentication is enforced by the ModKit pipeline (`.authenticated()` in routes.rs);
        // unauthenticated requests are rejected before this function is reached.
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-1

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-2
        let allowed_metrics: Vec<AllowedMetric> = self
            .config
            .metrics
            .iter()
            .filter(|(_, cfg)| {
                cfg.modules
                    .as_ref()
                    .is_none_or(|mods| mods.iter().any(|m| m == module_name))
            })
            .map(|(name, cfg)| AllowedMetric {
                name: name.clone(),
                kind: cfg.kind,
            })
            .collect();
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-2

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-3
        if allowed_metrics.is_empty() {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-3a
            return Err(UsageCollectorError::module_not_found(module_name));
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-3a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-3

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-4
        Ok(ModuleConfig { allowed_metrics })
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config:p2:inst-cfg-p-4
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "local_client_tests.rs"]
mod local_client_tests;
