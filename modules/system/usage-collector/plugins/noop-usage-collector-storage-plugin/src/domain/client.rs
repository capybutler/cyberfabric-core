use async_trait::async_trait;

use usage_collector_sdk::models::{
    AggregationQuery, AggregationResult, PagedResult, RawQuery, UsageRecord,
};
use usage_collector_sdk::{UsageCollectorError, UsageCollectorPluginClientV1};

use super::service::Service;

#[async_trait]
impl UsageCollectorPluginClientV1 for Service {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult {
            items: vec![],
            next_cursor: None,
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "client_tests.rs"]
mod client_tests;
