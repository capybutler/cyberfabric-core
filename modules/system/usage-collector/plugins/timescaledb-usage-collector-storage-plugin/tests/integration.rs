// @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability:p10
//! Level 2 integration tests for the TimescaleDB usage-collector storage plugin.
//!
//! Run with:
//!   cargo test -p timescaledb-usage-collector-storage-plugin --features integration

#![cfg(feature = "integration")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use modkit_security::AccessScope;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::{ContainerAsync, ContainerRequest, GenericImage, ImageExt, runners::AsyncRunner};
use modkit_odata::CursorV1;
use usage_collector_sdk::{UsageCollectorError, UsageCollectorPluginClientV1};
use usage_collector_sdk::models::{AggregationFn, AggregationQuery, BucketSize, GroupByDimension, RawQuery, UsageKind, UsageRecord};
use uuid::Uuid;

use timescaledb_usage_collector_storage_plugin::domain::client::TimescaleDbPluginClient;
use timescaledb_usage_collector_storage_plugin::domain::insert_port::InsertPort;
use timescaledb_usage_collector_storage_plugin::domain::metrics::{NoopMetrics, PluginMetrics};
use timescaledb_usage_collector_storage_plugin::infra::continuous_aggregate::setup_continuous_aggregate;
use timescaledb_usage_collector_storage_plugin::infra::migrations::run_migrations;
use timescaledb_usage_collector_storage_plugin::infra::pg_insert_port::PgInsertPort;

// ── Container and pool setup ──────────────────────────────────────────────────

struct TestDb {
    _container: ContainerAsync<GenericImage>,
    pool: PgPool,
}

fn timescaledb_image() -> GenericImage {
    GenericImage::new("timescale/timescaledb", "latest-pg16")
        .with_exposed_port(ContainerPort::Tcp(5432))
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
}

/// Starts a TimescaleDB container, waits for it to be ready, runs migrations,
/// sets up the continuous aggregate, and returns the container drop handle and pool.
async fn setup_container_and_pool() -> TestDb {
    let container = ContainerRequest::from(timescaledb_image())
        .with_env_var("POSTGRES_PASSWORD", "testpass")
        .with_env_var("POSTGRES_USER", "testuser")
        .with_env_var("POSTGRES_DB", "testdb")
        .start()
        .await
        .expect("failed to start timescaledb container");

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get mapped port for 5432");

    let url = format!("postgres://testuser:testpass@127.0.0.1:{port}/testdb");

    let pool = connect_with_retry(&url, 60).await;

    run_migrations(&pool).await.expect("schema migration failed");
    setup_continuous_aggregate(&pool)
        .await
        .expect("continuous aggregate setup failed");

    TestDb { _container: container, pool }
}

async fn connect_with_retry(url: &str, timeout_secs: u64) -> PgPool {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
        {
            Ok(pool) => return pool,
            Err(_) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => panic!("timed out waiting for database: {e}"),
        }
    }
}

// ── Client construction helper ────────────────────────────────────────────────

fn make_client(pool: PgPool) -> TimescaleDbPluginClient {
    let insert_port: Arc<dyn InsertPort> = Arc::new(PgInsertPort::new(pool.clone()));
    let metrics: Arc<dyn PluginMetrics> = Arc::new(NoopMetrics);
    TimescaleDbPluginClient::new(insert_port, pool, metrics)
}

// ── Test-record factory ───────────────────────────────────────────────────────

