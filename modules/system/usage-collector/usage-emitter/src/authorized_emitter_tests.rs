use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::{
    LeasedMessageHandler, MessageResult, Outbox, OutboxHandle, OutboxMessage, Partitions,
    outbox_migrations,
};
use modkit_db::{ConnectOpts, Db, connect_db};
use usage_collector_sdk::models::{UsageKind, UsageRecord};
use uuid::Uuid;

use super::AuthorizedUsageEmitter;
use crate::config::UsageEmitterConfig;
use crate::error::UsageEmitterError;

// ── Infrastructure ────────────────────────────────────────────────────────────

struct NoopHandler;

#[async_trait]
impl LeasedMessageHandler for NoopHandler {
    async fn handle(&self, _msg: &OutboxMessage) -> MessageResult {
        MessageResult::Ok
    }
}

async fn build_db(name: &str) -> Db {
    let url = format!("sqlite:file:{name}?mode=memory&cache=shared");
    let db = connect_db(
        &url,
        ConnectOpts {
            max_conns: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    run_migrations_for_testing(&db, outbox_migrations())
        .await
        .unwrap();
    db
}

async fn build_outbox(db: Db) -> OutboxHandle {
    let cfg = UsageEmitterConfig::default();
    Outbox::builder(db)
        .queue(
            cfg.outbox_queue.as_str(),
            Partitions::of(cfg.outbox_partition_count),
        )
        .leased(NoopHandler)
        .start()
        .await
        .unwrap()
}

const FIXTURE_RESOURCE_TYPE: &str = "test.resource";

// ── Test fixture ──────────────────────────────────────────────────────────────

struct Fixture {
    db: Db,
    _handle: OutboxHandle,
    emitter: AuthorizedUsageEmitter,
    tenant: Uuid,
    resource_id: Uuid,
}

impl Fixture {
    async fn build(name: &str) -> Self {
        Self::build_with_config(name, UsageEmitterConfig::default()).await
    }

    async fn build_with_config(name: &str, config: UsageEmitterConfig) -> Self {
        use usage_collector_sdk::models::AllowedMetric;

        let db = build_db(name).await;
        let handle = build_outbox(db.clone()).await;
        let outbox = Arc::clone(handle.outbox());
        let tenant = Uuid::new_v4();
        let resource_id = Uuid::new_v4();
        let allowed_metrics = vec![
            AllowedMetric {
                name: "test.gauge".to_owned(),
                kind: UsageKind::Gauge,
            },
            AllowedMetric {
                name: "test.counter".to_owned(),
                kind: UsageKind::Counter,
            },
        ];
        let emitter = AuthorizedUsageEmitter::new(
            Arc::new(config),
            db.clone(),
            outbox,
            "test-module".to_owned(),
            tenant,
            resource_id,
            FIXTURE_RESOURCE_TYPE.to_owned(),
            allowed_metrics,
            Uuid::nil(),
            "test.subject".to_owned(),
        );
        Self {
            db,
            _handle: handle,
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
            subject_id: Uuid::nil(),
            subject_type: "test.subject".to_owned(),
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
            authorization_max_age: Duration::ZERO,
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
    // "test.counter" is registered as Counter; submitting it as Gauge is a kind mismatch.
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
