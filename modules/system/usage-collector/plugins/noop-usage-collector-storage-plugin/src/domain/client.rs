use async_trait::async_trait;

use usage_collector_sdk::models::{
    AggregationQuery, AggregationResult, RawQuery, UsageRecord,
};
use usage_collector_sdk::{Page, PageInfo, UsageCollectorError, UsageCollectorPluginClientV1};

use super::service::Service;

#[async_trait]
impl UsageCollectorPluginClientV1 for Service {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    // @cpt-algo:cpt-cf-usage-collector-algo-query-api-noop-stubs:p1
    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-8
    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-noop-stubs:p1:inst-noop-1
    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-noop-stubs:p1:inst-noop-1
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p1:inst-sdk-8

    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-9
    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-noop-stubs:p2:inst-noop-2
    async fn query_raw(
        &self,
        query: RawQuery,
    ) -> Result<Page<UsageRecord>, UsageCollectorError> {
        Ok(Page::new(
            vec![],
            PageInfo { next_cursor: None, prev_cursor: None, limit: query.page_size as u64 },
        ))
    }
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-noop-stubs:p2:inst-noop-2
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-9
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "client_tests.rs"]
mod client_tests;
