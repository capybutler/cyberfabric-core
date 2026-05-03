//! PostgreSQL implementation of the insert port.

use async_trait::async_trait;
use sqlx::PgPool;
use usage_collector_sdk::models::{UsageKind, UsageRecord};

use crate::domain::insert_port::InsertPort;

pub struct PgInsertPort {
    pool: PgPool,
}

impl PgInsertPort {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

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
            .await?;

        Ok(result.rows_affected())
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-3
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-6
}
