use chrono::Utc;
use modkit_odata::{CursorV1, Page, PageInfo, SortDir};
use uuid::Uuid;

use super::{
    AggregationFn, AggregationQuery, AggregationResult, BucketSize, GroupByDimension, RawQuery,
    UsageKind, UsageRecord,
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
        group_by: vec![
            GroupByDimension::TimeBucket(BucketSize::Day),
            GroupByDimension::UsageType,
        ],
        bucket_size: Some(BucketSize::Day),
        usage_type: Some("compute.cpu".to_owned()),
        resource_id: Some(Uuid::nil()),
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
        max_rows: 0,
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
    assert!(
        !json.contains("bucket_start"),
        "absent Option must not appear in JSON"
    );
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

// ── Page<T> round-trip ───────────────────────────────────────────────────

#[test]
fn page_without_cursor_roundtrip_serde() {
    let page: Page<UsageRecord> = Page::new(
        vec![make_record()],
        PageInfo {
            next_cursor: None,
            prev_cursor: None,
            limit: 50,
        },
    );
    let json = serde_json::to_string(&page).unwrap();
    let deserialized: Page<UsageRecord> = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.items.len(), 1);
    assert!(deserialized.page_info.next_cursor.is_none());
}

#[test]
fn page_with_cursor_roundtrip_serde() {
    let cursor = CursorV1 {
        k: vec![
            "2026-01-01T06:00:00+00:00".to_owned(),
            Uuid::nil().to_string(),
        ],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let encoded = cursor
        .encode()
        .expect("CursorV1 encode is infallible for valid data");
    let page: Page<UsageRecord> = Page::new(
        vec![],
        PageInfo {
            next_cursor: Some(encoded.clone()),
            prev_cursor: None,
            limit: 50,
        },
    );
    let json = serde_json::to_string(&page).unwrap();
    let deserialized: Page<UsageRecord> = serde_json::from_str(&json).unwrap();
    let cursor_str = deserialized
        .page_info
        .next_cursor
        .expect("next_cursor must be present");
    assert_eq!(cursor_str, encoded);
}

// ── CursorV1 encode/decode ───────────────────────────────────────────────

#[test]
fn cursorv1_encode_decode_roundtrip() {
    let original = CursorV1 {
        k: vec![
            "2026-01-01T06:00:00+00:00".to_owned(),
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000")
                .unwrap()
                .to_string(),
        ],
        o: SortDir::Asc,
        s: "+timestamp,+id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let encoded = original
        .encode()
        .expect("CursorV1 encode is infallible for valid data");
    let decoded = CursorV1::decode(&encoded)
        .expect("CursorV1 decode must succeed for freshly encoded cursor");
    assert_eq!(decoded.k, original.k);
    assert_eq!(decoded.o, original.o);
    assert_eq!(decoded.s, original.s);
    assert_eq!(decoded.f, original.f);
    assert_eq!(decoded.d, original.d);
}

#[test]
fn cursorv1_decode_malformed_base64_returns_error() {
    let result = CursorV1::decode("not-valid-base64url!!!");
    assert!(result.is_err(), "malformed base64url must return an error");
}

#[test]
fn cursorv1_decode_invalid_json_returns_error() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL};
    let bad = BASE64URL.encode(b"not-json-at-all");
    let result = CursorV1::decode(&bad);
    assert!(result.is_err(), "invalid JSON payload must return an error");
}

#[test]
fn cursorv1_encode_decode_roundtrip_desc() {
    let original = CursorV1 {
        k: vec![
            "2026-01-01T06:00:00+00:00".to_owned(),
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000")
                .unwrap()
                .to_string(),
        ],
        o: SortDir::Desc,
        s: "-timestamp,-id".to_owned(),
        f: None,
        d: "fwd".to_owned(),
    };
    let encoded = original
        .encode()
        .expect("CursorV1 encode is infallible for valid data");
    let decoded = CursorV1::decode(&encoded)
        .expect("CursorV1 decode must succeed for freshly encoded cursor");
    assert_eq!(decoded.o, SortDir::Desc);
    assert_eq!(decoded.k, original.k);
}

#[test]
fn cursorv1_decode_missing_k_field_returns_error() {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL};
    let json_missing_k = r#"{"o":"asc","s":"+timestamp,+id","d":"fwd"}"#;
    let encoded = BASE64URL.encode(json_missing_k.as_bytes());
    let result = CursorV1::decode(&encoded);
    assert!(
        result.is_err(),
        "CursorV1 missing required k field must return an error"
    );
}

// ── Enum serde name tests ─────────────────────────────────────────────────

#[test]
fn test_aggregation_fn_serde_names() {
    assert_eq!(
        serde_json::to_string(&AggregationFn::Sum).unwrap(),
        "\"sum\""
    );
    assert_eq!(
        serde_json::to_string(&AggregationFn::Count).unwrap(),
        "\"count\""
    );
    assert_eq!(
        serde_json::to_string(&AggregationFn::Min).unwrap(),
        "\"min\""
    );
    assert_eq!(
        serde_json::to_string(&AggregationFn::Max).unwrap(),
        "\"max\""
    );
    assert_eq!(
        serde_json::to_string(&AggregationFn::Avg).unwrap(),
        "\"avg\""
    );
    // Round-trip
    assert_eq!(
        serde_json::from_str::<AggregationFn>("\"sum\"").unwrap(),
        AggregationFn::Sum
    );
    assert_eq!(
        serde_json::from_str::<AggregationFn>("\"avg\"").unwrap(),
        AggregationFn::Avg
    );
}

#[test]
fn test_bucket_size_serde_names() {
    assert_eq!(
        serde_json::to_string(&BucketSize::Minute).unwrap(),
        "\"minute\""
    );
    assert_eq!(
        serde_json::to_string(&BucketSize::Hour).unwrap(),
        "\"hour\""
    );
    assert_eq!(serde_json::to_string(&BucketSize::Day).unwrap(), "\"day\"");
    assert_eq!(
        serde_json::to_string(&BucketSize::Week).unwrap(),
        "\"week\""
    );
    assert_eq!(
        serde_json::to_string(&BucketSize::Month).unwrap(),
        "\"month\""
    );
    // Round-trip
    assert_eq!(
        serde_json::from_str::<BucketSize>("\"hour\"").unwrap(),
        BucketSize::Hour
    );
}

#[test]
fn test_group_by_dimension_serde_names() {
    // Externally tagged enum: {"time_bucket":"day"} for TimeBucket(Day)
    let tb_day = GroupByDimension::TimeBucket(BucketSize::Day);
    let json = serde_json::to_string(&tb_day).unwrap();
    assert!(
        json.contains("time_bucket") && json.contains("day"),
        "TimeBucket(Day) must serialize to externally-tagged JSON, got: {json}"
    );
    // Unit variants use snake_case
    assert_eq!(
        serde_json::to_string(&GroupByDimension::UsageType).unwrap(),
        "\"usage_type\""
    );
    assert_eq!(
        serde_json::to_string(&GroupByDimension::Subject).unwrap(),
        "\"subject\""
    );
    assert_eq!(
        serde_json::to_string(&GroupByDimension::Resource).unwrap(),
        "\"resource\""
    );
    assert_eq!(
        serde_json::to_string(&GroupByDimension::Source).unwrap(),
        "\"source\""
    );
    // Round-trip for a unit variant
    assert_eq!(
        serde_json::from_str::<GroupByDimension>("\"usage_type\"").unwrap(),
        GroupByDimension::UsageType
    );
}
