//! REST handlers for the usage-collector gateway.

use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use axum::extract::{Path, Query};
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use http::StatusCode;
use modkit::api::problem::{Problem, internal_error};
use modkit_security::SecurityContext;
use usage_collector_sdk::models::{AggregationQuery, GroupByDimension, RawQuery};
use usage_collector_sdk::{
    CursorV1, Page, UsageCollectorClientV1, UsageCollectorError, UsageCollectorPluginClientV1,
    UsageRecord,
};
use usage_emitter::{UsageEmitterError, UsageEmitterV1};

use crate::config::{DEFAULT_PAGE_SIZE, MAX_AGG_ROWS, MAX_FILTER_STRING_LEN, MAX_PAGE_SIZE, MAX_QUERY_TIME_RANGE};
use crate::domain::authz::{USAGE_RECORD_READ, actions, authorize_and_compile_scope};

use super::dto::{
    AggregatedQueryParams, AggregationResultDto, AllowedMetricResponse, CreateUsageRecordRequest,
    ModuleConfigResponse, RawQueryParams,
};

/// Handler for `POST /usage-collector/v1/records`.
///
/// # Errors
/// Returns a [`Problem`] on authorization failure, validation errors, or internal emitter failure.
pub async fn handle_create_usage_record(
    Extension(ctx): Extension<SecurityContext>,
    Extension(emitter): Extension<Arc<dyn UsageEmitterV1>>,
    Json(req): Json<CreateUsageRecordRequest>,
) -> Result<StatusCode, Problem> {
    // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-1
    if let Some(err) = validate_metadata_size(req.metadata.as_ref()) {
        return Err(err);
    }
    // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-1

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

    builder.enqueue().await.map_err(emitter_error_to_problem)?;

    // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-7
    Ok(StatusCode::NO_CONTENT)
    // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-7
}

fn validate_metadata_size(metadata: Option<&serde_json::Value>) -> Option<Problem> {
    let meta = metadata?;
    let byte_len = serde_json::to_vec(meta).map_or(0, |v| v.len());
    if byte_len > 8192 {
        tracing::warn!(
            byte_len,
            limit = 8192,
            "Metadata byte length exceeds limit; rejecting record"
        );
        return Some(Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Metadata too large",
            format!("metadata byte length {byte_len} exceeds limit of 8192"),
        ));
    }
    None
}

