//! Continuous aggregate setup for the `TimescaleDB` storage plugin.

use sqlx::PgPool;

use crate::domain::error::MigrationError;

// @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1
/// # Errors
///
/// Returns [`MigrationError`] if any DDL or policy registration statement fails.
pub async fn setup_continuous_aggregate(pool: &PgPool) -> Result<(), MigrationError> {
    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-1
    let view_existed: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM timescaledb_information.continuous_aggregates
            WHERE view_name = 'usage_agg_1h'
        )",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        MigrationError::ContinuousAggregateSetupFailed(format!(
            "failed to check if usage_agg_1h exists: {e}"
        ))
    })?;

    if !view_existed {
        sqlx::query(
            "CREATE MATERIALIZED VIEW IF NOT EXISTS usage_agg_1h \
             WITH (timescaledb.continuous) AS \
             SELECT \
                 time_bucket('1 hour', timestamp) AS bucket, \
                 tenant_id, \
                 metric, \
                 module, \
                 resource_type, \
                 subject_type, \
                 SUM(value)  AS sum_val, \
                 COUNT(*)    AS cnt_val, \
                 MIN(value)  AS min_val, \
                 MAX(value)  AS max_val \
             FROM usage_records \
             GROUP BY bucket, tenant_id, metric, module, resource_type, subject_type \
             WITH NO DATA",
        )
        .execute(pool)
        .await
        .map_err(|e| {
            MigrationError::ContinuousAggregateSetupFailed(format!(
                "failed to create usage_agg_1h continuous aggregate view: {e}"
            ))
        })?;
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-1

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-2
    sqlx::query(
        "SELECT add_continuous_aggregate_policy( \
             'usage_agg_1h', \
             start_offset      => INTERVAL '3 hours', \
             end_offset        => INTERVAL '1 hour', \
             schedule_interval => INTERVAL '30 minutes', \
             if_not_exists     => true \
         )",
    )
    .execute(pool)
    .await
    .map_err(|e| {
        MigrationError::ContinuousAggregateSetupFailed(format!(
            "failed to register refresh policy for usage_agg_1h: {e}"
        ))
    })?;
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-2

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-3
    if !view_existed {
        sqlx::query(
            "CALL refresh_continuous_aggregate('usage_agg_1h', NULL, now() - INTERVAL '1 hour')",
        )
        .execute(pool)
        .await
        .map_err(|e| {
            MigrationError::ContinuousAggregateSetupFailed(format!(
                "failed to trigger initial refresh of usage_agg_1h: {e}"
            ))
        })?;
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-3

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-4
    let view_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM timescaledb_information.continuous_aggregates
            WHERE view_name = 'usage_agg_1h'
        )",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        MigrationError::ContinuousAggregateSetupFailed(format!(
            "failed to verify usage_agg_1h view exists: {e}"
        ))
    })?;

    let policy_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM timescaledb_information.jobs
            WHERE hypertable_name = 'usage_agg_1h'
            AND proc_name = 'policy_refresh_continuous_aggregate'
        )",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        MigrationError::ContinuousAggregateSetupFailed(format!(
            "failed to verify refresh policy for usage_agg_1h: {e}"
        ))
    })?;

    if !view_exists || !policy_exists {
        return Err(MigrationError::ContinuousAggregateSetupFailed(
            "post-setup verification failed: view or refresh policy not found".to_owned(),
        ));
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-4

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-5
    Ok(())
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-5
}
