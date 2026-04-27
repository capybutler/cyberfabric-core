//! Route registration for the usage-collector REST API.

use std::sync::Arc;

use axum::{Extension, Router};

use modkit::api::operation_builder::LicenseFeature;
use modkit::api::{OpenApiRegistry, OperationBuilder};
use usage_collector_sdk::UsageCollectorClientV1;
use usage_emitter::UsageEmitterV1;

use super::dto::{CreateUsageRecordRequest, ModuleConfigResponse};
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
        .json_response(
            http::StatusCode::OK,
            "Record accepted and stored",
        )
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

    router.layer(Extension(emitter)).layer(Extension(collector))
}
