use std::sync::{Arc, Mutex};

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
    Unavailable,
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

    fn unavailable() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Unavailable,
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
            CollectorOutcome::Unavailable => {
                Err(UsageCollectorError::unavailable("connection refused"))
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
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
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
async fn handle_collector_unavailable_error_returns_retry() {
    let h = handler(MockCollector::unavailable());
    let payload = serde_json::to_vec(&valid_usage_record()).unwrap();
    let msg = make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Retry));
}

#[tokio::test]
async fn handle_success_returns_ok() {
    let h = handler(MockCollector::ok());
    let payload = serde_json::to_vec(&valid_usage_record()).unwrap();
    let msg = make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Ok));
}

// ── Subject fields deserialization ────────────────────────────────────────────

struct CapturingCollector {
    captured: Arc<Mutex<Option<UsageRecord>>>,
}

impl CapturingCollector {
    fn new(captured: Arc<Mutex<Option<UsageRecord>>>) -> Arc<Self> {
        Arc::new(Self { captured })
    }
}

#[async_trait]
impl UsageCollectorClientV1 for CapturingCollector {
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        *self.captured.lock().unwrap() = Some(record);
        Ok(())
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

#[tokio::test]
async fn handle_delivery_preserves_subject_fields_through_deserialization() {
    let known_subject_id = Uuid::new_v4();
    let record = UsageRecord {
        subject_id: Some(known_subject_id),
        subject_type: Some("real.subject".to_owned()),
        ..valid_usage_record()
    };

    let captured: Arc<Mutex<Option<UsageRecord>>> = Arc::new(Mutex::new(None));
    let collector = CapturingCollector::new(Arc::clone(&captured));
    let h = handler(collector);
    let payload = serde_json::to_vec(&record).unwrap();
    let msg = make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Ok));

    let received = captured
        .lock()
        .unwrap()
        .take()
        .expect("collector must have received the record");
    assert_eq!(
        received.subject_id,
        Some(known_subject_id),
        "subject_id must survive serialisation round-trip"
    );
    assert_eq!(
        received.subject_type.as_deref(),
        Some("real.subject"),
        "subject_type must survive serialisation round-trip"
    );
}
