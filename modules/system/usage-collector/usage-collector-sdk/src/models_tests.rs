use chrono::Utc;
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