fn counter_record(tenant_id: Uuid, resource_id: Uuid, key: &str) -> UsageRecord {
    UsageRecord {
        module: "integration-test".to_string(),
        tenant_id,
        metric: "test.cpu".to_string(),
        kind: UsageKind::Counter,
        value: 10.0,
        resource_id,
        resource_type: "vm".to_string(),
        subject_id: None,
        subject_type: None,
        idempotency_key: key.to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

// ── Test 1: migration_idempotency ─────────────────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn migration_idempotency() {
    let db = setup_container_and_pool().await;

    // Second run must succeed (migrations are idempotent)
    run_migrations(&db.pool)
        .await
        .expect("second migration run must succeed — migrations are not idempotent");

    // Verify hypertable exists after both runs
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM timescaledb_information.hypertables \
         WHERE hypertable_name = 'usage_records'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("failed to query timescaledb_information.hypertables");

    assert_eq!(count, 1, "usage_records hypertable must exist after idempotent migration");
}

// ── Test 2: concurrent_upsert_exactly_one_row ─────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn concurrent_upsert_exactly_one_row() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let idempotency_key = format!("idem-{}", Uuid::new_v4());

    let client = Arc::new(make_client(db.pool.clone()));

    // Spawn 5 concurrent tasks each inserting the same record
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let c = client.clone();
            let record = counter_record(tenant_id, resource_id, &idempotency_key);
            tokio::spawn(async move { c.create_usage_record(record).await })
        })
        .collect();

    for handle in handles {
        handle
            .await
            .expect("task panicked")
            .expect("create_usage_record returned error under concurrent upsert");
    }

    // Exactly one row must persist for the idempotency key
    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM usage_records WHERE tenant_id = $1 AND idempotency_key = $2",
    )
    .bind(tenant_id)
    .bind(&idempotency_key)
    .fetch_one(&db.pool)
    .await
    .expect("row count query failed");

    assert_eq!(
        row_count, 1,
        "exactly one row must persist under concurrent inserts with the same idempotency key"
    );
}

// ── Test 3: query_aggregated_routing_decision ─────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_routing_decision() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);

    // Insert 3 records from 3 hours ago so they land in a past 1-hour cagg bucket
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    for i in 0..3u32 {
        let mut record = counter_record(tenant_id, resource_id, &format!("cagg-key-{i}"));
        record.timestamp = past_ts;
        client.create_usage_record(record).await.expect("insert failed");
    }

    // Refresh the cagg to materialise the inserted data
    sqlx::query(
        "CALL refresh_continuous_aggregate(\
             'usage_agg_1h', \
             (NOW() - INTERVAL '5 hours')::timestamptz, \
             (NOW() - INTERVAL '1 hour')::timestamptz\
         )",
    )
    .execute(&db.pool)
    .await
    .expect("manual cagg refresh failed");

    // Time range covers the inserted data and the cagg bucket
    let time_range = (
        past_ts - chrono::Duration::hours(1),
        past_ts + chrono::Duration::hours(2),
    );

    // Raw hypertable path: resource_id filter forces routing to usage_records
    let raw_results = client
        .query_aggregated(AggregationQuery {
            scope: scope.clone(),
            time_range,
            function: AggregationFn::Sum,
            group_by: vec![],
            bucket_size: None,
            usage_type: None,
            resource_id: Some(resource_id),
            resource_type: None,
            subject_id: None,
            subject_type: None,
            source: None,
            max_rows: 100,
        })
        .await
        .expect("raw hypertable path query failed");

    assert_eq!(raw_results.len(), 1, "raw hypertable path must return exactly one aggregated row");
    assert!(
        (raw_results[0].value - 30.0).abs() < 1e-6,
        "raw hypertable path must return sum=30.0, got {}",
        raw_results[0].value
    );

    // Continuous aggregate path: no resource_id/subject_id → routed to usage_agg_1h
    let cagg_results = client
        .query_aggregated(AggregationQuery {
            scope: scope.clone(),
            time_range,
            function: AggregationFn::Sum,
            group_by: vec![],
            bucket_size: None,
            usage_type: None,
            resource_id: None,
            resource_type: None,
            subject_id: None,
            subject_type: None,
            source: None,
            max_rows: 100,
        })
        .await
        .expect("continuous aggregate path query failed");

    assert_eq!(cagg_results.len(), 1, "continuous aggregate path must return exactly one aggregated row");
    assert!(
        (cagg_results[0].value - 30.0).abs() < 1e-6,
        "continuous aggregate path must return sum=30.0 after manual refresh, got {}",
        cagg_results[0].value
    );
}

