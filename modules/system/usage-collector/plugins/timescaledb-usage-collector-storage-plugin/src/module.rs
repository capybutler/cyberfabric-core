//! TimescaleDB usage-collector storage plugin module.
//!
//! Registers a GTS plugin instance in the types registry and exposes
//! [`usage_collector_sdk::UsageCollectorPluginClientV1`] backed by a TimescaleDB connection pool.

use std::sync::Arc;

use async_trait::async_trait;
use modkit::Module;
use modkit::client_hub::ClientScope;
use modkit::context::ModuleCtx;
use modkit::gts::BaseModkitPluginV1;
use opentelemetry::global;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::{debug, error, info};
use types_registry_sdk::{RegisterResult, TypesRegistryClient};
use usage_collector_sdk::{UsageCollectorPluginClientV1, UsageCollectorStoragePluginSpecV1};

use crate::config::TimescaleDbConfig;
use crate::domain::client::TimescaleDbPluginClient;
use crate::domain::metrics::NoopMetrics;
use crate::infra::continuous_aggregate::setup_continuous_aggregate;
use crate::infra::migrations::run_migrations;
use crate::infra::pg_insert_port::PgInsertPort;

/// TimescaleDB production storage plugin for the usage-collector gateway.
#[modkit::module(
    name = "timescaledb-usage-collector-storage-plugin",
    deps = ["types-registry", "usage-collector"]
)]
#[derive(Default)]
struct TimescaleDbStoragePlugin;

#[async_trait]
impl Module for TimescaleDbStoragePlugin {
    // @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1
    // @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts:p1
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-validate-config
        let cfg: TimescaleDbConfig = ctx.config().map_err(|e| {
            error!("TimescaleDB plugin configuration load failed");
            e
        })?;
        cfg.validate().map_err(|e| {
            error!(error = %e, "TimescaleDB plugin configuration validation failed");
            anyhow::anyhow!("configuration validation failed: {e}")
        })?;
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-validate-config

        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts:p1:inst-build-secure-conn
        // TLS is enforced: database_url validated above to contain sslmode=require.
        // The URL is captured here and never written to logs or error messages.
        let database_url = cfg.database_url.clone();
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts:p1:inst-build-secure-conn

        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-build-pool
        let pool = PgPoolOptions::new()
            .min_connections(cfg.pool_size_min)
            .max_connections(cfg.pool_size_max)
            .acquire_timeout(cfg.connection_timeout)
            .connect(&database_url)
            .await
            .map_err(|_| {
                error!("Failed to create TimescaleDB connection pool");
                anyhow::anyhow!("connection pool initialization failed")
            })?;
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-build-pool

        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-run-migrations
        run_migrations(&pool).await.map_err(|e| {
            error!(error = %e, "TimescaleDB schema migration failed");
            anyhow::anyhow!("schema migration failed")
        })?;
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-run-migrations

        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-setup-continuous-aggregate
        setup_continuous_aggregate(&pool).await.map_err(|e| {
            error!(error = %e, "TimescaleDB continuous aggregate setup failed");
            anyhow::anyhow!("continuous aggregate setup failed")
        })?;
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-setup-continuous-aggregate

        let instance_id = UsageCollectorStoragePluginSpecV1::gts_make_instance_id(
            "cf.core._.timescaledb_usage_collector_storage_plugin.v1",
        );

        let registry = ctx.client_hub().get::<dyn TypesRegistryClient>()?;
        let instance = BaseModkitPluginV1::<UsageCollectorStoragePluginSpecV1> {
            id: instance_id.clone(),
            vendor: "virtuozzo".to_string(),
            priority: 10,
            properties: UsageCollectorStoragePluginSpecV1,
        };
        let instance_json = serde_json::to_value(&instance)?;

        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts:p1:inst-register-gts
        let results = registry
            .register(vec![instance_json])
            .await
            .map_err(|e| {
                error!(error = %e, "GTS registration call failed for TimescaleDB plugin");
                anyhow::anyhow!("GTS registration failed")
            })?;
        RegisterResult::ensure_all_ok(&results).map_err(|e| {
            error!(error = %e, "GTS registration rejected for TimescaleDB plugin");
            e
        })?;
        info!(%instance_id, "GTS registration successful for TimescaleDB storage plugin");
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts:p1:inst-register-gts

        let insert_port: Arc<dyn crate::domain::insert_port::InsertPort> =
            Arc::new(PgInsertPort::new(pool.clone()));
        let metrics: Arc<dyn crate::domain::metrics::PluginMetrics> = Arc::new(NoopMetrics);
        let client = TimescaleDbPluginClient::new(insert_port, pool.clone(), metrics);
        let api: Arc<dyn UsageCollectorPluginClientV1> = Arc::new(client);

        // @cpt-begin:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-register-client
        ctx.client_hub()
            .register_scoped::<dyn UsageCollectorPluginClientV1>(
                ClientScope::gts_id(&instance_id),
                api,
            );
        // @cpt-end:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1:inst-register-client

        info!(
            %instance_id,
            "TimescaleDB usage-collector storage plugin started successfully"
        );

        tokio::spawn(run_health_check_loop(pool));

        Ok(())
    }
}

async fn run_health_check_loop(pool: PgPool) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        health_check(&pool).await;
    }
}

/// Executes a liveness probe against the pool and emits the `storage_health_status` gauge.
///
/// Emits `1.0` when the probe succeeds, `0.0` when it fails.
async fn health_check(pool: &PgPool) {
    let healthy = sqlx::query("SELECT 1").execute(pool).await.is_ok();
    let status = if healthy { 1.0_f64 } else { 0.0_f64 };

    let meter = global::meter("timescaledb-usage-collector-storage-plugin");
    let gauge = meter.f64_gauge("storage_health_status").build();
    gauge.record(status, &[]);

    if healthy {
        debug!("TimescaleDB health check passed");
    } else {
        error!("TimescaleDB health check failed: pool unreachable");
    }
}
