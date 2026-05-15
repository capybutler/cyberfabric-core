//! Sliding-window circuit breaker used by the usage-collector domain service.
//!
//! Wraps fallible async calls to the storage plugin so that repeated
//! infrastructure failures stop hammering an unhealthy backend, while
//! caller-induced errors do not trip the breaker.
//!
//! The single entry point is [`CircuitBreaker::execute`], which acquires a
//! permit, invokes the closure, classifies the result, and updates breaker
//! state atomically.

use std::time::Instant;

use parking_lot::Mutex;
use tracing::{info, warn};
use usage_collector_sdk::UsageCollectorError;

use super::error::DomainError;
use crate::config::CircuitBreakerConfig;

#[derive(Debug)]
enum State {
    Closed,
    Open { opened_at: Instant },
    HalfOpen,
}

#[derive(Debug)]
struct Inner {
    config: CircuitBreakerConfig,
    state: State,
    failure_timestamps: Vec<Instant>,
}

impl Inner {
    /// Atomically check whether the next call may proceed and (if so) update
    /// state for an in-flight probe.
    ///
    /// Returns `Ok(was_probe)` — `true` when the caller is the single
    /// `HalfOpen` probe, `false` for normal `Closed` traffic — or
    /// `Err(DomainError::CircuitOpen)` when the circuit is rejecting calls.
    fn try_acquire(&mut self) -> Result<bool, DomainError> {
        match &self.state {
            State::Closed => Ok(false),
            State::Open { opened_at } => {
                if opened_at.elapsed() >= self.config.recovery_timeout {
                    info!("Circuit breaker transitioning from Open to HalfOpen for probe");
                    self.state = State::HalfOpen;
                    Ok(true)
                } else {
                    Err(DomainError::CircuitOpen)
                }
            }
            // A probe is already in flight; reject everyone else until it completes.
            State::HalfOpen => Err(DomainError::CircuitOpen),
        }
    }

    fn record_failure(&mut self) {
        let now = Instant::now();

        self.failure_timestamps
            .retain(|t| now.duration_since(*t) < self.config.window);
        self.failure_timestamps.push(now);

        let failures_in_window = self.failure_timestamps.len();

        if failures_in_window >= self.config.failure_threshold as usize {
            match self.state {
                State::Closed | State::HalfOpen => {
                    warn!(
                        failures_in_window,
                        threshold = self.config.failure_threshold,
                        "Circuit breaker opening after too many failures within the rolling window"
                    );
                    self.state = State::Open { opened_at: now };
                    self.failure_timestamps.clear();
                }
                // Already open; resetting opened_at would prevent recovery under sustained load.
                State::Open { .. } => {}
            }
        }
    }

    fn record_success(&mut self) {
        match self.state {
            State::HalfOpen => {
                info!(
                    "Circuit breaker transitioning from HalfOpen to Closed after successful probe"
                );
                self.state = State::Closed;
                self.failure_timestamps.clear();
            }
            State::Closed => {
                self.failure_timestamps.clear();
            }
            State::Open { .. } => {
                warn!("Circuit breaker received success while Open; resetting to Closed");
                self.state = State::Closed;
                self.failure_timestamps.clear();
            }
        }
    }
}

/// Sliding-window circuit breaker.
///
/// Opens after `failure_threshold` failures within the rolling `window`,
/// stays open for `recovery_timeout`, then admits one probe call. Any
/// non-success during a probe re-opens the circuit; a successful probe
/// closes it.
pub struct CircuitBreaker {
    inner: Mutex<Inner>,
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            inner: Mutex::new(Inner {
                config,
                state: State::Closed,
                failure_timestamps: Vec::new(),
            }),
        }
    }

    /// Run `f` under the breaker.
    ///
    /// Returns `DomainError::CircuitOpen` without invoking `f` if the circuit
    /// is rejecting calls. Otherwise executes `f` and records the outcome:
    /// infrastructure-shaped errors count as failures, caller-induced errors
    /// are ignored, and during a `HalfOpen` probe any non-success re-opens the
    /// circuit.
    ///
    /// # Errors
    ///
    /// Propagates any `DomainError` returned by `f`, or `DomainError::CircuitOpen`
    /// when the circuit is open.
    pub async fn execute<F, Fut, T>(&self, f: F) -> Result<T, DomainError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, DomainError>>,
    {
        let was_probe = self.inner.lock().try_acquire()?;

        let result = f().await;

        let mut inner = self.inner.lock();
        match &result {
            Ok(_) => inner.record_success(),
            Err(e) if is_health_failure(e) => inner.record_failure(),
            // HalfOpen is strict: any non-success during the probe re-opens.
            Err(_) if was_probe => inner.record_failure(),
            // Caller-induced error in Closed state — leave breaker untouched.
            Err(_) => {}
        }

        result
    }
}

/// Returns `true` when `err` indicates plugin/infrastructure ill-health and
/// must trip the circuit breaker. Caller-induced errors return `false`.
fn is_health_failure(err: &DomainError) -> bool {
    match err {
        DomainError::TypesRegistryUnavailable(_)
        | DomainError::ClientHub(_)
        | DomainError::PluginNotFound { .. }
        | DomainError::PluginUnavailable { .. }
        | DomainError::Timeout
        | DomainError::Internal(_) => true,
        DomainError::Plugin(canonical) => is_canonical_health_failure(canonical),
        // Already-open or invalid-config errors are not new failure signals.
        DomainError::CircuitOpen
        | DomainError::InvalidPluginInstance { .. }
        | DomainError::ModuleNotConfigured { .. } => false,
    }
}

fn is_canonical_health_failure(err: &UsageCollectorError) -> bool {
    matches!(
        err,
        UsageCollectorError::ServiceUnavailable { .. }
            | UsageCollectorError::Internal { .. }
            | UsageCollectorError::Unknown { .. }
            | UsageCollectorError::DataLoss { .. }
            | UsageCollectorError::DeadlineExceeded { .. }
    )
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "circuit_breaker_tests.rs"]
mod circuit_breaker_tests;