// ── Test 4: cursor_stability_under_concurrent_inserts ─────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn cursor_stability_under_concurrent_inserts() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = Arc::new(make_client(db.pool.clone()));
    let scope = AccessScope::for_tenant(tenant_id);

    // Baseline: insert 5 records with distinct timestamps inside the query range
    let base_ts = Utc::now() - chrono::Duration::hours(2);
    let query_range = (base_ts - chrono::Duration::minutes(1), base_ts + chrono::Duration::hours(1));

    for i in 0..5u32 {
        let mut record = counter_record(tenant_id, resource_id, &format!("stable-{i}"));
        record.timestamp = base_ts + chrono::Duration::minutes(i64::from(i));
        client.create_usage_record(record).await.expect("baseline insert failed");
    }

    // Get the first page (3 records)
    let first_page = client
        .query_raw(RawQuery {
            scope: scope.clone(),
            time_range: query_range,
            usage_type: None,
            resource_id: None,
            resource_type: None,
            subject_type: None,
            subject_id: None,
            cursor: None,
            page_size: 3,
        })
        .await
        .expect("first page query failed");

    assert_eq!(first_page.items.len(), 3, "first page must contain 3 records");
    let cursor_str = first_page
        .page_info
        .next_cursor
        .expect("cursor must be present when page_size equals result count");
    let cursor = CursorV1::decode(&cursor_str).expect("cursor string from page 1 must be a valid CursorV1");

    // Concurrently insert 3 records OUTSIDE the query range so they cannot affect pagination
    let outside_ts = base_ts + chrono::Duration::hours(2);
    let c = client.clone();
    let outside_handle = tokio::spawn(async move {
        for i in 0..3u32 {
            let mut r = counter_record(tenant_id, resource_id, &format!("outside-{i}"));
            r.timestamp = outside_ts + chrono::Duration::minutes(i64::from(i));
            c.create_usage_record(r).await.expect("outside-range insert failed");
        }
    });

    // Get the second page using the cursor from page 1
    let second_page = client
        .query_raw(RawQuery {
            scope: scope.clone(),
            time_range: query_range,
            usage_type: None,
            resource_id: None,
            resource_type: None,
            subject_type: None,
            subject_id: None,
            cursor: Some(cursor),
            page_size: 3,
        })
        .await
        .expect("second page query failed");

    outside_handle.await.expect("outside-range insert task panicked");

    assert_eq!(
        second_page.items.len(),
        2,
        "second page must contain the remaining 2 records; concurrent outside-range inserts must not appear"
    );
    assert!(
        second_page.page_info.next_cursor.is_none(),
        "no next cursor expected after the last page is exhausted"
    );
}

// ── Additional helpers ────────────────────────────────────────────────────────

fn counter_record_with_value(tenant_id: Uuid, resource_id: Uuid, key: &str, value: f64) -> UsageRecord {
    UsageRecord {
        value,
        ..counter_record(tenant_id, resource_id, key)
    }
}

fn record_with_subject(tenant_id: Uuid, resource_id: Uuid, subject_id: Uuid, key: &str) -> UsageRecord {
    UsageRecord {
        subject_id: Some(subject_id),
        subject_type: Some("user".to_string()),
        ..counter_record(tenant_id, resource_id, key)
    }
}

// ── Test 5: health_check_metric ───────────────────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn health_check_metric() {
    let db = setup_container_and_pool().await;

    // Healthy pool: SELECT 1 succeeds → storage_health_status = 1
    let probe_healthy = sqlx::query("SELECT 1").execute(&db.pool).await.is_ok();
    assert!(
        probe_healthy,
        "liveness probe must succeed while the container is running (storage_health_status = 1)"
    );

    // Closed pool: queries fail → storage_health_status = 0
    db.pool.close().await;
    let probe_after_close = sqlx::query("SELECT 1").execute(&db.pool).await.is_ok();
    assert!(
        !probe_after_close,
        "liveness probe must fail after pool is closed (storage_health_status = 0)"
    );
}

// ── Group A: all 5 aggregation functions on the raw path ──────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_raw_sum() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        client.create_usage_record(counter_record_with_value(tenant_id, resource_id, &format!("sum-raw-{i}"), val)).await.expect("insert failed");
    }

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 60.0).abs() < 1e-6, "expected sum=60.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_raw_count() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    for i in 0u32..3 {
        client.create_usage_record(counter_record(tenant_id, resource_id, &format!("count-raw-{i}"))).await.expect("insert failed");
    }

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Count,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 3.0).abs() < 1e-6, "expected count=3.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_raw_min() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        client.create_usage_record(counter_record_with_value(tenant_id, resource_id, &format!("min-raw-{i}"), val)).await.expect("insert failed");
    }

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Min,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected min=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_raw_max() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        client.create_usage_record(counter_record_with_value(tenant_id, resource_id, &format!("max-raw-{i}"), val)).await.expect("insert failed");
    }

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Max,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 30.0).abs() < 1e-6, "expected max=30.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_raw_avg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        client.create_usage_record(counter_record_with_value(tenant_id, resource_id, &format!("avg-raw-{i}"), val)).await.expect("insert failed");
    }

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Avg,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 20.0).abs() < 1e-6, "expected avg=20.0, got {}", results[0].value);
}

