// @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use chrono::Utc;
use modkit_security::{AccessScope, ScopeConstraint, ScopeFilter, ScopeValue, pep_properties};
use uuid::Uuid;

use usage_collector_sdk::models::{
    AggregationFn, AggregationQuery, AggregationResult, RawQuery, UsageKind, UsageRecord,
};
use usage_collector_sdk::{Page, UsageCollectorError, UsageCollectorPluginClientV1};

use super::TimescaleDbPluginClient;
use crate::domain::error::{ScopeTranslationError, StoragePluginError};
use crate::domain::insert_port::InsertPort;
use crate::domain::metrics::PluginMetrics;
use crate::domain::query_port::QueryPort;
use crate::domain::scope::scope_to_sql;

// ── Mock: insert port ─────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum InsertBehavior {
    Success(u64),
    PoolTimeout,
}

struct MockInsertPort {
    behavior: InsertBehavior,
    captured_value: Option<Arc<Mutex<f64>>>,
}

impl MockInsertPort {
    fn success(rows: u64) -> Arc<Self> {
        Arc::new(Self {
            behavior: InsertBehavior::Success(rows),
            captured_value: None,
        })
    }

    fn pool_timeout() -> Arc<Self> {
        Arc::new(Self {
            behavior: InsertBehavior::PoolTimeout,
            captured_value: None,
        })
    }

    fn capturing(cap: Arc<Mutex<f64>>) -> Arc<Self> {
        Arc::new(Self {
            behavior: InsertBehavior::Success(1),
            captured_value: Some(cap),
        })
    }
}

#[async_trait]
impl InsertPort for MockInsertPort {
    async fn insert_usage_record(&self, record: &UsageRecord) -> Result<u64, StoragePluginError> {
        if let Some(ref cap) = self.captured_value {
            *cap.lock().unwrap() = record.value;
        }
        match self.behavior {
            InsertBehavior::Success(n) => Ok(n),
            InsertBehavior::PoolTimeout => {
                Err(StoragePluginError::Transient("pool timed out".to_owned()))
            }
        }
    }
}

// ── Mock: query port ──────────────────────────────────────────────────────────

struct MockQueryPort {
    agg_fail: bool,
    raw_fail: bool,
}

impl MockQueryPort {
    fn success() -> Arc<Self> {
        Arc::new(Self {
            agg_fail: false,
            raw_fail: false,
        })
    }
    fn agg_failing() -> Arc<Self> {
        Arc::new(Self {
            agg_fail: true,
            raw_fail: false,
        })
    }
}

#[async_trait]
impl QueryPort for MockQueryPort {
    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        if self.agg_fail {
            Err(UsageCollectorError::unavailable("mock transient"))
        } else {
            Ok(vec![])
        }
    }

    async fn query_raw(&self, _query: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        if self.raw_fail {
            Err(UsageCollectorError::unavailable("mock transient"))
        } else {
            Ok(Page::empty(10))
        }
    }
}

// ── Mock: metrics ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct MockMetrics {
    ingestion_success: AtomicU32,
    ingestion_error: AtomicU32,
    ingestion_latency_called: AtomicU32,
    query_latency_called: AtomicU32,
    dedup: AtomicU32,
    schema_validation_errors: AtomicU32,
}

