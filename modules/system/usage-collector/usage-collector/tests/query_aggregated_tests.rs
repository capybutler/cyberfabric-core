#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use chrono::Utc;
use http::{Method, Request, StatusCode};
use tower::ServiceExt;
use usage_collector_sdk::UsageCollectorPluginClientV1;

use common::{AppHarness, MockUsageCollectorPluginClientV1, encode_dt};

/// MAX_QUERY_TIME_RANGE is 8784 hours (~1 year); use 8785h to exceed it.
const OVER_MAX_HOURS: i64 = 8785;

#[tokio::test]
async fn query_aggregated_invalid_time_range() {
    let harness = AppHarness::new().await;

    let now = Utc::now();
    let from = encode_dt(now);
    let to = encode_dt(now - chrono::Duration::hours(1));
    let uri = format!("/usage-collector/v1/aggregated?from={from}&to={to}&fn=sum");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["detail"].as_str().and_then(|d| {
        let v: serde_json::Value = serde_json::from_str(d).ok()?;
        v["code"].as_str().map(str::to_owned)
    }).as_deref(), Some("VALIDATION_ERROR"));
}

#[tokio::test]
async fn query_aggregated_time_range_too_wide() {
    let harness = AppHarness::new().await;

    let to = Utc::now();
    let from = to - chrono::Duration::hours(OVER_MAX_HOURS);
    let from_str = encode_dt(from);
    let to_str = encode_dt(to);
    let uri = format!("/usage-collector/v1/aggregated?from={from_str}&to={to_str}&fn=sum");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_aggregated_missing_bucket_size() {
    let harness = AppHarness::new().await;

    let from = encode_dt(Utc::now() - chrono::Duration::hours(1));
    let to = encode_dt(Utc::now());
    // group_by=time_bucket without bucket_size; serde deserialization of the tuple
    // variant fails, which axum surfaces as 400 Bad Request.
    let uri = format!(
        "/usage-collector/v1/aggregated?from={from}&to={to}&fn=sum&group_by=time_bucket"
    );

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_aggregated_forbidden() {
    let harness = AppHarness::with_deny_authz().await;

    let from = encode_dt(Utc::now() - chrono::Duration::hours(1));
    let to = encode_dt(Utc::now());
    let uri = format!("/usage-collector/v1/aggregated?from={from}&to={to}&fn=sum");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn query_aggregated_result_too_large() {
    let plugin: Arc<dyn UsageCollectorPluginClientV1> =
        Arc::new(MockUsageCollectorPluginClientV1::too_large());
    let harness = AppHarness::with_plugin(plugin).await;

    let from = encode_dt(Utc::now() - chrono::Duration::hours(1));
    let to = encode_dt(Utc::now());
    let uri = format!("/usage-collector/v1/aggregated?from={from}&to={to}&fn=sum");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_aggregated_happy_path() {
    let harness = AppHarness::new().await;

    let from = encode_dt(Utc::now() - chrono::Duration::hours(1));
    let to = encode_dt(Utc::now());
    let uri = format!("/usage-collector/v1/aggregated?from={from}&to={to}&fn=sum");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json.is_array(), "response must be a JSON array");
}
