use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use modkit_db::outbox::{LeasedMessageHandler, MessageResult, OutboxMessage};
use usage_collector_sdk::models::{UsageKind, UsageRecord};
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use uuid::Uuid;

use super::DeliveryHandler;

enum CollectorOutcome {
    Ok,
    Transient,
    Permanent,
}

struct MockCollector {
    outcome: CollectorOutcome,
}

impl MockCollector {
    fn ok() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Ok,
        })
    }

    fn transient() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Transient,
        })
    }

    fn permanent() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Permanent,
        })
    }
}

#[async_trait]
impl UsageCollectorClientV1 for MockCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        match self.outcome {
            CollectorOutcome::Ok => Ok(()),
            CollectorOutcome::Transient => Err(UsageCollectorError::plugin_timeout()),
            CollectorOutcome::Permanent => {
                Err(UsageCollectorError::authorization_failed("permanent"))
            }
        }
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![],
        })
    }
}

fn valid_usage_record() -> UsageRecord {
    UsageRecord {
        tenant_id: Uuid::new_v4(),
        module: "test-module".to_owned(),
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        subject_id: Uuid::nil(),
        subject_type: "test.subject".to_owned(),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

fn make_msg(payload_bytes: Vec<u8>) -> OutboxMessage {
    OutboxMessage {
        partition_id: 0,
        seq: 1,
        payload: payload_bytes,
        payload_type: "application/json".to_owned(),
        created_at: Utc::now(),
        attempts: 0,
    }
}

fn handler(collector: Arc<dyn UsageCollectorClientV1>) -> DeliveryHandler {
    DeliveryHandler::new(collector)
}

#[tokio::test]
async fn handle_invalid_json_payload_is_rejected() {
    let h = handler(MockCollector::ok());
    let msg = make_msg(b"not-json".to_vec());
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Reject(_)));
}

#[tokio::test]
async fn handle_collector_transient_error_returns_retry() {
    let h = handler(MockCollector::transient());
    let payload = serde_json::to_vec(&valid_usage_record()).unwrap();
    let msg = make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Retry));
}

#[tokio::test]
async fn handle_collector_permanent_error_returns_reject() {
    let h = handler(MockCollector::permanent());
    let payload = serde_json::to_vec(&valid_usage_record()).unwrap();
    let msg = make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Reject(_)));
}

#[tokio::test]
async fn handle_success_returns_ok() {
    let h = handler(MockCollector::ok());
    let payload = serde_json::to_vec(&valid_usage_record()).unwrap();
    let msg = make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Ok));
}
