use chrono::Utc;
use usage_collector_sdk::models::Subject;
use usage_collector_sdk::{UsageCollectorPluginClientV1, UsageKind, UsageRecord};
use uuid::Uuid;

use super::Service;

fn make_record(tenant_id: Uuid) -> UsageRecord {
    UsageRecord {
        module: "test-module".to_owned(),
        tenant_id,
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        subject: Some(Subject::with_type(Uuid::nil(), "test.subject")),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

#[tokio::test]
async fn create_usage_record_always_returns_ok() {
    let service = Service;
    let plugin: &dyn UsageCollectorPluginClientV1 = &service;
    let rec = make_record(Uuid::new_v4());
    let result = plugin.create_usage_record(rec).await;
    assert!(result.is_ok());
}