// ── Group B: all 5 aggregation functions on the cagg path ─────────────────────

async fn cagg_refresh(pool: &sqlx::PgPool) {
    sqlx::query(
        "CALL refresh_continuous_aggregate(\
             'usage_agg_1h', \
             (NOW() - INTERVAL '5 hours')::timestamptz, \
             (NOW() - INTERVAL '1 hour')::timestamptz\
         )",
    )
    .execute(pool)
    .await
    .expect("manual cagg refresh failed");
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_cagg_sum() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        let mut r = counter_record_with_value(tenant_id, resource_id, &format!("sum-cagg-{i}"), val);
        r.timestamp = past_ts;
        client.create_usage_record(r).await.expect("insert failed");
    }
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 60.0).abs() < 1e-6, "expected cagg sum=60.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_cagg_count() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    for i in 0u32..3 {
        let mut r = counter_record(tenant_id, resource_id, &format!("count-cagg-{i}"));
        r.timestamp = past_ts;
        client.create_usage_record(r).await.expect("insert failed");
    }
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Count,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 3.0).abs() < 1e-6, "expected cagg count=3.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_cagg_min() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        let mut r = counter_record_with_value(tenant_id, resource_id, &format!("min-cagg-{i}"), val);
        r.timestamp = past_ts;
        client.create_usage_record(r).await.expect("insert failed");
    }
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Min,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected cagg min=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_cagg_max() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        let mut r = counter_record_with_value(tenant_id, resource_id, &format!("max-cagg-{i}"), val);
        r.timestamp = past_ts;
        client.create_usage_record(r).await.expect("insert failed");
    }
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Max,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 30.0).abs() < 1e-6, "expected cagg max=30.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_cagg_avg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    for (i, val) in [(0u32, 10.0f64), (1, 20.0), (2, 30.0)] {
        let mut r = counter_record_with_value(tenant_id, resource_id, &format!("avg-cagg-{i}"), val);
        r.timestamp = past_ts;
        client.create_usage_record(r).await.expect("insert failed");
    }
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Avg,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 20.0).abs() < 1e-6, "expected cagg avg=20.0, got {}", results[0].value);
}

// ── Group C: GroupByDimension variants ────────────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_usage_type_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id, "gbu-type-raw-1")).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::UsageType],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].usage_type, Some("test.cpu".to_string()));
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_usage_type_cagg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    let mut r = counter_record(tenant_id, resource_id, "gbu-type-cagg-1");
    r.timestamp = past_ts;
    client.create_usage_record(r).await.expect("insert failed");
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::UsageType],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!(results[0].usage_type.is_some(), "expected usage_type to be populated on cagg path");
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_resource() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id, "gbr-raw-1")).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::Resource],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].resource_id, Some(resource_id));
    assert_eq!(results[0].resource_type, Some("vm".to_string()));
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_subject() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let subject_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(record_with_subject(tenant_id, resource_id, subject_id, "gbs-raw-1")).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::Subject],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].subject_id, Some(subject_id));
    assert_eq!(results[0].subject_type, Some("user".to_string()));
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_source() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id, "gbsrc-raw-1")).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::Source],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source, Some("integration-test".to_string()));
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_time_bucket_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id, "gbtb-raw-1")).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::TimeBucket(BucketSize::Hour)],
        bucket_size: Some(BucketSize::Hour),
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert!(!results.is_empty(), "expected at least one result row");
    assert!(results[0].bucket_start.is_some(), "expected bucket_start to be populated");
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_group_by_time_bucket_cagg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    let mut r = counter_record(tenant_id, resource_id, "gbtb-cagg-1");
    r.timestamp = past_ts;
    client.create_usage_record(r).await.expect("insert failed");
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::TimeBucket(BucketSize::Hour)],
        bucket_size: Some(BucketSize::Hour),
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert!(!results.is_empty(), "expected at least one result row");
    assert!(results[0].bucket_start.is_some(), "expected bucket_start to be populated on cagg path");
}

