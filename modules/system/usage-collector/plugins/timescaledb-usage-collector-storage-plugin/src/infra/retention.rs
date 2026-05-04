//! Retention policy setup for the `TimescaleDB` storage plugin.

use std::time::Duration;

use sqlx::PgPool;

use crate::domain::error::StoragePluginError;

/// Applies the `TimescaleDB` retention policy to `usage_records` and removes
/// expired rows from `usage_idempotency_keys`.
///
/// # Errors
///
/// Returns [`StoragePluginError`] if any statement fails.
pub async fn setup_retention_policy(
    pool: &PgPool,
    retention: Duration,
) -> Result<(), StoragePluginError> {
    let interval = format!("{} seconds", retention.as_secs());

    sqlx::query(
        "SELECT add_retention_policy('usage_records', $1::interval, if_not_exists => true)",
    )
    .bind(&interval)
    .execute(pool)
    .await
    .map_err(|e| {
        StoragePluginError::RetentionPolicySetupFailed(format!(
            "failed to add retention policy for usage_records: {e}"
        ))
    })?;

    sqlx::query("DELETE FROM usage_idempotency_keys WHERE created_at < NOW() - $1::interval")
        .bind(&interval)
        .execute(pool)
        .await
        .map_err(|e| {
            StoragePluginError::RetentionPolicySetupFailed(format!(
                "failed to clean up expired idempotency keys: {e}"
            ))
        })?;

    Ok(())
}
