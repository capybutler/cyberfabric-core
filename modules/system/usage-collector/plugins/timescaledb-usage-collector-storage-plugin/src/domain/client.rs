//! TimescaleDB storage plugin client.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use sqlx::PgPool;
use usage_collector_sdk::models::{
    AggregationQuery, AggregationResult, PagedResult, RawQuery, UsageKind, UsageRecord,
};
use usage_collector_sdk::{UsageCollectorError, UsageCollectorPluginClientV1};

use crate::domain::metrics::PluginMetrics;

/// Storage plugin client backed by a TimescaleDB connection pool.
pub struct TimescaleDbPluginClient {
    pool: PgPool,
    metrics: Arc<dyn PluginMetrics>,
}

impl TimescaleDbPluginClient {
    /// Creates a new client wrapping the given connection pool and metrics port.
    pub fn new(pool: PgPool, metrics: Arc<dyn PluginMetrics>) -> Self {
        Self { pool, metrics }
    }
}

fn is_transient_error(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::PoolTimedOut => true,
        sqlx::Error::PoolClosed => true,
        sqlx::Error::Io(_) => true,
        sqlx::Error::Database(db_err) => matches!(
            db_err.code().as_deref(),
            Some("40001" | "40P01" | "57P03" | "53300" | "08006" | "08001")
        ),
        _ => false,
    }
}

#[async_trait]
impl UsageCollectorPluginClientV1 for TimescaleDbPluginClient {
    // @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1
    // @cpt-flow:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        let start = Instant::now();

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-1
        // Plugin entry point; called by the gateway when delegating record storage.
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-1

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-2
        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-1
        if record.kind == UsageKind::Counter && record.value < 0.0 {
            self.metrics.record_schema_validation_error();
            return Err(UsageCollectorError::internal(
                "invalid record: counter value must be >= 0",
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-1

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-2
        if record.kind == UsageKind::Counter && record.idempotency_key.is_empty() {
            self.metrics.record_schema_validation_error();
            return Err(UsageCollectorError::internal(
                "invalid record: idempotency_key required for counter records",
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-2
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-2

        let kind_str = match record.kind {
            UsageKind::Counter => "counter",
            UsageKind::Gauge => "gauge",
        };

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-6
        // ingested_at is set via NOW() in the INSERT SQL — not populated from the caller
        let insert_sql = "INSERT INTO usage_records (
                tenant_id, module, kind, metric, value, timestamp, idempotency_key,
                resource_id, resource_type, subject_id, subject_type, metadata, ingested_at
            )
            VALUES (
                $1, $2, $3, $4, $5::numeric, $6, NULLIF($7, ''),
                $8, $9, $10, $11, $12::jsonb, NOW()
            )
            ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL
            DO NOTHING";
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-6

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-3
        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-3
        let metadata_json = record
            .metadata
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        let result = sqlx::query(insert_sql)
            .bind(record.tenant_id)
            .bind(&record.module)
            .bind(kind_str)
            .bind(&record.metric)
            .bind(record.value)
            .bind(record.timestamp)
            .bind(&record.idempotency_key)
            .bind(record.resource_id)
            .bind(&record.resource_type)
            .bind(record.subject_id)
            .bind(&record.subject_type)
            .bind(metadata_json.as_deref())
            .execute(&self.pool)
            .await;
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-3
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-3

        let pg_result = match result {
            Ok(r) => r,
            Err(e) => {
                // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-4
                if let sqlx::Error::Database(ref db_err) = e {
                    if db_err.code().as_deref() == Some("23505") {
                        return Err(UsageCollectorError::internal(format!(
                            "unexpected unique constraint violation: {db_err}"
                        )));
                    }
                }
                // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-4

                // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-4
                // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-5
                if is_transient_error(&e) {
                    return Err(UsageCollectorError::unavailable(format!(
                        "transient error: {e}"
                    )));
                }
                // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-5
                // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-4

                return Err(UsageCollectorError::internal(format!("storage error: {e}")));
            }
        };

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        if pg_result.rows_affected() == 0 {
            self.metrics.record_dedup();
        }
        self.metrics.record_ingestion_latency_ms(elapsed_ms);
        self.metrics.record_ingestion_success();

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-5
        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-7
        Ok(())
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-7
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-5
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        todo!("Phase 6: query_aggregated — aggregation query with routing")
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        todo!("Phase 7: query_raw — cursor-based raw record pagination")
    }
}