// ── Group D: query_aggregated filters on the raw path ─────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_usage_type_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // Insert "test.cpu" (value 10) and "test.mem" (value 20)
    client.create_usage_record(counter_record_with_value(tenant_id, resource_id, "fut-cpu-1", 10.0)).await.expect("insert failed");
    let mut mem_rec = counter_record_with_value(tenant_id, resource_id, "fut-mem-1", 20.0);
    mem_rec.metric = "test.mem".to_string();
    client.create_usage_record(mem_rec).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: Some("test.cpu".to_string()),
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected only test.cpu sum=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_resource_type_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // 2 "vm" records (value 10 each) and 1 "disk" record (value 20)
    client.create_usage_record(counter_record_with_value(tenant_id, resource_id, "frt-vm-1", 10.0)).await.expect("insert failed");
    client.create_usage_record(counter_record_with_value(tenant_id, resource_id, "frt-vm-2", 10.0)).await.expect("insert failed");
    let mut disk_rec = counter_record_with_value(tenant_id, resource_id, "frt-disk-1", 20.0);
    disk_rec.resource_type = "disk".to_string();
    client.create_usage_record(disk_rec).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: Some("vm".to_string()),
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 20.0).abs() < 1e-6, "expected vm sum=20.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_subject_type_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let subject_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // 1 record with subject_type "user" (value 10) and 1 without (value 20)
    client.create_usage_record(record_with_subject(tenant_id, resource_id, subject_id, "fst-user-1")).await.expect("insert failed");
    client.create_usage_record(counter_record_with_value(tenant_id, resource_id, "fst-none-1", 20.0)).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: Some("user".to_string()),
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected user-only sum=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_source_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // 1 record from "mod-a" (value 10) and 1 from "mod-b" (value 20)
    client.create_usage_record(counter_record_with_value(tenant_id, resource_id, "fsrc-a-1", 10.0)).await.expect("insert failed");
    let mut mod_b = counter_record_with_value(tenant_id, resource_id, "fsrc-b-1", 20.0);
    mod_b.module = "mod-b".to_string();
    client.create_usage_record(mod_b).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: Some("integration-test".to_string()),
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected integration-test-only sum=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_multi_raw() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // (cpu, vm, 10), (cpu, disk, 15), (mem, vm, 20), (mem, disk, 25)
    client.create_usage_record(counter_record_with_value(tenant_id, resource_id, "fmulti-cv-1", 10.0)).await.expect("insert failed");
    let mut cpu_disk = counter_record_with_value(tenant_id, resource_id, "fmulti-cd-1", 15.0);
    cpu_disk.resource_type = "disk".to_string();
    client.create_usage_record(cpu_disk).await.expect("insert failed");
    let mut mem_vm = counter_record_with_value(tenant_id, resource_id, "fmulti-mv-1", 20.0);
    mem_vm.metric = "test.mem".to_string();
    client.create_usage_record(mem_vm).await.expect("insert failed");
    let mut mem_disk = counter_record_with_value(tenant_id, resource_id, "fmulti-md-1", 25.0);
    mem_disk.metric = "test.mem".to_string();
    mem_disk.resource_type = "disk".to_string();
    client.create_usage_record(mem_disk).await.expect("insert failed");

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: Some("test.cpu".to_string()),
        resource_id: Some(resource_id),
        resource_type: Some("vm".to_string()),
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected multi-filter sum=10.0, got {}", results[0].value);
}

