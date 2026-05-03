//! Schema migration runner for the TimescaleDB storage plugin.

use sqlx::PgPool;

use crate::domain::error::MigrationError;

// @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1
pub async fn run_migrations(pool: &PgPool) -> Result<(), MigrationError> {
    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-1
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE")
        .execute(pool)
        .await
        .map_err(|e| MigrationError::Migration(format!("failed to create timescaledb extension: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-1

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-2
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS usage_records (
            id              UUID        NOT NULL DEFAULT gen_random_uuid(),
            tenant_id       UUID        NOT NULL,
            module          TEXT        NOT NULL,
            kind            TEXT        NOT NULL CHECK (kind IN ('counter', 'gauge')),
            metric          TEXT        NOT NULL,
            value           NUMERIC     NOT NULL,
            timestamp       TIMESTAMPTZ NOT NULL,
            idempotency_key TEXT,
            resource_id     UUID        NOT NULL,
            resource_type   TEXT        NOT NULL,
            subject_id      UUID,
            subject_type    TEXT,
            ingested_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            metadata        JSONB,
            PRIMARY KEY (id, timestamp)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to create usage_records table: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-2

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8
    // Separate plain table for idempotency deduplication. TimescaleDB requires all
    // unique indexes on a hypertable to include the partition column (timestamp), so
    // cross-partition idempotency cannot be enforced with a partial index on usage_records.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS usage_idempotency_keys (
            tenant_id       UUID NOT NULL,
            idempotency_key TEXT NOT NULL,
            PRIMARY KEY (tenant_id, idempotency_key)
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to create usage_idempotency_keys table: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-3
    sqlx::query(
        "SELECT create_hypertable('usage_records', 'timestamp', if_not_exists => true)",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to convert usage_records to hypertable: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-3

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-4
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_usage_records_tenant_time \
         ON usage_records (tenant_id, timestamp DESC)",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to create idx_usage_records_tenant_time: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-4

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-5
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_usage_records_tenant_metric_time \
         ON usage_records (tenant_id, metric, timestamp DESC)",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to create idx_usage_records_tenant_metric_time: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-5

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-6
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_usage_records_tenant_subject_time \
         ON usage_records (tenant_id, subject_id, timestamp DESC)",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to create idx_usage_records_tenant_subject_time: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-6

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-7
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_usage_records_tenant_resource_time \
         ON usage_records (tenant_id, resource_id, timestamp DESC)",
    )
    .execute(pool)
    .await
    .map_err(|e| MigrationError::Migration(format!("failed to create idx_usage_records_tenant_resource_time: {e}")))?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-7

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-9
    Ok(())
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-9
}
