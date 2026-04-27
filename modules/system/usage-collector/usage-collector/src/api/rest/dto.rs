//! REST DTOs for the usage-collector gateway.

use chrono::{DateTime, Utc};
use usage_collector_sdk::models::UsageKind;
use uuid::Uuid;

/// Request body to create one usage record (ingest).
#[derive(Debug, Clone)]
#[modkit_macros::api_dto(request)]
pub struct CreateUsageRecordRequest {
    /// Name of the module emitting this record.
    pub module: String,
    /// Tenant that owns the record.
    pub tenant_id: Uuid,
    /// Logical type of the metered resource.
    pub resource_type: String,
    /// Identifier of the metered resource instance.
    pub resource_id: Uuid,
    /// Identifier of the subject (user/service account) performing the request.
    pub subject_id: Uuid,
    /// Type of the subject (e.g. `"user"`, `"service_account"`).
    pub subject_type: String,
    /// Metric name for this observation.
    pub metric: String,
    /// Optional idempotency key; if omitted, one is generated when building the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Numeric value for this usage observation.
    pub value: f64,
    /// Observation timestamp (UTC).
    pub timestamp: DateTime<Utc>,
    /// Optional caller-supplied metadata. Serialized size MUST NOT exceed 8 192 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// One allowed metric entry in a module config response.
#[derive(Debug, Clone)]
#[modkit_macros::api_dto(response)]
pub struct AllowedMetricResponse {
    /// Metric name.
    pub name: String,
    /// Gauge vs counter semantics.
    pub kind: UsageKind,
}

/// Response body for the get-module-config endpoint.
#[derive(Debug, Clone)]
#[modkit_macros::api_dto(response)]
pub struct ModuleConfigResponse {
    /// Metrics the module is allowed to emit.
    pub allowed_metrics: Vec<AllowedMetricResponse>,
}
