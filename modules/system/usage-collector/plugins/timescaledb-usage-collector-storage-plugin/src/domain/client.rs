//! TimescaleDB storage plugin client.

use async_trait::async_trait;
use sqlx::PgPool;
use usage_collector_sdk::models::{
    AggregationQuery, AggregationResult, PagedResult, RawQuery, UsageRecord,
};
use usage_collector_sdk::{UsageCollectorError, UsageCollectorPluginClientV1};

/// Storage plugin client backed by a TimescaleDB connection pool.
pub struct TimescaleDbPluginClient {
    #[allow(dead_code)]
    pool: PgPool,
}

impl TimescaleDbPluginClient {
    /// Creates a new client wrapping the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UsageCollectorPluginClientV1 for TimescaleDbPluginClient {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        todo!("Phase 5: create_usage_record — idempotent ingest write path")
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        todo!("Phase 6: query_aggregated — aggregation query with routing")
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        todo!("Phase 7: query_raw — cursor-based raw record pagination")
    }
}