async fn authorize_request(
    ctx: &SecurityContext,
    emitter: &Arc<dyn UsageEmitterV1>,
    req: &CreateUsageRecordRequest,
) -> Result<usage_emitter::AuthorizedUsageEmitter, Problem> {
    match (&req.subject_id, &req.subject_type) {
        (None, None) => {
            // No subject — skip PDP authorization.
            tracing::debug!("No subject fields present; skipping PDP authorization");
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
                .map_err(emitter_error_to_problem)
        }
        (Some(sid_str), _) => {
            // subject_id present (subject_type optional) — authorize via PDP.
            let subject_id = sid_str.parse::<uuid::Uuid>().map_err(|_| {
                Problem::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Invalid subject_id",
                    "subject_id must be a valid UUID",
                )
            })?;
            emitter
                .for_module(&req.module)
                .authorize_for(
                    ctx,
                    req.tenant_id,
                    req.resource_id,
                    req.resource_type.clone(),
                    Some(subject_id),
                    req.subject_type.clone(),
                )
                .await
                .map_err(emitter_error_to_problem)
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
    Extension(collector): Extension<Arc<dyn UsageCollectorClientV1>>,
) -> Result<Json<ModuleConfigResponse>, Problem> {
    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-2
    // Authenticated request received; ModKit pipeline enforces authentication before handler entry.
    // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-2

    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-3
    let result = collector.get_module_config(&module_name).await;
    // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-3

    // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4
    let config = match result {
        // @cpt-begin:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4a
        Err(UsageCollectorError::ModuleNotFound {
            module_name: ref name,
        }) => {
            return Err(Problem::new(
                StatusCode::NOT_FOUND,
                "Module not found",
                format!("module '{name}' is not configured"),
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config:p2:inst-cfg-4a
        Err(e) => return Err(internal_error(e.to_string())),
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
    Extension(plugin): Extension<Arc<dyn UsageCollectorPluginClientV1>>,
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
        errors.push(
            "time range must be strictly ascending (from must be before to)".to_owned(),
        );
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3a

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3b
    if let Ok(d) = params.to.signed_duration_since(params.from).to_std()
        && d > MAX_QUERY_TIME_RANGE
    {
        errors.push("time range exceeds maximum allowed duration".to_owned());
    }
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-3b

    if params.group_by.iter().any(|d| matches!(d, GroupByDimension::TimeBucket(_)))
        && params.bucket_size.is_none()
    {
        errors.push(
            "bucket_size: required when group_by includes time_bucket".to_owned(),
        );
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
        authorize_and_compile_scope(&ctx, Arc::clone(&authz), &USAGE_RECORD_READ, actions::LIST)
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
    let rows = match plugin.query_aggregated(query).await {
        Ok(rows) => rows,
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8b
        Err(UsageCollectorError::QueryResultTooLarge { .. }) => {
            return Err(Problem::new(
                StatusCode::BAD_REQUEST,
                "Query too broad",
                r#"{"error":"query too broad"}"#.to_owned(),
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8b
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8c
        Err(e) => {
            let correlation_id = ctx.subject_id().to_string();
            tracing::error!(
                correlation_id = %correlation_id,
                error = %e,
                "Storage plugin error during query_aggregated"
            );
            return Err(Problem::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Service unavailable",
                serde_json::json!({
                    "error": "service_unavailable",
                    "correlation_id": correlation_id,
                })
                .to_string(),
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-aggregated:p1:inst-agg-8c
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
    Extension(plugin): Extension<Arc<dyn UsageCollectorPluginClientV1>>,
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
        errors.push(
            "time range must be strictly ascending (from must be before to)".to_owned(),
        );
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

    let decoded_cursor = if let Some(ref cursor_str) = params.cursor {
        if let Ok(cursor) = CursorV1::decode(cursor_str) {
            let ts_str = cursor.k.first().map_or("", String::as_str);
            if let Ok(ts) = DateTime::parse_from_rfc3339(ts_str) {
                let ts_utc = ts.with_timezone(&Utc);
                if ts_utc < params.from || ts_utc > params.to {
                    errors.push(
                        "cursor: timestamp is outside the requested [from, to] range".to_owned(),
                    );
                    None
                } else {
                    Some(cursor)
                }
            } else {
                errors.push("cursor: malformed cursor".to_owned());
                None
            }
        } else {
            errors.push("cursor: malformed cursor".to_owned());
            None
        }
    } else {
        None
    };
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
    let Ok(scope) = authorize_and_compile_scope(
        &ctx,
        Arc::clone(&authz),
        &USAGE_RECORD_READ,
        actions::LIST,
    )
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
    let paged = match plugin.query_raw(query).await {
        Ok(p) => p,
        // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8b
        Err(e) => {
            let correlation_id = ctx.subject_id().to_string();
            tracing::error!(
                correlation_id = %correlation_id,
                error = %e,
                "Storage plugin error during query_raw"
            );
            return Err(Problem::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Service unavailable",
                serde_json::json!({
                    "error": "service_unavailable",
                    "correlation_id": correlation_id,
                })
                .to_string(),
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8b
    };
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-8

    // @cpt-begin:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-9
    Ok(Json(paged))
    // @cpt-end:cpt-cf-usage-collector-flow-query-api-raw:p2:inst-raw-9
}

fn emitter_error_to_problem(e: UsageEmitterError) -> Problem {
    match e {
        UsageEmitterError::AuthorizationFailed { message } => {
            Problem::new(StatusCode::FORBIDDEN, "Forbidden", message)
        }
        UsageEmitterError::AuthorizationExpired => {
            Problem::new(StatusCode::FORBIDDEN, "Forbidden", e.to_string())
        }
        UsageEmitterError::InvalidRecord { message } => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Invalid usage record",
            message,
        ),
        UsageEmitterError::MetricNotAllowed { metric } => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Metric not allowed",
            format!("metric not allowed for this module: {metric}"),
        ),
        UsageEmitterError::MetricKindMismatch {
            metric,
            expected,
            actual,
        } => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Metric kind mismatch",
            format!("metric '{metric}' expects kind {expected:?} but record specifies {actual:?}"),
        ),
        UsageEmitterError::NegativeCounterValue { value } => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Invalid usage record",
            format!("counter usage record has a negative value: {value}"),
        ),
        UsageEmitterError::MetadataTooLarge { len } => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Metadata too large",
            format!("metadata byte length {len} exceeds the 8192-byte limit"),
        ),
        UsageEmitterError::ModuleNotConfigured { module_name } => Problem::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Module not configured",
            format!("module '{module_name}' is not configured in the gateway"),
        ),
        UsageEmitterError::Internal { message } => internal_error(message),
        UsageEmitterError::Outbox(err) => internal_error(err.to_string()),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "handlers_tests.rs"]
mod handlers_tests;
