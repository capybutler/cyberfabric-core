//! REST DTOs for the usage-collector gateway.

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL};
use chrono::{DateTime, Utc};
use usage_collector_sdk::models::{
    AggregationFn, AggregationResult, BucketSize, GroupByDimension, UsageKind, UsageRecord,
};
use uuid::Uuid;

/// Decoded cursor components: `(timestamp, record_id, issued_at)`.
type DecodedCursor = (DateTime<Utc>, Uuid, DateTime<Utc>);

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
    /// `None` when no subject context is available; PDP subject validation is skipped in that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
    /// Type of the subject (e.g. `"user"`, `"service_account"`).
    /// `None` when no subject context is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
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

/// Query parameters for `GET /usage-collector/v1/aggregated`.
///
/// `tenant_id` is NEVER accepted here — it is derived from `SecurityContext` only.
#[derive(Debug, Clone)]
#[modkit_macros::api_dto(request)]
pub struct AggregatedQueryParams {
    /// Aggregation function to apply.
    #[serde(rename = "fn")]
    pub fn_: AggregationFn,
    /// Start of the time range (RFC 3339 UTC, exclusive lower bound).
    pub from: DateTime<Utc>,
    /// End of the time range (RFC 3339 UTC, exclusive upper bound).
    pub to: DateTime<Utc>,
    /// Dimensions to group by.
    #[serde(default)]
    pub group_by: Vec<GroupByDimension>,
    /// Required when `group_by` includes `TimeBucket`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_size: Option<BucketSize>,
    /// Filter by usage type. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_type: Option<String>,
    /// Filter by subject UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<Uuid>,
    /// Filter by subject type string. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
    /// Filter by resource UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<Uuid>,
    /// Filter by resource type string. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    /// Filter by source module. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Response type alias for `GET /usage-collector/v1/aggregated`.
#[allow(dead_code)]
pub type AggregationResultResponse = Vec<AggregationResultDto>;

/// One row in an aggregated query result.
///
/// Option fields are absent (not null) in JSON when the corresponding
/// `GroupByDimension` was not requested.
#[derive(Debug, Clone)]
#[modkit_macros::api_dto(response)]
pub struct AggregationResultDto {
    pub function: AggregationFn,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_start: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl From<AggregationResult> for AggregationResultDto {
    fn from(r: AggregationResult) -> Self {
        Self {
            function: r.function,
            value: r.value,
            bucket_start: r.bucket_start,
            usage_type: r.usage_type,
            subject_id: r.subject_id,
            subject_type: r.subject_type,
            resource_id: r.resource_id,
            resource_type: r.resource_type,
            source: r.source,
        }
    }
}

/// Error returned when decoding a gateway pagination cursor fails.
#[derive(Debug)]
pub enum CursorError {
    Malformed,
}

/// Encode `(timestamp, id, issued_at)` as a base64url gateway cursor string.
///
/// Format: `base64url("<timestamp_rfc3339>,<uuid>,<issued_at_rfc3339>")`.
/// The caller supplies `issued_at`; pass `Utc::now()` in production code.
// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-6
pub fn cursor_encode(timestamp: DateTime<Utc>, id: Uuid, issued_at: DateTime<Utc>) -> String {
    let payload = format!("{},{},{}", timestamp.to_rfc3339(), id, issued_at.to_rfc3339());
    BASE64URL.encode(payload.as_bytes())
}

/// Decode a gateway cursor string into `(timestamp, id, issued_at)`.
///
/// Returns `Err(CursorError::Malformed)` on any parse failure.
pub fn cursor_decode(raw: &str) -> Result<DecodedCursor, CursorError> {
    let bytes = BASE64URL.decode(raw).map_err(|_| CursorError::Malformed)?;
    let payload = String::from_utf8(bytes).map_err(|_| CursorError::Malformed)?;
    let mut parts = payload.splitn(3, ',');
    let ts_str = parts.next().ok_or(CursorError::Malformed)?;
    let id_str = parts.next().ok_or(CursorError::Malformed)?;
    let issued_at_str = parts.next().ok_or(CursorError::Malformed)?;
    let timestamp = DateTime::parse_from_rfc3339(ts_str)
        .map_err(|_| CursorError::Malformed)?
        .with_timezone(&Utc);
    let id = Uuid::parse_str(id_str).map_err(|_| CursorError::Malformed)?;
    let issued_at = DateTime::parse_from_rfc3339(issued_at_str)
        .map_err(|_| CursorError::Malformed)?
        .with_timezone(&Utc);
    Ok((timestamp, id, issued_at))
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-6

/// Returns `true` when the cursor has exceeded its TTL.
///
/// Compares `issued_at` (the wall-clock time the cursor was created) against `ttl`,
/// not the data record timestamp. These are fundamentally different: one is when the
/// token was issued, the other is where pagination is positioned in the data.
// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-6a
pub fn cursor_check_ttl(issued_at: DateTime<Utc>, now: DateTime<Utc>, ttl: Duration) -> bool {
    (now - issued_at)
        .to_std()
        .is_ok_and(|age| age > ttl)
}
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-6a

/// Query parameters for `GET /usage-collector/v1/raw`.
///
/// `tenant_id` is NEVER accepted here — it is derived from `SecurityContext` only.
#[derive(Debug, Clone)]
#[modkit_macros::api_dto(request)]
pub struct RawQueryParams {
    /// Start of the time range (RFC 3339 UTC, exclusive lower bound).
    pub from: DateTime<Utc>,
    /// End of the time range (RFC 3339 UTC, exclusive upper bound).
    pub to: DateTime<Utc>,
    /// Opaque pagination cursor from a previous response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Number of records per page. Defaults to `DEFAULT_PAGE_SIZE`; must be in `[1, MAX_PAGE_SIZE]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<usize>,
    /// Filter by usage type. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_type: Option<String>,
    /// Filter by subject UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<Uuid>,
    /// Filter by subject type string. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
    /// Filter by resource UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<Uuid>,
    /// Filter by resource type string. Max length: `MAX_FILTER_STRING_LEN` bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
}

/// Response body for `GET /usage-collector/v1/raw`.
///
/// Absent `next_cursor` signals the final page. Empty `items` with absent `next_cursor` is a
/// valid success.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PagedResultResponse {
    pub items: Vec<UsageRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}
