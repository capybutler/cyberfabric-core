//! Route registration for the usage-collector REST API.

use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use axum::{Extension, Router};

use modkit::api::operation_builder::LicenseFeature;
use modkit::api::{OpenApiRegistry, OperationBuilder};
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorPluginClientV1};
use usage_emitter::UsageEmitterV1;

use super::dto::{AggregationResultDto, CreateUsageRecordRequest, ModuleConfigResponse};
use super::handlers;

const API_TAG: &str = "Usage Collector";

struct License;

impl AsRef<str> for License {
    fn as_ref(&self) -> &'static str {
        "gts.x.core.lic.feat.v1~x.core.global.base.v1"
    }
}

impl LicenseFeature for License {}

/// Register all REST routes for the usage-collector module.
pub fn register_routes(
    router: Router,
    openapi: &dyn OpenApiRegistry,
    emitter: Arc<dyn UsageEmitterV1>,
    collector: Arc<dyn UsageCollectorClientV1>,
    authz_client: Arc<dyn AuthZResolverClient>,
    plugin_client: Arc<dyn UsageCollectorPluginClientV1>,
) -> Router {
    let router = OperationBuilder::post("/usage-collector/v1/records")
        .operation_id("usage_collector.create_usage_record")
        .summary("Create a usage record")
        .description(
            "Accepts a usage record payload and delegates storage to the configured storage plugin.",
        )
        .tag(API_TAG)
        .authenticated()
        .require_license_features::<License>([])
        .json_request::<CreateUsageRecordRequest>(openapi, "Usage record to create")
        .allow_content_types(&["application/json"])
        .handler(handlers::handle_create_usage_record)
        .json_response(http::StatusCode::NO_CONTENT, "Record accepted and stored")
        .error_403(openapi)
        .error_422(openapi)
        .error_500(openapi)
        .register(router, openapi);

    let router = OperationBuilder::get("/usage-collector/v1/modules/{module_name}/config")
        .operation_id("usage_collector.get_module_config")
        .summary("Get module configuration")
        .description(
            "Returns the allowed metrics (and future configuration) for the specified module.",
        )
        .tag(API_TAG)
        .authenticated()
        .require_license_features::<License>([])
        .path_param("module_name", "Module name")
        .handler(handlers::handle_get_module_config)
        .json_response_with_schema::<ModuleConfigResponse>(
            openapi,
            http::StatusCode::OK,
            "Module configuration",
        )
        .error_404(openapi)
        .error_500(openapi)
        .register(router, openapi);

    let router = OperationBuilder::get("/usage-collector/v1/aggregated")
        .operation_id("usage_collector.query_aggregated")
        .summary("Query aggregated usage data")
        .description(
            "Returns aggregated usage statistics for the authenticated tenant, \
             authorized via the platform PDP.",
        )
        .tag(API_TAG)
        .authenticated()
        .require_license_features::<License>([])
        .handler(handlers::handle_query_aggregated)
        .json_response_with_schema::<Vec<AggregationResultDto>>(
            openapi,
            http::StatusCode::OK,
            "Aggregated usage data",
        )
        .error_400(openapi)
        .error_403(openapi)
        .error_500(openapi)
        .register(router, openapi);

    let router = OperationBuilder::get("/usage-collector/v1/raw")
        .operation_id("usage_collector.query_raw")
        .summary("Query raw usage records")
        .description(
            "Returns a paginated page of raw usage records for the authenticated tenant, \
             authorized via the platform PDP. Null next_cursor in page_info signals the final page.",
        )
        .tag(API_TAG)
        .authenticated()
        .require_license_features::<License>([])
        .handler(handlers::handle_query_raw)
        .json_response(http::StatusCode::OK, "Paged raw usage records")
        .error_400(openapi)
        .error_403(openapi)
        .error_500(openapi)
        .register(router, openapi);

    router
        .layer(Extension(emitter))
        .layer(Extension(collector))
        .layer(Extension(authz_client))
        .layer(Extension(plugin_client))
}
