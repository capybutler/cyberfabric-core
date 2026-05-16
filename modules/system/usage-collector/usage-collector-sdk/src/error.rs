use modkit_canonical_errors::resource_error;

pub use modkit_canonical_errors::CanonicalError;

pub type UsageCollectorError = CanonicalError;

/// Resource-scoped error type for usage record operations.
#[resource_error("gts.cf.core.usage.record.v1~")]
pub struct UsageRecordError;

/// Resource-scoped error type for module configuration operations.
#[resource_error("gts.cf.core.usage.module_config.v1~")]
pub struct ModuleConfigError;
