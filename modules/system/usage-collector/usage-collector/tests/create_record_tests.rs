#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use axum::body::Body;
use chrono::Utc;
use http::{Method, Request, StatusCode};
use tower::ServiceExt;
use usage_emitter::UsageEmitterV1;
use uuid::Uuid;

use common::{AppHarness, MockUsageEmitterV1};

#[tokio::test]
async fn create_record_happy_path() {
    let emitter = Arc::new(MockUsageEmitterV1::with_allow_authz().await) as Arc<dyn UsageEmitterV1>;
    let harness = AppHarness::with_emitter(emitter);

    let body = serde_json::json!({
        "module": "test-module",
        "tenant_id": Uuid::new_v4(),
        "resource_type": "test.resource",
        "resource_id": Uuid::new_v4(),
        "metric": "test.gauge",
        "value": 1.0,
        "timestamp": Utc::now().to_rfc3339(),
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/usage-collector/v1/records")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn create_record_metadata_too_large() {
    let harness = AppHarness::new().await;

    let large_value = "a".repeat(9000);
    let body = serde_json::json!({
        "module": "test-module",
        "tenant_id": Uuid::new_v4(),
        "resource_type": "test.resource",
        "resource_id": Uuid::new_v4(),
        "metric": "test.gauge",
        "value": 1.0,
        "timestamp": Utc::now().to_rfc3339(),
        "metadata": {"key": large_value},
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/usage-collector/v1/records")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn create_record_subject_type_without_subject_id() {
    let harness = AppHarness::new().await;

    let body = serde_json::json!({
        "module": "test-module",
        "tenant_id": Uuid::new_v4(),
        "resource_type": "test.resource",
        "resource_id": Uuid::new_v4(),
        "metric": "test.gauge",
        "value": 1.0,
        "timestamp": Utc::now().to_rfc3339(),
        "subject_type": "user",
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/usage-collector/v1/records")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn create_record_emitter_authorization_failed() {
    let emitter = Arc::new(MockUsageEmitterV1::with_deny_authz().await) as Arc<dyn UsageEmitterV1>;
    let harness = AppHarness::with_emitter(emitter);

    let body = serde_json::json!({
        "module": "test-module",
        "tenant_id": Uuid::new_v4(),
        "resource_type": "test.resource",
        "resource_id": Uuid::new_v4(),
        "metric": "test.gauge",
        "value": 1.0,
        "timestamp": Utc::now().to_rfc3339(),
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/usage-collector/v1/records")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_record_invalid_subject_id_uuid() {
    let harness = AppHarness::new().await;

    let body = serde_json::json!({
        "module": "test-module",
        "tenant_id": Uuid::new_v4(),
        "resource_type": "test.resource",
        "resource_id": Uuid::new_v4(),
        "subject_id": "not-a-valid-uuid",
        "metric": "test.gauge",
        "value": 1.0,
        "timestamp": Utc::now().to_rfc3339(),
    });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/usage-collector/v1/records")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
