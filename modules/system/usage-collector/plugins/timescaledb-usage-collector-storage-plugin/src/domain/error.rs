//! Error types for the TimescaleDB storage plugin.

use thiserror::Error;

/// Errors produced by the TimescaleDB storage plugin.
#[derive(Debug, Error)]
pub enum StoragePluginError {
    /// A record field failed validation (e.g. negative counter value, missing idempotency key).
    #[error("invalid record: {0}")]
    InvalidRecord(String),

    /// A transient database error (connection lost, pool timeout, serialization failure).
    #[error("transient error: {0}")]
    Transient(String),

    /// A configuration error detected at startup (missing URL, TLS rejected, etc.).
    #[error("configuration error: {0}")]
    Configuration(String),

    /// A schema migration step failed.
    #[error("migration error: {0}")]
    Migration(String),

    /// The continuous aggregate setup step failed.
    #[error("continuous aggregate setup failed: {0}")]
    ContinuousAggregateSetupFailed(String),

    /// A query against the database failed.
    #[error("query failed: {0}")]
    QueryFailed(String),

    /// A connection pool error (pool exhausted, pool creation failed, etc.).
    #[error("connection pool error: {0}")]
    ConnectionPool(String),
}

/// Type alias for migration-related errors.
pub type MigrationError = StoragePluginError;