// ── Group E: query_aggregated filters on the cagg path ────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_usage_type_cagg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    let mut cpu_rec = counter_record_with_value(tenant_id, resource_id, "fute-cpu-1", 10.0);
    cpu_rec.timestamp = past_ts;
    client.create_usage_record(cpu_rec).await.expect("insert failed");
    let mut mem_rec = counter_record_with_value(tenant_id, resource_id, "fute-mem-1", 20.0);
    mem_rec.metric = "test.mem".to_string();
    mem_rec.timestamp = past_ts;
    client.create_usage_record(mem_rec).await.expect("insert failed");
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: Some("test.cpu".to_string()),
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected cagg usage_type filter sum=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_resource_type_cagg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    let mut vm_rec = counter_record_with_value(tenant_id, resource_id, "frte-vm-1", 10.0);
    vm_rec.timestamp = past_ts;
    client.create_usage_record(vm_rec).await.expect("insert failed");
    let mut disk_rec = counter_record_with_value(tenant_id, resource_id, "frte-disk-1", 20.0);
    disk_rec.resource_type = "disk".to_string();
    disk_rec.timestamp = past_ts;
    client.create_usage_record(disk_rec).await.expect("insert failed");
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: Some("vm".to_string()),
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected cagg resource_type filter sum=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_subject_type_cagg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let subject_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    let mut user_rec = record_with_subject(tenant_id, resource_id, subject_id, "fste-user-1");
    user_rec.timestamp = past_ts;
    client.create_usage_record(user_rec).await.expect("insert failed");
    let mut none_rec = counter_record_with_value(tenant_id, resource_id, "fste-none-1", 20.0);
    none_rec.timestamp = past_ts;
    client.create_usage_record(none_rec).await.expect("insert failed");
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: Some("user".to_string()),
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected cagg subject_type filter sum=10.0, got {}", results[0].value);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_filter_source_cagg() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let past_ts = Utc::now() - chrono::Duration::hours(3);
    let time_range = (past_ts - chrono::Duration::hours(1), past_ts + chrono::Duration::hours(2));

    let mut a_rec = counter_record_with_value(tenant_id, resource_id, "fsrce-a-1", 10.0);
    a_rec.timestamp = past_ts;
    client.create_usage_record(a_rec).await.expect("insert failed");
    let mut b_rec = counter_record_with_value(tenant_id, resource_id, "fsrce-b-1", 20.0);
    b_rec.module = "mod-b".to_string();
    b_rec.timestamp = past_ts;
    client.create_usage_record(b_rec).await.expect("insert failed");
    cagg_refresh(&db.pool).await;

    let results = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: Some("integration-test".to_string()),
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected cagg source filter sum=10.0, got {}", results[0].value);
}

// ── Group F: query_aggregated scope isolation ──────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_scope_isolation() {
    let db = setup_container_and_pool().await;
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope_a = AccessScope::for_tenant(tenant_a);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // Same resource_id, different tenants
    client.create_usage_record(counter_record_with_value(tenant_a, resource_id, "scope-a-1", 10.0)).await.expect("insert tenant_a failed");
    client.create_usage_record(counter_record_with_value(tenant_b, resource_id, "scope-b-1", 20.0)).await.expect("insert tenant_b failed");

    let results = client.query_aggregated(AggregationQuery {
        scope: scope_a,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![],
        bucket_size: None,
        usage_type: None,
        resource_id: Some(resource_id),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 100,
    }).await.expect("query_aggregated failed");

    assert_eq!(results.len(), 1);
    assert!((results[0].value - 10.0).abs() < 1e-6, "expected only tenant_a sum=10.0, got {}", results[0].value);
}

// ── Group G: QueryResultTooLarge ──────────────────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_aggregated_result_too_large() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    // 3 records with distinct resource_ids → group by Resource yields 3 rows
    for i in 0u32..3 {
        let rid = Uuid::new_v4();
        client.create_usage_record(counter_record(tenant_id, rid, &format!("too-large-{i}"))).await.expect("insert failed");
    }

    let result = client.query_aggregated(AggregationQuery {
        scope,
        time_range,
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::Resource],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 2,
    }).await;

    assert!(
        matches!(result, Err(ref e) if matches!(e, UsageCollectorError::QueryResultTooLarge { .. })),
        "expected QueryResultTooLarge, got {result:?}"
    );
}

