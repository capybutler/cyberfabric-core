use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
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
}

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

/// Error decoding a cursor.
#[derive(Debug, thiserror::Error)]
pub enum CursorDecodeError {
    #[error("invalid base64")]
    InvalidBase64,
    #[error("invalid UTF-8 in cursor payload")]
    InvalidUtf8,
    #[error("missing cursor field: {0}")]
    MissingField(&'static str),
    #[error("invalid timestamp in cursor")]
    InvalidTimestamp,
    #[error("invalid UUID in cursor")]
    InvalidUuid,
}

/// Opaque pagination cursor encoding an exclusive lower bound (timestamp, id).
///
/// Serializes as a base64-encoded string in JSON responses.
/// Payload format: `timestamp=<RFC3339>&id=<UUID>` base64-encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    pub timestamp: DateTime<Utc>,
    pub id: Uuid,
}

impl Cursor {
    /// Encode this cursor as a base64 string.
    #[must_use]
    pub fn encode(&self) -> String {
        let payload = format!(
            "timestamp={}&id={}",
            self.timestamp.to_rfc3339(),
            self.id
        );
        BASE64.encode(payload.as_bytes())
    }

    /// Decode a base64 cursor string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid base64 or the payload is malformed.
    pub fn decode(encoded: &str) -> Result<Self, CursorDecodeError> {
        let bytes = BASE64
            .decode(encoded)
            .map_err(|_| CursorDecodeError::InvalidBase64)?;
        let payload =
            String::from_utf8(bytes).map_err(|_| CursorDecodeError::InvalidUtf8)?;

        let timestamp_str = cursor_field(&payload, "timestamp=")
            .ok_or(CursorDecodeError::MissingField("timestamp"))?;
        let id_str = cursor_field(&payload, "id=")
            .ok_or(CursorDecodeError::MissingField("id"))?;

        let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
            .map_err(|_| CursorDecodeError::InvalidTimestamp)?
            .with_timezone(&Utc);
        let id = Uuid::parse_str(&id_str).map_err(|_| CursorDecodeError::InvalidUuid)?;

        Ok(Self { timestamp, id })
    }
}

fn cursor_field(payload: &str, prefix: &str) -> Option<String> {
    payload
        .split('&')
        .find(|part| part.starts_with(prefix))
        .map(|part| part[prefix.len()..].to_owned())
}

impl Serialize for Cursor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encode())
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Cursor::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Paginated result returned by `query_raw`.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[schema(value_type = Option<String>)]
    pub next_cursor: Option<Cursor>,
}

/// Parameters for a raw usage record query delegated to the storage plugin.
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
    pub cursor: Option<Cursor>,
    /// Number of records per page.
    pub page_size: usize,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "models_tests.rs"]
mod models_tests;
