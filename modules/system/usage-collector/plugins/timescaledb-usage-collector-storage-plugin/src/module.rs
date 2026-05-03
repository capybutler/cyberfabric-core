//! TimescaleDB usage-collector storage plugin module.
//!
//! Registers a GTS plugin instance in the types registry and exposes
//! [`usage_collector_sdk::UsageCollectorPluginClientV1`] backed by a TimescaleDB connection pool.

use async_trait::async_trait;
use modkit::Module;
use modkit::context::ModuleCtx;

/// TimescaleDB production storage plugin for the usage-collector gateway.
#[modkit::module(
    name = "timescaledb-usage-collector-storage-plugin",
    deps = ["types-registry", "usage-collector"]
)]
#[derive(Default)]
struct TimescaleDbStoragePlugin;

#[async_trait]
impl Module for TimescaleDbStoragePlugin {
    async fn init(&self, _ctx: &ModuleCtx) -> anyhow::Result<()> {
        todo!("Phase 8: GTS registration, pool creation, migrations, and health check")
    }
}
