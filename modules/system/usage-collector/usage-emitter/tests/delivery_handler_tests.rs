#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use modkit_db::outbox::{LeasedMessageHandler, MessageResult};
use usage_collector_sdk::models::UsageRecord;
use usage_emitter::DeliveryHandler;
use uuid::Uuid;

#[tokio::test]
async fn handle_invalid_json_payload_is_rejected() {
    let h = DeliveryHandler::new(common::MockCollector::ok());
    let msg = common::make_msg(b"not-json".to_vec());
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Reject(_)));
}

#[tokio::test]
async fn handle_collector_transient_error_returns_retry() {
    let h = DeliveryHandler::new(common::MockCollector::transient());
    let payload = serde_json::to_vec(&common::valid_usage_record()).unwrap();
    let msg = common::make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Retry));
}

#[tokio::test]
async fn handle_collector_permanent_error_returns_reject() {
    let h = DeliveryHandler::new(common::MockCollector::permanent());
    let payload = serde_json::to_vec(&common::valid_usage_record()).unwrap();
    let msg = common::make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Reject(_)));
}

#[tokio::test]
async fn handle_collector_unavailable_error_returns_retry() {
    let h = DeliveryHandler::new(common::MockCollector::unavailable());
    let payload = serde_json::to_vec(&common::valid_usage_record()).unwrap();
    let msg = common::make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Retry));
}

#[tokio::test]
async fn handle_success_returns_ok() {
    let h = DeliveryHandler::new(common::MockCollector::ok());
    let payload = serde_json::to_vec(&common::valid_usage_record()).unwrap();
    let msg = common::make_msg(payload);
    let result = h.handle(&msg).await;
    assert!(matches!(result, MessageResult::Ok));
}

#[tokio::test]
async fn handle_delivery_preserves_subject_fields_through_deserialization() {
    let known_subject_id = Uuid::new_v4();
    let record = UsageRecord {
        subject_id: Some(known_subject_id),
        subject_type: Some("real.subject".to_owned()),
        ..common::valid_usage_record()
    };

    let captured: Arc<Mutex<Option<UsageRecord>>> = Arc::new(Mutex::new(None));
    let collector = common::CapturingCollector::new(Arc::clone(&captured));
    let h = DeliveryHandler::new(collector);
    let payload = serde_json::to_vec(&record).unwrap();
    let msg = common::make_msg(payload);
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
