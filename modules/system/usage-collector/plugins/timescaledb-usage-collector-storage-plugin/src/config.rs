//! Configuration for the TimescaleDB usage-collector storage plugin.

use std::time::Duration;

use serde::Deserialize;

fn default_pool_size_min() -> u32 {
    2
}

fn default_pool_size_max() -> u32 {
    16
}

fn default_retention_default() -> Duration {
    Duration::from_secs(365 * 24 * 3_600)
}

fn default_connection_timeout() -> Duration {
    Duration::from_secs(10)
}

fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    humantime::parse_duration(&s).map_err(serde::de::Error::custom)
}

/// Plugin configuration.
///
/// All parameters are static and require a plugin restart to change.
/// `database_url` must include `sslmode=require`; plaintext connections are rejected.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimescaleDbConfig {
    /// PostgreSQL connection URL. Must include `sslmode=require`.
    #[serde(default)]
    pub database_url: String,

    /// Minimum connection pool size (1–64). Default: 2.
    #[serde(default = "default_pool_size_min")]
    pub pool_size_min: u32,

    /// Maximum connection pool size (1–128). Must be >= `pool_size_min`. Default: 16.
    #[serde(default = "default_pool_size_max")]
    pub pool_size_max: u32,

    /// Default data retention period (7 days – 7 years). Default: 365 days.
    #[serde(
        default = "default_retention_default",
        deserialize_with = "deserialize_duration"
    )]
    pub retention_default: Duration,

    /// Connection acquisition timeout (1s – 60s). Default: 10s.
    #[serde(
        default = "default_connection_timeout",
        deserialize_with = "deserialize_duration"
    )]
    pub connection_timeout: Duration,
}

impl TimescaleDbConfig {
    /// Validates all configuration parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if `database_url` is missing, TLS is not required,
    /// pool sizes are out of range, or timeouts are out of range.
    pub fn validate(&self) -> anyhow::Result<()> {
        todo!("Phase 8: validate TimescaleDB configuration")
    }
}

impl Default for TimescaleDbConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            pool_size_min: default_pool_size_min(),
            pool_size_max: default_pool_size_max(),
            retention_default: default_retention_default(),
            connection_timeout: default_connection_timeout(),
        }
    }
}
