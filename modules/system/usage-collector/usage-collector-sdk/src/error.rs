/// Errors produced by the usage collector gateway, client trait, and storage plugins.
#[derive(Debug, thiserror::Error)]
pub enum UsageCollectorError {
    /// Authorization or policy denial (e.g. emit-time PDP); retained for API compatibility.
    #[error("authorization failed: {message}")]
    AuthorizationFailed {
        /// Human-readable failure description.
        message: String,
    },

    /// Types-registry / plugin resolution / hub wiring failures and other unexpected conditions.
    #[error("internal error: {message}")]
    Internal {
        /// Detail for operators and logs.
        message: String,
    },

    /// No metrics are configured for this module in the gateway's static configuration.
    #[error("module not found in configuration: {module_name}")]
    ModuleNotFound {
        /// Name of the module that has no configured metrics.
        module_name: String,
    },

    /// Plugin call exceeded the configured timeout.
    #[error("storage plugin call timed out")]
    PluginTimeout,

    /// Circuit breaker is open — storage plugin calls are suspended until the recovery window elapses.
    #[error("storage plugin circuit breaker is open")]
    CircuitOpen,
}

impl UsageCollectorError {
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
    pub fn module_not_found(module_name: impl Into<String>) -> Self {
        Self::ModuleNotFound {
            module_name: module_name.into(),
        }
    }

    #[must_use]
    pub fn plugin_timeout() -> Self {
        Self::PluginTimeout
    }

    #[must_use]
    pub fn circuit_open() -> Self {
        Self::CircuitOpen
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "error_tests.rs"]
mod error_tests;
