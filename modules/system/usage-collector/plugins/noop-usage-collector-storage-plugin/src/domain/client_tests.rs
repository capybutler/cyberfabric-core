use chrono::Utc;
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
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

#[tokio::test]
async fn create_usage_record_always_returns_ok() {
    let service = Service::new();
    let plugin: &dyn UsageCollectorPluginClientV1 = &service;
    let rec = make_record(Uuid::new_v4());
    let result = plugin.create_usage_record(rec).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn create_usage_record_multiple_records_all_return_ok() {
    let service = Service::new();
    let plugin: &dyn UsageCollectorPluginClientV1 = &service;
    for _ in 0..5 {
        let rec = make_record(Uuid::new_v4());
        let result = plugin.create_usage_record(rec).await;
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn noop_query_aggregated_returns_empty_vec() {
    use modkit_security::AccessScope;
    use usage_collector_sdk::models::{AggregationFn, AggregationQuery, GroupByDimension};

    let svc = Service::new();
    let query = AggregationQuery {
        scope: AccessScope::deny_all(),
        time_range: (Utc::now(), Utc::now()),
        function: AggregationFn::Sum,
        group_by: vec![GroupByDimension::UsageType],
        bucket_size: None,
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_id: None,
        subject_type: None,
        source: None,
    };
    let result = svc.query_aggregated(query).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), vec![]);
}

#[tokio::test]
async fn noop_query_raw_returns_empty_paged_result() {
    use modkit_security::AccessScope;
    use usage_collector_sdk::models::RawQuery;

    let svc = Service::new();
    let query = RawQuery {
        scope: AccessScope::deny_all(),
        time_range: (Utc::now(), Utc::now()),
        usage_type: None,
        resource_id: None,
        resource_type: None,
        subject_type: None,
        subject_id: None,
        cursor: None,
        page_size: 100,
    };
    let result = svc.query_raw(query).await;
    assert!(result.is_ok());
    let paged = result.unwrap();
    assert!(paged.items.is_empty());
    assert!(paged.next_cursor.is_none());
}
