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

    /// Transient infrastructure failure: connection/transport error or a dependent service
    /// (e.g. the identity/AuthN service) is temporarily unreachable.
    /// The operation may succeed on retry once the outage resolves.
    #[error("service unavailable: {message}")]
    Unavailable {
        /// Detail for operators and logs.
        message: String,
    },

    /// The aggregation query would return more rows than the configured `MAX_AGG_ROWS` limit.
    /// The query must be narrowed (smaller time range, additional filters, or finer grouping).
    #[error("query result too large: got {result_count} rows, limit is {limit}")]
    QueryResultTooLarge {
        /// Number of rows the query would produce.
        result_count: usize,
        /// Configured row limit.
        limit: usize,
    },
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

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn query_result_too_large(result_count: usize, limit: usize) -> Self {
        Self::QueryResultTooLarge {
            result_count,
            limit,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "error_tests.rs"]
mod error_tests;
