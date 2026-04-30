use chrono::Utc;
use uuid::Uuid;

use super::{
    AggregationFn, AggregationQuery, AggregationResult, BucketSize, Cursor, GroupByDimension,
    PagedResult, RawQuery, UsageKind, UsageRecord,
};
use modkit_security::AccessScope;

fn make_record() -> UsageRecord {
    UsageRecord {
        module: "test-module".to_owned(),
        tenant_id: Uuid::nil(),
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::nil(),
        resource_type: "test.resource".to_owned(),
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
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

#[test]
fn usage_record_subject_none_serde() {
    let rec = UsageRecord {
        subject_id: None,
        subject_type: None,
        ..make_record()
    };
    let json = serde_json::to_string(&rec).unwrap();
    assert!(
        !json.contains("\"subject_id\""),
        "subject_id must be absent from JSON when None, got: {json}"
    );
    assert!(
        !json.contains("\"subject_type\""),
        "subject_type must be absent from JSON when None, got: {json}"
    );
    let deserialized: UsageRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.subject_id, None);
    assert_eq!(deserialized.subject_type, None);
}

// ── AggregationQuery round-trip ───────────────────────────────────────────

#[test]
fn aggregation_query_roundtrip_serde() {
    let from = Utc::now();
    let to = Utc::now();
    let query = AggregationQuery {
        scope: AccessScope::deny_all(),
        time_range: (from, to),
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::TimeBucket(BucketSize::Day), GroupByDimension::UsageType],
        bucket_size: Some(BucketSize::Day),
        usage_type: Some("compute.cpu".to_owned()),
        resource_id: Some(Uuid::nil()),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
    };
    let json = serde_json::to_string(&query).unwrap();
    let deserialized: AggregationQuery = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.function, AggregationFn::Sum);
    assert_eq!(deserialized.bucket_size, Some(BucketSize::Day));
    assert_eq!(deserialized.usage_type.as_deref(), Some("compute.cpu"));
    assert_eq!(deserialized.resource_id, Some(Uuid::nil()));
    // scope is skipped in serde; defaults to deny_all
    assert!(deserialized.scope.is_deny_all());
}

// ── AggregationResult round-trip ─────────────────────────────────────────

#[test]
fn aggregation_result_roundtrip_serde() {
    let result = AggregationResult {
        function: AggregationFn::Count,
        value: 42.0,
        bucket_start: None,
        usage_type: Some("compute.cpu".to_owned()),
        subject_id: None,
        subject_type: None,
        resource_id: None,
        resource_type: None,
        source: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: AggregationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.function, AggregationFn::Count);
    assert!((deserialized.value - 42.0_f64).abs() < f64::EPSILON);
    assert_eq!(deserialized.usage_type.as_deref(), Some("compute.cpu"));
    assert!(!json.contains("bucket_start"), "absent Option must not appear in JSON");
}

// ── RawQuery round-trip ───────────────────────────────────────────────────

#[test]
fn raw_query_roundtrip_serde() {
    let from = Utc::now();
    let to = Utc::now();
    let query = RawQuery {
        scope: AccessScope::deny_all(),
        time_range: (from, to),
        usage_type: Some("network.bytes".to_owned()),
        resource_id: None,
        resource_type: None,
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 50,
    };
    let json = serde_json::to_string(&query).unwrap();
    let deserialized: RawQuery = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.page_size, 50);
    assert_eq!(deserialized.usage_type.as_deref(), Some("network.bytes"));
    assert!(deserialized.scope.is_deny_all());
}

// ── PagedResult round-trip ────────────────────────────────────────────────

#[test]
fn paged_result_without_cursor_roundtrip_serde() {
    let result: PagedResult<UsageRecord> = PagedResult {
        items: vec![make_record()],
        next_cursor: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: PagedResult<UsageRecord> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.items.len(), 1);
    assert!(deserialized.next_cursor.is_none());
    assert!(!json.contains("next_cursor"), "absent cursor must not appear in JSON");
}

#[test]
fn paged_result_with_cursor_roundtrip_serde() {
    let cursor = Cursor {
        timestamp: "2026-01-01T06:00:00Z".parse::<chrono::DateTime<Utc>>().unwrap(),
        id: Uuid::nil(),
    };
    let result: PagedResult<UsageRecord> = PagedResult {
        items: vec![],
        next_cursor: Some(cursor.clone()),
    };
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: PagedResult<UsageRecord> = serde_json::from_str(&json).unwrap();
    assert!(deserialized.next_cursor.is_some());
    let decoded = deserialized.next_cursor.unwrap();
    assert_eq!(decoded, cursor);
}

// ── Cursor encode/decode ──────────────────────────────────────────────────

#[test]
fn cursor_encode_decode_roundtrip() {
    let original = Cursor {
        timestamp: "2026-01-01T06:00:00Z".parse::<chrono::DateTime<Utc>>().unwrap(),
        id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
    };
    let encoded = original.encode();
    let decoded = Cursor::decode(&encoded).expect("decode must succeed");
    assert_eq!(decoded, original);
}

#[test]
fn cursor_decode_malformed_base64_returns_error() {
    let result = Cursor::decode("not-valid-base64!!!");
    assert!(result.is_err(), "malformed base64 must return an error");
}

#[test]
fn cursor_decode_missing_field_returns_error() {
    // valid base64 but missing 'id' field
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    let bad = BASE64.encode(b"timestamp=2026-01-01T00:00:00Z");
    let result = Cursor::decode(&bad);
    assert!(result.is_err());
}
