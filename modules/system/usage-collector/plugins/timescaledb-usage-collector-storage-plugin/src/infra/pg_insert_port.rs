//! `PostgreSQL` implementation of the insert port.

use async_trait::async_trait;
use sqlx::PgPool;
use usage_collector_sdk::models::{UsageKind, UsageRecord};

use crate::domain::insert_port::InsertPort;

pub struct PgInsertPort {
    pool: PgPool,
}

impl PgInsertPort {
    #[must_use] 
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const INSERT_RECORD_SQL: &str = "INSERT INTO usage_records (
        tenant_id, module, kind, metric, value, timestamp, idempotency_key,
        resource_id, resource_type, subject_id, subject_type, metadata, ingested_at
    )
    VALUES (
        $1, $2, $3, $4, $5::numeric, $6, NULLIF($7, ''),
        $8, $9, $10, $11, $12::jsonb, NOW()
    )";

#[async_trait]
impl InsertPort for PgInsertPort {
    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-6
    // ingested_at is set via NOW() in the INSERT SQL — not populated from the caller
    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-3
    async fn insert_usage_record(&self, record: &UsageRecord) -> Result<u64, sqlx::Error> {
        let kind_str = match record.kind {
            UsageKind::Counter => "counter",
            UsageKind::Gauge => "gauge",
        };
        let metadata_json = record.metadata.as_ref().map(std::string::ToString::to_string);

        // TimescaleDB unique indexes must include the partition column (timestamp), so
        // cross-partition idempotency is enforced via a separate plain table inside a
        // transaction instead of an ON CONFLICT clause on usage_records.
        if record.kind == UsageKind::Counter && !record.idempotency_key.is_empty() {
            let mut tx = self.pool.begin().await?;

            let claimed = sqlx::query(
                "INSERT INTO usage_idempotency_keys (tenant_id, idempotency_key) \
                 VALUES ($1, $2) \
                 ON CONFLICT (tenant_id, idempotency_key) DO NOTHING",
            )
            .bind(record.tenant_id)
            .bind(&record.idempotency_key)
            .execute(&mut *tx)
            .await?
            .rows_affected();

            if claimed == 0 {
                tx.rollback().await.ok();
                return Ok(0);
            }

            let rows = sqlx::query(INSERT_RECORD_SQL)
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
                .execute(&mut *tx)
                .await?
                .rows_affected();

            tx.commit().await?;
            return Ok(rows);
        }

        let result = sqlx::query(INSERT_RECORD_SQL)
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
            .await?;

        Ok(result.rows_affected())
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-3
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-6
}
