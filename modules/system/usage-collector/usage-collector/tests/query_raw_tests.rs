#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use chrono::Utc;
use http::{Method, Request, StatusCode};
use modkit_odata::SortDir;
use tower::ServiceExt;
use usage_collector_sdk::{CursorV1, UsageCollectorPluginClientV1};
use uuid::Uuid;

use common::{AppHarness, MockUsageCollectorPluginClientV1, encode_dt};

/// MAX_QUERY_TIME_RANGE is 8784 hours; use 8785h to exceed it.
const OVER_MAX_HOURS: i64 = 8785;

/// MAX_PAGE_SIZE is 1000; use 1001 to exceed it.
const OVER_MAX_PAGE_SIZE: usize = 1001;

fn valid_raw_uri() -> String {
    let from = encode_dt(Utc::now() - chrono::Duration::hours(1));
    let to = encode_dt(Utc::now());
    format!("/usage-collector/v1/raw?from={from}&to={to}")
}

#[tokio::test]
async fn query_raw_invalid_time_range() {
    let harness = AppHarness::new().await;

    let now = Utc::now();
    let from = encode_dt(now);
    let to = encode_dt(now - chrono::Duration::hours(1));
    let uri = format!("/usage-collector/v1/raw?from={from}&to={to}");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_raw_time_range_too_wide() {
    let harness = AppHarness::new().await;

    let to = Utc::now();
    let from = to - chrono::Duration::hours(OVER_MAX_HOURS);
    let from_str = encode_dt(from);
    let to_str = encode_dt(to);
    let uri = format!("/usage-collector/v1/raw?from={from_str}&to={to_str}");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_raw_invalid_page_size_zero() {
    let harness = AppHarness::new().await;
    let uri = format!("{}&page_size=0", valid_raw_uri());

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_raw_page_size_exceeds_max() {
    let harness = AppHarness::new().await;
    let uri = format!("{}&page_size={OVER_MAX_PAGE_SIZE}", valid_raw_uri());

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// CursorV1 migration: TTL check removed — cursor within [from, to] now always succeeds.
#[tokio::test]
async fn query_raw_cursor_expired() {
    let harness = AppHarness::new().await;

    let now = Utc::now();
    let cursor_ts = now - chrono::Duration::hours(1);
    let from = cursor_ts - chrono::Duration::hours(1);
    let to = now;
    let cursor = CursorV1 {
        k: vec![cursor_ts.to_rfc3339(), Uuid::new_v4().to_string()],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let cursor_str = cursor.encode().expect("CursorV1 encode is infallible for valid data");

    let from_str = encode_dt(from);
    let to_str = encode_dt(to);
    let uri = format!("/usage-collector/v1/raw?from={from_str}&to={to_str}&cursor={cursor_str}");

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn query_raw_forbidden() {
    let harness = AppHarness::with_deny_authz().await;
    let uri = valid_raw_uri();

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn query_raw_happy_path() {
    let harness = AppHarness::new().await;
    let uri = valid_raw_uri();

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["items"].is_array(), "response must contain items array");
}

#[tokio::test]
async fn query_raw_pagination_next_cursor() {
    let plugin: Arc<dyn UsageCollectorPluginClientV1> =
        Arc::new(MockUsageCollectorPluginClientV1::with_raw_cursor());
    let harness = AppHarness::with_plugin(plugin).await;
    let uri = valid_raw_uri();

    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = harness.router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let cursor = json["page_info"]["next_cursor"].as_str().expect("page_info.next_cursor must be present");
    assert!(!cursor.is_empty(), "next_cursor must be a non-empty string");
}