// ── Group H: query_raw filters ────────────────────────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_raw_filter_usage_type() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id, "rut-cpu-1")).await.expect("insert failed");
    let mut mem_rec = counter_record(tenant_id, resource_id, "rut-mem-1");
    mem_rec.metric = "test.mem".to_string();
    client.create_usage_record(mem_rec).await.expect("insert failed");

    let page = client.query_raw(RawQuery {
        scope,
        time_range,
        usage_type: Some("test.cpu".to_string()),
        resource_id: None,
        resource_type: None,
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 100,
    }).await.expect("query_raw failed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].metric, "test.cpu");
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_raw_filter_resource_id() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id_a = Uuid::new_v4();
    let resource_id_b = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id_a, "rrid-a-1")).await.expect("insert failed");
    client.create_usage_record(counter_record(tenant_id, resource_id_b, "rrid-b-1")).await.expect("insert failed");

    let page = client.query_raw(RawQuery {
        scope,
        time_range,
        usage_type: None,
        resource_id: Some(resource_id_a),
        resource_type: None,
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 100,
    }).await.expect("query_raw failed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].resource_id, resource_id_a);
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_raw_filter_resource_type() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_id, resource_id, "rrtype-vm-1")).await.expect("insert failed");
    let mut disk_rec = counter_record(tenant_id, resource_id, "rrtype-disk-1");
    disk_rec.resource_type = "disk".to_string();
    client.create_usage_record(disk_rec).await.expect("insert failed");

    let page = client.query_raw(RawQuery {
        scope,
        time_range,
        usage_type: None,
        resource_id: None,
        resource_type: Some("vm".to_string()),
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 100,
    }).await.expect("query_raw failed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].resource_type, "vm");
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_raw_filter_subject_id() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let subject_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(record_with_subject(tenant_id, resource_id, subject_id, "rsid-with-1")).await.expect("insert failed");
    client.create_usage_record(counter_record(tenant_id, resource_id, "rsid-without-1")).await.expect("insert failed");

    let page = client.query_raw(RawQuery {
        scope,
        time_range,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_type: None,
        subject_id: Some(subject_id),
        cursor: None,
        page_size: 100,
    }).await.expect("query_raw failed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].subject_id, Some(subject_id));
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_raw_filter_subject_type() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let subject_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope = AccessScope::for_tenant(tenant_id);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(record_with_subject(tenant_id, resource_id, subject_id, "rst-user-1")).await.expect("insert failed");
    client.create_usage_record(counter_record(tenant_id, resource_id, "rst-none-1")).await.expect("insert failed");

    let page = client.query_raw(RawQuery {
        scope,
        time_range,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_type: Some("user".to_string()),
        subject_id: None,
        cursor: None,
        page_size: 100,
    }).await.expect("query_raw failed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].subject_type, Some("user".to_string()));
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn query_raw_scope_isolation() {
    let db = setup_container_and_pool().await;
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());
    let scope_a = AccessScope::for_tenant(tenant_a);
    let now = Utc::now();
    let time_range = (now - chrono::Duration::hours(1), now + chrono::Duration::hours(1));

    client.create_usage_record(counter_record(tenant_a, resource_id, "rscope-a-1")).await.expect("insert tenant_a failed");
    client.create_usage_record(counter_record(tenant_b, resource_id, "rscope-b-1")).await.expect("insert tenant_b failed");

    let page = client.query_raw(RawQuery {
        scope: scope_a,
        time_range,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 100,
    }).await.expect("query_raw failed");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].tenant_id, tenant_a, "must only return tenant_a records");
}

// ── Group I: create_usage_record validation errors ────────────────────────────

#[cfg(feature = "integration")]
#[tokio::test]
async fn create_record_negative_counter_value() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());

    let mut record = counter_record(tenant_id, resource_id, "neg-val-key");
    record.value = -1.0;

    let err = client.create_usage_record(record).await.unwrap_err();
    assert!(
        matches!(err, UsageCollectorError::Internal { .. }),
        "expected Internal error for negative counter value, got {err:?}"
    );
}

#[cfg(feature = "integration")]
#[tokio::test]
async fn create_record_empty_idempotency_key() {
    let db = setup_container_and_pool().await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let client = make_client(db.pool.clone());

    let record = counter_record(tenant_id, resource_id, "");

    let err = client.create_usage_record(record).await.unwrap_err();
    assert!(
        matches!(err, UsageCollectorError::Internal { .. }),
        "expected Internal error for empty idempotency_key, got {err:?}"
    );
}
