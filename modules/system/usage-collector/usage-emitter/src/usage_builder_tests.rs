use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::{
    LeasedMessageHandler, MessageResult, Outbox, OutboxHandle, OutboxMessage, Partitions,
    outbox_migrations,
};
use modkit_db::{ConnectOpts, Db, connect_db};
use usage_collector_sdk::models::{AllowedMetric, UsageKind};
use uuid::Uuid;

use crate::authorized_emitter::AuthorizedUsageEmitter;
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

// ── Test fixture ──────────────────────────────────────────────────────────────

struct Fixture {
    db: Db,
    _handle: OutboxHandle,
    emitter: AuthorizedUsageEmitter,
}

impl Fixture {
    async fn build(name: &str) -> Self {
        let db = build_db(name).await;
        let handle = build_outbox(db.clone()).await;
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
            Arc::new(UsageEmitterConfig::default()),
            db.clone(),
            Arc::clone(handle.outbox()),
            "test-module".to_owned(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            "test.resource".to_owned(),
            allowed_metrics,
            Uuid::nil(),
            "test.subject".to_owned(),
        );
        Self {
            db,
            _handle: handle,
            emitter,
        }
    }

    fn conn(&self) -> modkit_db::DbConn<'_> {
        self.db.conn().unwrap()
    }
}

// ── Metric kind resolution ────────────────────────────────────────────────────

#[tokio::test]
async fn builder_enqueues_gauge_metric() {
    let f = Fixture::build("ub_gauge").await;
    f.emitter
        .build_usage_record("test.gauge", 42.0)
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}

#[tokio::test]
async fn builder_enqueues_counter_metric_with_positive_value() {
    let f = Fixture::build("ub_counter_pos").await;
    f.emitter
        .build_usage_record("test.counter", 1.0)
        .with_idempotency_key("idem-key")
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}

#[tokio::test]
async fn builder_rejects_counter_metric_with_negative_value() {
    let f = Fixture::build("ub_counter_neg").await;
    let err = f
        .emitter
        .build_usage_record("test.counter", -1.0)
        .with_idempotency_key("idem-key")
        .enqueue_in(&f.conn())
        .await
        .unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidRecord { .. }));
}

#[tokio::test]
async fn builder_rejects_counter_metric_without_idempotency_key() {
    let f = Fixture::build("ub_counter_no_idem").await;
    let err = f
        .emitter
        .build_usage_record("test.counter", 1.0)
        .enqueue_in(&f.conn())
        .await
        .unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidRecord { .. }));
}

#[tokio::test]
async fn builder_accepts_gauge_metric_with_negative_value() {
    let f = Fixture::build("ub_gauge_neg").await;
    f.emitter
        .build_usage_record("test.gauge", -5.0)
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}

#[tokio::test]
async fn builder_rejects_unknown_metric() {
    let f = Fixture::build("ub_unknown_metric").await;
    let err = f
        .emitter
        .build_usage_record("unknown.metric", 1.0)
        .enqueue_in(&f.conn())
        .await
        .unwrap_err();
    assert!(
        matches!(err, UsageEmitterError::MetricNotAllowed { ref metric } if metric == "unknown.metric")
    );
}

// ── Optional fields ───────────────────────────────────────────────────────────

#[tokio::test]
async fn builder_with_idempotency_key_enqueues_successfully() {
    let f = Fixture::build("ub_idem_key").await;
    f.emitter
        .build_usage_record("test.gauge", 1.0)
        .with_idempotency_key("my-stable-key")
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}

#[tokio::test]
async fn builder_with_timestamp_enqueues_successfully() {
    let f = Fixture::build("ub_timestamp").await;
    let ts = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    f.emitter
        .build_usage_record("test.gauge", 1.0)
        .with_timestamp(ts)
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}

#[tokio::test]
async fn builder_without_optional_fields_enqueues_successfully() {
    let f = Fixture::build("ub_defaults").await;
    f.emitter
        .build_usage_record("test.gauge", 0.0)
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}
