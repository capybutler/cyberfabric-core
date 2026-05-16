//! Domain errors for the usage-collector gateway.

use modkit_macros::domain_model;
use usage_collector_sdk::{UsageCollectorError, UsageRecordError, ModuleConfigError};

/// Internal domain errors for the usage-collector gateway service.
///
/// Mapped to [`UsageCollectorError`] at the [`super::UsageCollectorLocalClient`]
/// boundary and to HTTP problems at the REST handler boundary.
#[domain_model]
#[derive(thiserror::Error, Debug)]
pub enum DomainError {
    #[error("types registry is not available: {0}")]
    TypesRegistryUnavailable(String),

    #[error("no plugin instances found for vendor '{vendor}'")]
    PluginNotFound { vendor: String },

    #[error("invalid plugin instance content for '{gts_id}': {reason}")]
    InvalidPluginInstance { gts_id: String, reason: String },

    #[error("plugin not available for '{gts_id}': {reason}")]
    PluginUnavailable { gts_id: String, reason: String },

    #[error("plugin call timed out")]
    Timeout,

    #[error("circuit breaker open")]
    CircuitOpen,

    #[error("module '{module}' not configured")]
    ModuleNotConfigured { module: String },

    #[error("plugin error: {0}")]
    Plugin(#[source] UsageCollectorError),

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<types_registry_sdk::TypesRegistryError> for DomainError {
    fn from(e: types_registry_sdk::TypesRegistryError) -> Self {
        Self::TypesRegistryUnavailable(e.to_string())
    }
}

impl From<modkit::client_hub::ClientHubError> for DomainError {
    fn from(e: modkit::client_hub::ClientHubError) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<modkit::plugins::ChoosePluginError> for DomainError {
    fn from(e: modkit::plugins::ChoosePluginError) -> Self {
        match e {
            modkit::plugins::ChoosePluginError::InvalidPluginInstance { gts_id, reason } => {
                Self::InvalidPluginInstance { gts_id, reason }
            }
            modkit::plugins::ChoosePluginError::PluginNotFound { vendor, .. } => {
                Self::PluginNotFound { vendor }
            }
        }
    }
}

impl From<DomainError> for UsageCollectorError {
    fn from(e: DomainError) -> Self {
        match e {
            DomainError::Plugin(canonical) => canonical,
            DomainError::ModuleNotConfigured { module } => {
                ModuleConfigError::not_found("module not configured")
                    .with_resource(&module)
                    .create()
            }
            DomainError::Timeout => {
                UsageRecordError::deadline_exceeded("plugin call timed out").create()
            }
            DomainError::CircuitOpen => UsageCollectorError::service_unavailable()
                .with_detail("circuit breaker open")
                .create(),
            DomainError::PluginNotFound { vendor } => UsageCollectorError::service_unavailable()
                .with_detail(format!("no plugin instances found for vendor '{vendor}'"))
                .create(),
            DomainError::PluginUnavailable { gts_id, reason } => {
                UsageCollectorError::service_unavailable()
                    .with_detail(format!("plugin not available for '{gts_id}': {reason}"))
                    .create()
            }
            DomainError::InvalidPluginInstance { gts_id, reason } => {
                UsageCollectorError::internal(format!(
                    "invalid plugin instance '{gts_id}': {reason}"
                ))
                .create()
            }
            DomainError::TypesRegistryUnavailable(reason) | DomainError::Internal(reason) => {
                UsageCollectorError::internal(reason).create()
            }
        }
    }
}
