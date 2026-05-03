#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use http::{Method, Request, StatusCode};
use tower::ServiceExt;
use usage_collector_sdk::{AllowedMetric, ModuleConfig, UsageCollectorClientV1, UsageKind};

use common::{AppHarness, MockUsageCollectorClientV1, NotFoundCollector};

#[tokio::test]
async fn module_config_found() {
    let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(MockUsageCollectorClientV1 {
        config: ModuleConfig {
            allowed_metrics: vec![AllowedMetric {
                name: "cpu.usage".to_owned(),
                kind: UsageKind::Gauge,
            }],
        },
    });
    let harness = AppHarness::with_collector(collector).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/usage-collector/v1/modules/my-module/config")
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let metrics = json["allowed_metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0]["name"], "cpu.usage");
}

#[tokio::test]
async fn module_config_not_found() {
    let collector: Arc<dyn UsageCollectorClientV1> = Arc::new(NotFoundCollector);
    let harness = AppHarness::with_collector(collector).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/usage-collector/v1/modules/unknown-module/config")
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
