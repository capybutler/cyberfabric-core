#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use modkit_db::Db;
use usage_collector_sdk::models::{AllowedMetric, UsageKind, UsageRecord};
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use usage_emitter::{AuthorizedUsageEmitter, UsageEmitter, UsageEmitterConfig, UsageEmitterError, UsageEmitterV1};
use uuid::Uuid;

const FIXTURE_RESOURCE_TYPE: &str = "test.resource";

// ── Fixture collector that returns specific allowed metrics ───────────────────

struct FixtureCollector;

#[async_trait]
impl UsageCollectorClientV1 for FixtureCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![
                AllowedMetric {
                    name: "test.gauge".to_owned(),
                    kind: UsageKind::Gauge,
                },
                AllowedMetric {
                    name: "test.counter".to_owned(),
                    kind: UsageKind::Counter,
                },
            ],
        })
    }
}

// ── Test fixture ──────────────────────────────────────────────────────────────

struct Fixture {
    db: Db,
    _emitter: UsageEmitter,
    emitter: AuthorizedUsageEmitter,
    tenant: Uuid,
    resource_id: Uuid,
}

impl Fixture {
    async fn build(name: &str) -> Self {
        Self::build_with_config(name, UsageEmitterConfig::default()).await
    }

    async fn build_with_config(name: &str, config: UsageEmitterConfig) -> Self {
        let db = common::build_db(name).await;
        let emitter_obj = UsageEmitter::build(
            config,
            db.clone(),
            Arc::new(common::AllowAllAuthZ),
            Arc::new(FixtureCollector),
        )
        .await
        .unwrap();

        let ctx = common::make_ctx();
        let tenant = ctx.subject_tenant_id();
        let resource_id = Uuid::new_v4();

        let emitter = emitter_obj
            .for_module("test-module")
            .authorize_for(
                &ctx,
                tenant,
                resource_id,
                FIXTURE_RESOURCE_TYPE.to_owned(),
                Some(Uuid::nil()),
                Some("test.subject".to_owned()),
            )
            .await
            .unwrap();

        Self {
            db,
            _emitter: emitter_obj,
            emitter,
            tenant,
            resource_id,
        }
    }

    fn record(&self) -> UsageRecord {
        UsageRecord {
            tenant_id: self.tenant,
            module: "test-module".to_owned(),
            metric: "test.gauge".to_owned(),
            kind: UsageKind::Gauge,
            value: 1.0,
            resource_id: self.resource_id,
            resource_type: FIXTURE_RESOURCE_TYPE.to_owned(),
            subject_id: Some(Uuid::nil()),
            subject_type: Some("test.subject".to_owned()),
            idempotency_key: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            metadata: None,
        }
    }

    fn record_with_kind(&self, kind: UsageKind, value: f64) -> UsageRecord {
        let metric = match kind {
            UsageKind::Gauge => "test.gauge",
            UsageKind::Counter => "test.counter",
        }
        .to_owned();
        UsageRecord {
            metric,
            kind,
            value,
            ..self.record()
        }
    }

    fn conn(&self) -> modkit_db::DbConn<'_> {
        self.db.conn().unwrap()
    }
}

// ── Expiry ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_expired_authorization() {
    let f = Fixture::build_with_config(
        "ap_expired",
        UsageEmitterConfig {
            authorization_max_age: Duration::from_nanos(1),
            ..Default::default()
        },
    )
    .await;
    let err = f
        .emitter
        .enqueue_in(&f.conn(), f.record())
        .await
        .unwrap_err();
    assert!(matches!(err, UsageEmitterError::AuthorizationExpired));
}

#[tokio::test]
async fn enqueue_accepts_usage_record() {
    let f = Fixture::build("ap_pos_val").await;
    f.emitter.enqueue_in(&f.conn(), f.record()).await.unwrap();
}

// ── Authorization scope ───────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_mismatched_tenant() {
    let f = Fixture::build("ap_bad_tenant").await;
    let record = UsageRecord {
        tenant_id: Uuid::new_v4(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_resource_id() {
    let f = Fixture::build("ap_bad_res").await;
    let record = UsageRecord {
        resource_id: Uuid::new_v4(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_resource_type() {
    let f = Fixture::build("ap_bad_rt").await;
    let record = UsageRecord {
        resource_type: "other.resource".to_owned(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::AuthorizationFailed { .. }));
}

// ── Counter value ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_negative_counter_value() {
    let f = Fixture::build("ap_neg_ctr").await;
    let record = f.record_with_kind(UsageKind::Counter, -1.0);
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(
        err,
        UsageEmitterError::NegativeCounterValue { value } if (value + 1.0).abs() < f64::EPSILON
    ));
}

#[tokio::test]
async fn enqueue_accepts_zero_counter_value() {
    let f = Fixture::build("ap_zero_ctr").await;
    f.emitter
        .enqueue_in(&f.conn(), f.record_with_kind(UsageKind::Counter, 0.0))
        .await
        .unwrap();
}

#[tokio::test]
async fn enqueue_accepts_negative_gauge_value() {
    let f = Fixture::build("ap_neg_gauge").await;
    f.emitter
        .enqueue_in(&f.conn(), f.record_with_kind(UsageKind::Gauge, -1.0))
        .await
        .unwrap();
}

// ── Metric validation ─────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_metric_not_in_allowed_list() {
    let f = Fixture::build("ap_metric_disallowed").await;
    let record = UsageRecord {
        metric: "not.allowed.metric".to_owned(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(
        matches!(err, UsageEmitterError::MetricNotAllowed { ref metric } if metric == "not.allowed.metric")
    );
}

#[tokio::test]
async fn enqueue_rejects_metric_kind_mismatch() {
    let f = Fixture::build("ap_kind_mismatch").await;
    let record = UsageRecord {
        metric: "test.counter".to_owned(),
        kind: UsageKind::Gauge,
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(
        err,
        UsageEmitterError::MetricKindMismatch {
            ref metric,
            expected: UsageKind::Counter,
            actual: UsageKind::Gauge,
        } if metric == "test.counter"
    ));
}

// ── Module and subject mismatch rejection ─────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_mismatched_module() {
    let f = Fixture::build("ap_bad_module").await;
    let record = UsageRecord {
        module: "wrong-module".to_owned(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidRecord { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_subject_id() {
    let f = Fixture::build("ap_bad_subj_id").await;
    let record = UsageRecord {
        subject_id: Some(Uuid::new_v4()),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidRecord { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_subject_type() {
    let f = Fixture::build("ap_bad_subj_type").await;
    let record = UsageRecord {
        subject_type: Some("other.subject".to_owned()),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidRecord { .. }));
}

#[tokio::test]
async fn enqueue_accepts_record_when_module_and_subject_match_token() {
    let f = Fixture::build("ap_match_ok").await;
    f.emitter.enqueue_in(&f.conn(), f.record()).await.unwrap();
}
