//! PDP resource type and action constants for usage-record authorization.

use authz_resolver_sdk::pep::ResourceType;
use modkit_security::pep_properties;

/// Property names for usage record ABAC (beyond shared `pep_properties`).
pub mod properties {
    /// Metered resource instance identifier (usage record resource property).
    pub const RESOURCE_ID: &str = "resource_id";
    /// Metered resource type (usage record resource property).
    pub const RESOURCE_TYPE: &str = "resource_type";
    /// Source module identity (usage record resource property).
    pub const MODULE: &str = "module";
    /// Subject UUID (usage record resource property).
    pub const SUBJECT_ID: &str = "subject_id";
    /// Subject kind (usage record resource property).
    pub const SUBJECT_TYPE: &str = "subject_type";
}

/// Usage record resource type used by [`crate::UsageEmitter::authorize_for`].
///
/// Resource properties include `owner_tenant_id`, `resource_id`, `resource_type`,
/// `module`, `subject_id`, and `subject_type`.
pub const USAGE_RECORD: ResourceType = ResourceType {
    name: "gts.cf.core.usage.record.v1~",
    supported_properties: &[
        pep_properties::OWNER_TENANT_ID,
        properties::RESOURCE_ID,
        properties::RESOURCE_TYPE,
        properties::MODULE,
        properties::SUBJECT_ID,
        properties::SUBJECT_TYPE,
    ],
};

/// Authorization action name used when calling the PDP.
pub mod actions {
    pub const CREATE: &str = "create";
}
