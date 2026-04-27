use std::sync::Arc;

use async_trait::async_trait;
use modkit_db::outbox::{LeasedMessageHandler, MessageResult, OutboxMessage};
use tracing::{debug, error, info, warn};
use usage_collector_sdk::models::UsageRecord;
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};

/// Outbox delivery handler that forwards dequeued usage records to the usage collector,
/// calling [`UsageCollectorClientV1::create_usage_record`] once per message.
///
/// Implements [`LeasedMessageHandler`]: deserialization failures dead-letter the message;
/// collector failures map to batch retry or reject.
pub struct DeliveryHandler {
    collector: Arc<dyn UsageCollectorClientV1>,
}

impl DeliveryHandler {
    #[must_use]
    pub fn new(collector: Arc<dyn UsageCollectorClientV1>) -> Self {
        Self { collector }
    }
}

#[async_trait]
impl LeasedMessageHandler for DeliveryHandler {
    // @cpt-algo:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1
    // @cpt-flow:cpt-cf-usage-collector-flow-sdk-and-ingest-core-emit:p1
    async fn handle(&self, msg: &OutboxMessage) -> MessageResult {
        // Processor may pass several messages per lease cycle (see WorkerTuning::batch_size);
        // this handler still delivers each payload individually.

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-1
        let record_result = serde_json::from_slice::<UsageRecord>(&msg.payload);
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-1

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-2
        if let Err(ref err) = record_result {
            warn!(
                msg.seq,
                msg.partition_id,
                %err,
                "usage record deserialization failed"
            );
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-2a
            return MessageResult::Reject(err.to_string());
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-2a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-2

        // inst-dlv-3: gateway ingest request assembly from UsageRecord fields is performed
        // inside UsageCollectorClientV1::create_usage_record — see rest_client.rs
        // (UsageRecord IS the request at this layer; DTO assembly is an implementation detail
        //  of the REST client adapter).
        // SAFETY: the Err branch above always returns; this value is Ok.
        #[allow(clippy::unwrap_used)]
        let record = record_result.unwrap();

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-4
        let delivery_result = self.collector.create_usage_record(record).await;
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-4

        match delivery_result {
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-5
            Ok(()) => {
                debug!("usage record delivered to collector");
                // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-5a
                MessageResult::Ok
                // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-5a
            }
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-5
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-6
            Err(e @ (UsageCollectorError::PluginTimeout | UsageCollectorError::CircuitOpen)) => {
                // PluginTimeout covers: network timeout, HTTP 429, HTTP 5xx (see rest_client.rs).
                // CircuitOpen means the gateway circuit breaker is open; retry after backoff.
                info!(error = %e, "transient collector delivery error; will retry");
                // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-6a
                MessageResult::Retry
                // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-6a
            }
            // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-6
            // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-7
            Err(e) => {
                error!(error = %e, "permanent collector delivery error; dead-lettering message");
                // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-7a
                MessageResult::Reject(e.to_string())
                // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-7a
            } // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-7
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "delivery_handler_tests.rs"]
mod delivery_handler_tests;
