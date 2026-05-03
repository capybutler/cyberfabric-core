use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use modkit_odata::CursorV1;
use modkit_security::AccessScope;

/// Kind of numeric usage observation (gauge vs counter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    Gauge,
    Counter,
}

/// A single allowed metric definition returned by [`crate::UsageCollectorClientV1::get_module_config`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedMetric {
    /// Metric name.
    pub name: String,
    /// Gauge vs counter semantics for this metric.
    pub kind: UsageKind,
}

/// Per-module configuration returned by [`crate::UsageCollectorClientV1::get_module_config`].
///
/// Extensible: future fields may include rate limit config, max metadata size, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfig {
    /// Metrics this module is allowed to emit.
    pub allowed_metrics: Vec<AllowedMetric>,
}

/// A single usage record submitted to the collector.
///
/// All fields are public for direct construction, serde, and tests.
/// For emission from source modules, use the `usage-emitter` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Name of the module that emitted this record.
    pub module: String,
    /// Tenant that owns this usage observation.
    pub tenant_id: Uuid,
    /// Metric name for this observation.
    pub metric: String,
    /// Gauge vs counter semantics.
    pub kind: UsageKind,
    /// Numeric value for this usage observation.
    pub value: f64,
    /// Identifier of the metered resource instance.
    pub resource_id: Uuid,
    /// Logical type of the metered resource (e.g. GTS id or domain name).
    pub resource_type: String,
    /// Identifier of the subject (user or service) performing the metered action.
    /// `None` when no subject context is available; PDP validation is skipped in that case.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_id: Option<Uuid>,
    /// Logical type of the subject (e.g. GTS id or domain name).
    /// `None` when no subject context is available.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_type: Option<String>,
    /// Idempotency key for at-least-once delivery.
    pub idempotency_key: String,
    /// Timestamp of the observation.
    pub timestamp: DateTime<Utc>,
    /// Optional caller-supplied metadata (max 8 192 bytes serialized).
    /// Absent when not provided; serializes as absent JSON field, not `null`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Value>,
}

// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-1
/// Aggregation function applied over matching usage records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AggregationFn {
    Sum,
    Count,
    Min,
    Max,
    Avg,
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-1

// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-2
/// Time granularity for time-bucket grouping in aggregation queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BucketSize {
    Minute,
    Hour,
    Day,
    Week,
    Month,
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-2

// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-3
/// Dimension by which aggregation results may be grouped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupByDimension {
    TimeBucket(BucketSize),
    UsageType,
    Subject,
    Resource,
    Source,
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-3

// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-4
/// Parameters for an aggregated usage query delegated to the storage plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationQuery {
    /// Access scope compiled from PDP constraints. Excluded from serde (`AccessScope` is not serializable).
    #[serde(skip)]
    pub scope: AccessScope,
    /// Mandatory time range (from, to).
    pub time_range: (DateTime<Utc>, DateTime<Utc>),
    /// Aggregation function to apply.
    pub function: AggregationFn,
    /// Dimensions to group results by.
    pub group_by: Vec<GroupByDimension>,
    /// Required when `group_by` contains `TimeBucket`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bucket_size: Option<BucketSize>,
    /// Optional filter: usage type name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage_type: Option<String>,
    /// Optional filter: resource UUID.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resource_id: Option<Uuid>,
    /// Optional filter: resource type.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resource_type: Option<String>,
    /// Optional filter: subject UUID.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_id: Option<Uuid>,
    /// Optional filter: subject type.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_type: Option<String>,
    /// Optional filter: source module name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    /// Maximum number of result rows; populated by the gateway from `MAX_AGG_ROWS`.
    /// Storage plugins MUST NOT return more rows than this limit.
    #[serde(skip)]
    pub max_rows: usize,
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-4

// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-5
/// A single row in an aggregation result set.
///
/// Option fields are absent (not null) in JSON when the corresponding
/// `GroupByDimension` was not requested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AggregationResult {
    pub function: AggregationFn,
    pub value: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bucket_start: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resource_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resource_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-5

// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-7
/// Parameters for a raw usage record query delegated to the storage plugin.
///
/// Note: source-level filtering is intentionally absent from `RawQuery`.
/// `AggregationQuery` supports source filtering via `AggregationQuery::source`;
/// raw query source filtering is deferred to a future feature revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawQuery {
    /// Access scope compiled from PDP constraints. Excluded from serde (`AccessScope` is not serializable).
    #[serde(skip)]
    pub scope: AccessScope,
    /// Mandatory time range (from, to).
    pub time_range: (DateTime<Utc>, DateTime<Utc>),
    /// Optional filter: usage type name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage_type: Option<String>,
    /// Optional filter: resource UUID.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resource_id: Option<Uuid>,
    /// Optional filter: resource type.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resource_type: Option<String>,
    /// Optional filter: subject type.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_type: Option<String>,
    /// Optional filter: subject UUID.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subject_id: Option<Uuid>,
    /// Pagination cursor for the next page.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cursor: Option<CursorV1>,
    /// Number of records per page.
    pub page_size: usize,
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-7

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "models_tests.rs"]
mod models_tests;
