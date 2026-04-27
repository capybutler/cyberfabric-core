//! REST handlers for the usage-collector gateway.

use std::sync::Arc;

use axum::extract::Path;
use axum::{Extension, Json};
use http::StatusCode;
use modkit::api::problem::{Problem, internal_error};
use modkit_security::SecurityContext;
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use usage_emitter::{UsageEmitterError, UsageEmitterV1};

use super::dto::{AllowedMetricResponse, CreateUsageRecordRequest, ModuleConfigResponse};

/// Handler for `POST /usage-collector/v1/records`.
pub async fn handle_create_usage_record(
    Extension(ctx): Extension<SecurityContext>,
    Extension(emitter): Extension<Arc<dyn UsageEmitterV1>>,
    Json(req): Json<CreateUsageRecordRequest>,
) -> Result<StatusCode, Problem> {
    // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-1
    if let Some(ref meta) = req.metadata {
        let byte_len = serde_json::to_vec(meta).map_or(0, |v| v.len());
        if byte_len > 8192 {
            tracing::warn!(
                byte_len,
                limit = 8192,
                "Metadata byte length exceeds limit; rejecting record"
            );
            return Err(Problem::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Metadata too large",
                format!("metadata byte length {byte_len} exceeds limit of 8192"),
            ));
        }
    }
    // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler:inst-gw-1

    let authorized = emitter
        .for_module(&req.module)
        .authorize_for(
            &ctx,
            req.tenant_id,
            req.resource_id,
            req.resource_type,
            req.subject_id,
            req.subject_type,
        )
        .await
        .map_err(emitter_error_to_problem)?;

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

/// Handler for `GET /usage-collector/v1/modules/{module_name}/config`.
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
