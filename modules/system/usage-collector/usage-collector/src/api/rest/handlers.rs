//! REST handlers for the usage-collector gateway.

use std::sync::Arc;
use std::time::Duration;

use authz_resolver_sdk::AuthZResolverClient;
use axum::extract::{Path, Query};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use http::StatusCode;
use modkit::api::problem::Problem;
use modkit_security::SecurityContext;
use usage_collector_sdk::authz::{USAGE_RECORD, actions};
use usage_collector_sdk::models::{AggregationQuery, GroupByDimension, RawQuery};
use usage_collector_sdk::{CursorV1, Page, UsageRecord, UsageCollectorError};
use usage_emitter::UsageEmitterFactoryV1;

use crate::domain::authz::authorize_and_compile_scope;

use crate::domain::{DomainError, Service};

use super::dto::{
    AggregatedQueryParams, AggregationResultDto, AllowedMetricResponse, CreateUsageRecordRequest,
    ModuleConfigResponse, RawQueryParams,
};

/// Default number of raw records returned per page when `page_size` is absent.
const DEFAULT_PAGE_SIZE: usize = 100;

/// Maximum allowed value for `page_size` in raw queries.
const MAX_PAGE_SIZE: usize = 1_000;

/// Maximum number of rows `query_aggregated` may return before returning `QueryResultTooLarge`.
const MAX_AGG_ROWS: usize = 10_000;

/// Maximum byte length for string filter fields (`usage_type`, `resource_type`, `subject_type`, `source`).
const MAX_FILTER_STRING_LEN: usize = 256;

/// Maximum allowed query time range (from, to) per request (~1 year).
const MAX_QUERY_TIME_RANGE: Duration = Duration::from_hours(8784);

/// Handler for `POST /usage-collector/v1/records`.
///
/// # Errors
/// Returns a [`Problem`] on authorization failure, validation errors, or internal emitter failure.
pub async fn handle_create_usage_record(
    Extension(ctx): Extension<SecurityContext>,
    Extension(emitter): Extension<Arc<dyn UsageEmitterFactoryV1>>,
    Json(req): Json<CreateUsageRecordRequest>,
) -> Result<StatusCode, Problem> {
    let authorized = authorize_request(&ctx, &emitter, &req).await?;

    let mut builder = authorized
        .build_usage_record(req.metric, req.value)
        .with_timestamp(req.timestamp);

    if let Some(key) = req.idempotency_key.filter(|k| !k.is_empty()) {
        builder = builder.with_idempotency_key(key);
    }

    if let Some(meta) = req.metadata {
        builder = builder.with_metadata(meta);
    }

    builder.enqueue().await.map_err(|e| canonical_error_to_problem(&e))?;

    // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-7
    Ok(StatusCode::NO_CONTENT)
    // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-7
}

async fn authorize_request(
    ctx: &SecurityContext,
    emitter: &Arc<dyn UsageEmitterFactoryV1>,
    req: &CreateUsageRecordRequest,
) -> Result<usage_emitter::AuthorizedUsageEmitter, Problem> {
    match (&req.subject_id, &req.subject_type) {
        (None, None) => {
            // No subject context — call PDP without subject_id/subject_type properties.
            tracing::debug!(
                "No subject fields present; calling PDP without subject_id/subject_type properties"
            );
            emitter
                .for_module(&req.module)
                .authorize_for(
                    ctx,
                    req.tenant_id,
                    req.resource_id,
                    req.resource_type.clone(),
                    None,
                    None,
                )
                .await
                .map_err(|e| canonical_error_to_problem(&e))
        }
        (Some(subject_id), _) => {
            // subject_id present (subject_type optional) — authorize via PDP.
            emitter
                .for_module(&req.module)
                .authorize_for(
                    ctx,
                    req.tenant_id,
                    req.resource_id,
                    req.resource_type.clone(),
                    Some(*subject_id),
                    req.subject_type.clone(),
                )
                .await
                .map_err(|e| canonical_error_to_problem(&e))
        }
        _ => {
            // subject_type present without subject_id — invalid.
            Err(Problem::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid subject",
                "subject_type requires subject_id to be present",
            ))
        }
    }
}

/// Handler for `GET /usage-collector/v1/modules/{module_name}/config`.
///
/// # Errors
/// Returns a [`Problem`] if the module is not found or the collector call fails.
// @cpt-flow:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2
pub async fn handle_get_module_config(
    Path(module_name): Path<String>,
    Extension(service): Extension<Arc<Service>>,
) -> Result<Json<ModuleConfigResponse>, Problem> {
    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-2
    // Authenticated request received; ModKit pipeline enforces authentication before handler entry.
    // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-2

    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-3
    let result = service.get_module_config(&module_name);
    // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-3

    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4
    let config = match result {
        // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4a
        Err(e) => return Err(domain_error_to_problem(&e)),
        // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4a
        Ok(c) => c,
    };
    // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4

    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-5
    let response = ModuleConfigResponse {
        allowed_metrics: config
            .allowed_metrics
            .into_iter()
            .map(|m| AllowedMetricResponse {
                name: m.name,
                kind: m.kind,
            })
            .collect(),
    };

    Ok(Json(response))
    // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-5
}

