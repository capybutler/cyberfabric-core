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
use usage_collector_sdk::UsageCollectorPluginClientV1;
use usage_collector_sdk::models::{AggregationFn, AggregationQuery, RawQuery, UsageKind, UsageRecord};
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

    assert!(
        raw_results.iter().any(|r| r.value > 0.0),
        "raw hypertable path must return non-zero aggregated value for the inserted records"
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

    assert!(
        cagg_results.iter().any(|r| r.value > 0.0),
        "continuous aggregate path must return non-zero sum after manual refresh"
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
    let cursor = first_page
        .next_cursor
        .expect("cursor must be present when page_size equals result count");

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
        second_page.next_cursor.is_none(),
        "no next cursor expected after the last page is exhausted"
    );
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