impl PluginMetrics for MockMetrics {
    fn record_ingestion_success(&self) {
        self.ingestion_success.fetch_add(1, Ordering::SeqCst);
    }
    fn record_ingestion_error(&self) {
        self.ingestion_error.fetch_add(1, Ordering::SeqCst);
    }
    fn record_ingestion_latency_ms(&self, _elapsed_ms: f64) {
        self.ingestion_latency_called.fetch_add(1, Ordering::SeqCst);
    }
    fn record_dedup(&self) {
        self.dedup.fetch_add(1, Ordering::SeqCst);
    }
    fn record_schema_validation_error(&self) {
        self.schema_validation_errors.fetch_add(1, Ordering::SeqCst);
    }
    fn record_query_latency_ms(&self, _query_type: &str, _elapsed_ms: f64) {
        self.query_latency_called.fetch_add(1, Ordering::SeqCst);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_client(
    insert_port: Arc<dyn InsertPort>,
    metrics: Arc<MockMetrics>,
) -> TimescaleDbPluginClient {
    TimescaleDbPluginClient::new(insert_port, MockQueryPort::success(), metrics)
}

fn make_client_q(
    query_port: Arc<dyn QueryPort>,
    metrics: Arc<MockMetrics>,
) -> TimescaleDbPluginClient {
    TimescaleDbPluginClient::new(MockInsertPort::success(0), query_port, metrics)
}

fn base_counter_record() -> UsageRecord {
    UsageRecord {
        module: "test-module".to_owned(),
        tenant_id: Uuid::new_v4(),
        metric: "test.cpu".to_owned(),
        kind: UsageKind::Counter,
        value: 1.0,
        resource_id: Uuid::new_v4(),
        resource_type: "vm".to_owned(),
        subject_id: None,
        subject_type: None,
        idempotency_key: "idem-key-1".to_owned(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

fn base_gauge_record() -> UsageRecord {
    UsageRecord {
        kind: UsageKind::Gauge,
        idempotency_key: String::new(),
        ..base_counter_record()
    }
}

// ── create_usage_record tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_create_usage_record_valid_counter() {
    // Scenario: valid counter insert — DB mock returns 1 row affected
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(MockInsertPort::success(1), metrics.clone());

    let result = client.create_usage_record(base_counter_record()).await;

    assert!(result.is_ok());
    assert_eq!(
        metrics.ingestion_success.load(Ordering::SeqCst),
        1,
        "ingestion_success counter"
    );
    assert_eq!(
        metrics.ingestion_latency_called.load(Ordering::SeqCst),
        1,
        "latency histogram"
    );
}

#[tokio::test]
async fn test_create_usage_record_valid_gauge() {
    // Scenario: valid gauge insert — DB mock returns 1 row affected
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(MockInsertPort::success(1), metrics.clone());

    let result = client.create_usage_record(base_gauge_record()).await;

    assert!(result.is_ok());
    assert_eq!(
        metrics.ingestion_success.load(Ordering::SeqCst),
        1,
        "ingestion_success counter"
    );
}

#[tokio::test]
async fn test_create_usage_record_negative_counter_value_rejected() {
    // Scenario: counter with negative value rejected before any DB call
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(MockInsertPort::success(0), metrics.clone());
    let record = UsageRecord {
        value: -1.0,
        ..base_counter_record()
    };

    let result = client.create_usage_record(record).await;

    assert!(
        matches!(result, Err(UsageCollectorError::Internal { .. })),
        "expected Internal error for negative counter value"
    );
    assert_eq!(
        metrics.schema_validation_errors.load(Ordering::SeqCst),
        1,
        "validation error counter"
    );
    assert_eq!(
        metrics.ingestion_success.load(Ordering::SeqCst),
        0,
        "no success on validation failure"
    );
}

#[tokio::test]
async fn test_create_usage_record_missing_idempotency_key_for_counter_rejected() {
    // Scenario: counter without idempotency_key rejected before any DB call
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(MockInsertPort::success(0), metrics.clone());
    let record = UsageRecord {
        idempotency_key: String::new(),
        ..base_counter_record()
    };

    let result = client.create_usage_record(record).await;

    assert!(
        matches!(result, Err(UsageCollectorError::Internal { .. })),
        "expected Internal error for missing idempotency_key"
    );
    assert_eq!(
        metrics.schema_validation_errors.load(Ordering::SeqCst),
        1,
        "validation error counter"
    );
    assert_eq!(
        metrics.ingestion_success.load(Ordering::SeqCst),
        0,
        "no success on validation failure"
    );
}

#[tokio::test]
async fn test_create_usage_record_transient_db_error() {
    // Scenario: DB mock returns pool-timeout (transient); mapped to Unavailable
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(MockInsertPort::pool_timeout(), metrics.clone());

    let result = client.create_usage_record(base_counter_record()).await;

    assert!(
        matches!(result, Err(UsageCollectorError::Unavailable { .. })),
        "transient error must map to Unavailable"
    );
    assert_eq!(
        metrics.ingestion_error.load(Ordering::SeqCst),
        1,
        "ingestion_error counter"
    );
    assert_eq!(
        metrics.ingestion_success.load(Ordering::SeqCst),
        0,
        "no success on transient error"
    );
}

#[tokio::test]
async fn test_create_usage_record_idempotent_insert() {
    // Scenario: DB mock returns 0 rows affected (ON CONFLICT DO NOTHING); dedup recorded
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(MockInsertPort::success(0), metrics.clone());

    let result = client.create_usage_record(base_counter_record()).await;

    assert!(result.is_ok());
    assert_eq!(
        metrics.dedup.load(Ordering::SeqCst),
        1,
        "dedup counter must be incremented"
    );
    assert_eq!(
        metrics.ingestion_success.load(Ordering::SeqCst),
        0,
        "success not reported for dedup"
    );
}

#[tokio::test]
async fn test_create_usage_record_gauge_no_accumulation() {
    // Scenario: gauge value passed to insert equals submitted value — no accumulation applied
    let captured = Arc::new(Mutex::new(0.0_f64));
    let port = MockInsertPort::capturing(captured.clone());
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client(port, metrics);

    let submitted_value = 42.75_f64;
    let record = UsageRecord {
        value: submitted_value,
        ..base_gauge_record()
    };
    client.create_usage_record(record).await.unwrap();

    let stored = *captured.lock().unwrap();
    assert!(
        (stored - submitted_value).abs() < f64::EPSILON,
        "gauge value must not be accumulated or transformed before insert",
    );
}

// ── scope_to_sql tests ────────────────────────────────────────────────────────

#[test]
fn test_scope_to_sql_single_group() {
    // Scenario: AccessScope with one ConstraintGroup produces a single AND-branch WHERE fragment
    let tid = Uuid::new_v4();
    let scope = AccessScope::for_tenant(tid);

    let (sql, params) = scope_to_sql(&scope).unwrap();

    assert!(
        sql.contains("tenant_id"),
        "fragment must reference tenant_id column: {sql}"
    );
    assert_eq!(params.len(), 1, "single group yields one bind parameter");
}

#[test]
fn test_scope_to_sql_multiple_groups_or_of_ands_preserved() {
    // Scenario: two ConstraintGroups become two AND-branches joined with OR; no group flattening
    let tid1 = Uuid::new_v4();
    let tid2 = Uuid::new_v4();
    let scope = AccessScope::from_constraints(vec![
        ScopeConstraint::new(vec![ScopeFilter::in_uuids(
            pep_properties::OWNER_TENANT_ID,
            vec![tid1],
        )]),
        ScopeConstraint::new(vec![ScopeFilter::in_uuids(
            pep_properties::OWNER_TENANT_ID,
            vec![tid2],
        )]),
    ]);

    let (sql, params) = scope_to_sql(&scope).unwrap();

    assert!(sql.contains(" OR "), "groups must be joined with OR: {sql}");
    assert_eq!(params.len(), 2, "each group contributes one bind param");
    assert_eq!(
        sql.matches(" OR ").count(),
        1,
        "exactly one OR for two groups — no group flattening: {sql}"
    );
}

#[test]
fn test_scope_to_sql_empty_scope_fail_closed() {
    // Scenario: empty scope (deny_all) must fail closed, returning EmptyScope error
    let scope = AccessScope::deny_all();

    assert!(
        matches!(scope_to_sql(&scope), Err(ScopeTranslationError::EmptyScope)),
        "empty scope must return EmptyScope error - not allow-all"
    );
}

#[test]
fn test_scope_to_sql_ingroup_predicate_rejection() {
    // Scenario: InGroup predicate must return UnsupportedPredicate, not be silently ignored
    let scope = AccessScope::single(ScopeConstraint::new(vec![ScopeFilter::in_group(
        pep_properties::OWNER_TENANT_ID,
        vec![ScopeValue::Uuid(Uuid::new_v4())],
    )]));

    match scope_to_sql(&scope) {
        Err(ScopeTranslationError::UnsupportedPredicate { kind }) => {
            assert!(
                kind.contains("InGroup"),
                "kind must identify InGroup: {kind}"
            );
        }
        other => panic!("expected UnsupportedPredicate, got: {other:?}"),
    }
}

// ── query path tests ──────────────────────────────────────────────────────────

fn base_agg_query() -> AggregationQuery {
    AggregationQuery {
        scope: AccessScope::for_tenant(Uuid::new_v4()),
        time_range: (Utc::now() - chrono::Duration::hours(1), Utc::now()),
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
    }
}

fn base_raw_query() -> RawQuery {
    RawQuery {
        scope: AccessScope::for_tenant(Uuid::new_v4()),
        time_range: (Utc::now() - chrono::Duration::hours(1), Utc::now()),
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 10,
    }
}

#[tokio::test]
async fn test_query_aggregated_success_records_latency() {
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client_q(MockQueryPort::success(), metrics.clone());

    let result = client.query_aggregated(base_agg_query()).await;

    assert!(result.is_ok());
    assert_eq!(
        metrics.query_latency_called.load(Ordering::SeqCst),
        1,
        "query latency must be recorded on success"
    );
}

#[tokio::test]
async fn test_query_aggregated_error_still_records_latency() {
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client_q(MockQueryPort::agg_failing(), metrics.clone());

    let result = client.query_aggregated(base_agg_query()).await;

    assert!(result.is_err(), "agg_failing mock must propagate error");
    assert_eq!(
        metrics.query_latency_called.load(Ordering::SeqCst),
        1,
        "query latency must be recorded even on error"
    );
}

#[tokio::test]
async fn test_query_raw_success_records_latency() {
    let metrics = Arc::new(MockMetrics::default());
    let client = make_client_q(MockQueryPort::success(), metrics.clone());

    let result = client.query_raw(base_raw_query()).await;

    assert!(result.is_ok());
    assert_eq!(
        metrics.query_latency_called.load(Ordering::SeqCst),
        1,
        "query latency must be recorded on success"
    );
}