/// Handler for `GET /usage-collector/v1/aggregated`.
///
/// # Errors
/// Returns a [`Problem`] on authorization failure, validation errors, or plugin call failure.
// @cpt-flow:cpt-cf-usage-collector-flow-query-api-aggregated:p1
pub async fn handle_query_aggregated(
    Extension(ctx): Extension<SecurityContext>,
    Extension(authz): Extension<Arc<dyn AuthZResolverClient>>,
    Extension(service): Extension<Arc<Service>>,
    Query(params): Query<AggregatedQueryParams>,
) -> Result<Json<Vec<AggregationResultDto>>, Problem> {
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-1
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-2
    // tenant_id is derived from ctx; params must not contain tenant_id
    // (enforced by DTO definition — no tenant_id field in AggregatedQueryParams)
    let mut errors: Vec<String> = Vec::new();
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-2
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-1

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3a
    if params.from >= params.to {
        errors.push("time range must be strictly ascending (from must be before to)".to_owned());
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3a

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3b
    if let Ok(d) = params.to.signed_duration_since(params.from).to_std()
        && d > MAX_QUERY_TIME_RANGE
    {
        errors.push("time range exceeds maximum allowed duration".to_owned());
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3b

    if params
        .group_by
        .iter()
        .any(|d| matches!(d, GroupByDimension::TimeBucket(_)))
        && params.bucket_size.is_none()
    {
        errors.push("bucket_size: required when group_by includes time_bucket".to_owned());
    }

    if let Some(s) = &params.usage_type
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "usage_type: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }
    if let Some(s) = &params.resource_type
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "resource_type: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }
    if let Some(s) = &params.subject_type
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "subject_type: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }
    if let Some(s) = &params.source
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "source: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-4
    if !errors.is_empty() {
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-4a
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "Validation error",
            serde_json::json!({
                "error": "validation failed",
                "code": "VALIDATION_ERROR",
                "details": errors,
            })
            .to_string(),
        ));
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-4a
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-4

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-5
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-6
    let Ok(scope) =
        authorize_and_compile_scope(&ctx, Arc::clone(&authz), &USAGE_RECORD, actions::LIST)
            .await
    else {
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-6a
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "Forbidden",
            r#"{"error":"forbidden"}"#.to_owned(),
        ));
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-6a
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-6
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-5

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-7
    let query = AggregationQuery {
        scope,
        time_range: (params.from, params.to),
        function: params.fn_,
        group_by: params.group_by,
        bucket_size: params.bucket_size,
        usage_type: params.usage_type,
        resource_id: params.resource_id,
        resource_type: params.resource_type,
        subject_id: params.subject_id,
        subject_type: params.subject_type,
        source: params.source,
        max_rows: MAX_AGG_ROWS,
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-7

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8
    let rows = match service.query_aggregated(query).await {
        Ok(rows) => rows,
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8b
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8b
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8c
        Err(e) => {
            tracing::error!(
                subject_id = %ctx.subject_id(),
                error = %e,
                "Storage plugin error during query_aggregated"
            );
            return Err(domain_error_to_problem(&e));
        } // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8c
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-9
    let response: Vec<AggregationResultDto> =
        rows.into_iter().map(AggregationResultDto::from).collect();
    Ok(Json(response))
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-9
}

/// Handler for `GET /usage-collector/v1/raw`.
///
/// # Errors
/// Returns a [`Problem`] on authorization failure, validation errors, or plugin call failure.
// @cpt-flow:cpt-cf-usage-collector-flow-query-api-raw:p2
pub async fn handle_query_raw(
    Extension(ctx): Extension<SecurityContext>,
    Extension(authz): Extension<Arc<dyn AuthZResolverClient>>,
    Extension(service): Extension<Arc<Service>>,
    Query(params): Query<RawQueryParams>,
) -> Result<Json<Page<UsageRecord>>, Problem> {
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-1
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-2
    // tenant_id is derived from ctx; params must not contain tenant_id
    // (enforced by DTO definition — no tenant_id field in RawQueryParams)
    let mut errors: Vec<String> = Vec::new();
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-2
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-1

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-3
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-3a
    if params.from >= params.to {
        errors.push("time range must be strictly ascending (from must be before to)".to_owned());
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-3a

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-3b
    if let Ok(d) = params.to.signed_duration_since(params.from).to_std()
        && d > MAX_QUERY_TIME_RANGE
    {
        errors.push("time range exceeds maximum allowed duration".to_owned());
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-3b

    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-0
    let page_size = match params.page_size {
        None => DEFAULT_PAGE_SIZE,
        Some(0) => {
            errors.push("page_size: must be at least 1".to_owned());
            DEFAULT_PAGE_SIZE
        }
        Some(ps) if ps > MAX_PAGE_SIZE => {
            errors.push(format!("page_size: must not exceed {MAX_PAGE_SIZE}"));
            DEFAULT_PAGE_SIZE
        }
        Some(ps) => ps,
    };
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-0

    if let Some(s) = &params.usage_type
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "usage_type: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }
    if let Some(s) = &params.resource_type
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "resource_type: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }
    if let Some(s) = &params.subject_type
        && s.len() > MAX_FILTER_STRING_LEN
    {
        errors.push(format!(
            "subject_type: exceeds maximum length of {MAX_FILTER_STRING_LEN} bytes"
        ));
    }

    let decoded_cursor = params.cursor.as_deref().and_then(|cursor_str| {
        decode_and_validate_cursor(cursor_str, params.from, params.to, &mut errors)
    });
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-3

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-4
    if !errors.is_empty() {
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-4a
        return Err(Problem::new(
            StatusCode::BAD_REQUEST,
            "Validation error",
            serde_json::json!({
                "error": "validation failed",
                "code": "VALIDATION_ERROR",
                "details": errors,
            })
            .to_string(),
        ));
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-4a
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-4

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-5
    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-6
    let Ok(scope) =
        authorize_and_compile_scope(&ctx, Arc::clone(&authz), &USAGE_RECORD, actions::LIST)
            .await
    else {
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-6a
        return Err(Problem::new(
            StatusCode::FORBIDDEN,
            "Forbidden",
            r#"{"error":"forbidden"}"#.to_owned(),
        ));
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-6a
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-6
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-5

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-7
    let query = RawQuery {
        scope,
        time_range: (params.from, params.to),
        cursor: decoded_cursor,
        page_size,
        usage_type: params.usage_type,
        resource_id: params.resource_id,
        resource_type: params.resource_type,
        subject_type: params.subject_type,
        subject_id: params.subject_id,
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-7

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8
    let paged = match service.query_raw(query).await {
        Ok(p) => p,
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8b
        Err(e) => {
            tracing::error!(
                subject_id = %ctx.subject_id(),
                error = %e,
                "Storage plugin error during query_raw"
            );
            return Err(domain_error_to_problem(&e));
        } // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8b
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-9
    Ok(Json(paged))
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-9
}

fn decode_and_validate_cursor(
    cursor_str: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    errors: &mut Vec<String>,
) -> Option<CursorV1> {
    let Ok(cursor) = CursorV1::decode(cursor_str) else {
        errors.push("cursor: malformed cursor".to_owned());
        return None;
    };
    if cursor.k.len() < 2 {
        errors.push("cursor: malformed cursor".to_owned());
        return None;
    }
    let ts_str = cursor.k[0].as_str();
    let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) else {
        errors.push("cursor: malformed cursor".to_owned());
        return None;
    };
    let ts_utc = ts.with_timezone(&Utc);
    if ts_utc < from || ts_utc > to {
        errors.push("cursor: timestamp is outside the requested [from, to] range".to_owned());
        return None;
    }
    Some(cursor)
}

fn canonical_error_to_problem(e: &UsageCollectorError) -> Problem {
    Problem::new(
        StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        e.title(),
        e.detail(),
    )
}

fn domain_error_to_problem(e: &DomainError) -> Problem {
    match e {
        DomainError::ModuleNotConfigured { module } => Problem::new(
            StatusCode::NOT_FOUND,
            "Not Found",
            format!("module '{module}' not configured"),
        ),
        DomainError::Timeout => Problem::new(
            StatusCode::GATEWAY_TIMEOUT,
            "Gateway Timeout",
            "plugin call timed out",
        ),
        DomainError::CircuitOpen => Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            "circuit breaker open",
        ),
        DomainError::PluginUnavailable { gts_id, reason } => Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            format!("plugin not available for '{gts_id}': {reason}"),
        ),
        DomainError::PluginNotFound { vendor } => Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            format!("no plugin instances found for vendor '{vendor}'"),
        ),
        DomainError::TypesRegistryUnavailable(reason) => Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            format!("types registry is not available: {reason}"),
        ),
        DomainError::Plugin(canonical) => canonical_error_to_problem(canonical),
        DomainError::InvalidPluginInstance { gts_id, reason } => Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            format!("invalid plugin instance '{gts_id}': {reason}"),
        ),
        DomainError::Internal(reason) => Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            reason.clone(),
        ),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "handlers_tests.rs"]
mod handlers_tests;
