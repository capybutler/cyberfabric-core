//! TimescaleDB Usage Collector Storage Plugin
//!
//! A production [`usage_collector_sdk::UsageCollectorPluginClientV1`] implementation
//! backed by TimescaleDB for durable high-throughput usage record persistence,
//! aggregation query pushdown via continuous aggregates, and cursor-based raw pagination.
//!
//! ## Configuration
//!
//! ```yaml
//! modules:
//!   timescaledb_usage_collector_storage_plugin:
//!     config:
//!       database_url: "postgres://user:pass@host/db?sslmode=require"
//!       pool_size_min: 2
//!       pool_size_max: 16
//!       retention_default: "365days"
//!       connection_timeout: "10s"
//! ```
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod config;
mod domain;
mod infra;
mod module;
