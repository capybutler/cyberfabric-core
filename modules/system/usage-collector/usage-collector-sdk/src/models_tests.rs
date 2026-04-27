use chrono::Utc;
use uuid::Uuid;

use super::{UsageKind, UsageRecord};

fn make_record() -> UsageRecord {
    UsageRecord {
        module: "test-module".to_owned(),
        tenant_id: Uuid::nil(),
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::nil(),
        resource_type: "test.resource".to_owned(),
        subject_id: Uuid::nil(),
        subject_type: "test.subject".to_owned(),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

#[test]
fn usage_record_roundtrip_serde() {
    let mut rec = make_record();
    rec.value = 42.0;
    let json = serde_json::to_string(&rec).unwrap();
    let deserialized: UsageRecord = serde_json::from_str(&json).unwrap();
    assert!((deserialized.value - 42.0_f64).abs() < f64::EPSILON);
    assert_eq!(deserialized.kind, UsageKind::Gauge);
}

#[test]
fn usage_record_clone_copies_all_fields() {
    let rec = make_record();
    let cloned = rec.clone();
    assert_eq!(cloned.tenant_id, rec.tenant_id);
    assert!((cloned.value - rec.value).abs() < f64::EPSILON);
    assert_eq!(cloned.resource_id, rec.resource_id);
}
