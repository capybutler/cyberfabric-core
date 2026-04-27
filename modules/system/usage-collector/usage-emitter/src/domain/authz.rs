//! PDP resource type and action constants for usage-record authorization.
//!
//! These constants are implementation details of [`crate::UsageEmitter::authorize_for`].
//! They are intentionally `pub(crate)` — no external crate can construct a
//! `PolicyEnforcer` call against `gts.x.core.usage.record.v1 / create` using
//! names exported from this crate.

use authz_resolver_sdk::pep::ResourceType;
use modkit_security::pep_properties;

/// Property names for usage record ABAC (beyond shared `pep_properties`).
pub mod properties {
    /// Metered resource instance identifier (usage record resource property).
    pub const RESOURCE_ID: &str = "resource_id";
    /// Metered resource type (usage record resource property).
    pub const RESOURCE_TYPE: &str = "resource_type";
}

/// Usage record resource type used by [`crate::UsageEmitter::authorize_for`].
///
/// Resource properties include `owner_tenant_id`, `resource_id`, and `resource_type`.
pub const USAGE_RECORD: ResourceType = ResourceType {
    name: "gts.x.core.usage.record.v1~",
    supported_properties: &[
        pep_properties::OWNER_TENANT_ID,
        properties::RESOURCE_ID,
        properties::RESOURCE_TYPE,
    ],
};

/// Authorization action name used when calling the PDP.
pub mod actions {
    pub const CREATE: &str = "create";
}
