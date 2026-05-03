use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::{
    LeasedMessageHandler, MessageResult, Outbox, OutboxHandle, OutboxMessage, Partitions,
    outbox_migrations,
};
use modkit_db::{ConnectOpts, Db, connect_db};
use usage_collector_sdk::models::{AllowedMetric, UsageKind, UsageRecord};
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
    tenant_id: Uuid,
    resource_id: Uuid,
}

impl Fixture {
    async fn build(name: &str) -> Self {
        let db = build_db(name).await;
        let handle = build_outbox(db.clone()).await;
        let tenant_id = Uuid::new_v4();
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
            Arc::new(UsageEmitterConfig::default()),
            db.clone(),
            Arc::clone(handle.outbox()),
            "test-module".to_owned(),
            tenant_id,
            resource_id,
            "test.resource".to_owned(),
            allowed_metrics,
            Some(Uuid::nil()),
            Some("test.subject".to_owned()),
        );
        Self {
            db,
            _handle: handle,
            emitter,
            tenant_id,
            resource_id,
        }
    }

    fn conn(&self) -> modkit_db::DbConn<'_> {
        self.db.conn().unwrap()
    }

    /// Build a valid `UsageRecord` that matches the authorized token fields.
    fn record(&self) -> UsageRecord {
        UsageRecord {
            tenant_id: self.tenant_id,
            module: "test-module".to_owned(),
            metric: "test.gauge".to_owned(),
            kind: UsageKind::Gauge,
            value: 1.0,
            resource_id: self.resource_id,
            resource_type: "test.resource".to_owned(),
            subject_id: Some(Uuid::nil()),
            subject_type: Some("test.subject".to_owned()),
            idempotency_key: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            metadata: None,
        }
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

// ── Blank idempotency key handling ────────────────────────────────────────────

#[tokio::test]
async fn test_counter_with_blank_idempotency_key_is_rejected() {
    let f = Fixture::build("ub_counter_blank_idem").await;
    let err = f
        .emitter
        .build_usage_record("test.counter", 1.0)
        .with_idempotency_key("")
        .enqueue_in(&f.conn())
        .await
        .unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidRecord { .. }));
}

#[tokio::test]
async fn test_gauge_with_blank_idempotency_key_uses_uuid_fallback() {
    use std::sync::{Arc, Mutex};
    use usage_collector_sdk::models::UsageRecord;

    struct CaptureHandler {
        captured: Arc<Mutex<Option<Vec<u8>>>>,
    }

    #[async_trait::async_trait]
    impl LeasedMessageHandler for CaptureHandler {
        async fn handle(&self, msg: &OutboxMessage) -> MessageResult {
            let mut guard = self.captured.lock().unwrap();
            *guard = Some(msg.payload.clone());
            MessageResult::Ok
        }
    }

    let name = "ub_gauge_blank_idem";
    let url = format!("sqlite:file:{name}?mode=memory&cache=shared");
    let db = modkit_db::connect_db(
        &url,
        modkit_db::ConnectOpts {
            max_conns: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    modkit_db::migration_runner::run_migrations_for_testing(
        &db,
        modkit_db::outbox::outbox_migrations(),
    )
    .await
    .unwrap();

    let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let cfg = crate::config::UsageEmitterConfig::default();
    let handle = Outbox::builder(db.clone())
        .queue(
            cfg.outbox_queue.as_str(),
            Partitions::of(cfg.outbox_partition_count),
        )
        .leased(CaptureHandler {
            captured: Arc::clone(&captured),
        })
        .start()
        .await
        .unwrap();

    let allowed_metrics = vec![usage_collector_sdk::models::AllowedMetric {
        name: "test.gauge".to_owned(),
        kind: usage_collector_sdk::models::UsageKind::Gauge,
    }];
    let emitter = crate::authorized_emitter::AuthorizedUsageEmitter::new(
        Arc::new(cfg),
        db.clone(),
        Arc::clone(handle.outbox()),
        "test-module".to_owned(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "test.resource".to_owned(),
        allowed_metrics,
        Some(Uuid::nil()),
        Some("test.subject".to_owned()),
    );

    {
        let conn = db.conn().unwrap();
        emitter
            .build_usage_record("test.gauge", 1.0)
            .with_idempotency_key("")
            .enqueue_in(&conn)
            .await
            .unwrap();
    }

    // Wait for the outbox to deliver the message to the handler.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        {
            let guard = captured.lock().unwrap();
            if guard.is_some() {
                break;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for outbox delivery"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let payload = captured.lock().unwrap().take().unwrap();
    let record: UsageRecord = serde_json::from_slice(&payload).unwrap();
    assert!(
        record.idempotency_key.is_empty(),
        "gauge records must have empty idempotency_key so storage can store NULL"
    );
}

// ── Authorization scope mismatch rejection ────────────────────────────────────

#[tokio::test]
async fn builder_rejects_mismatched_module_name() {
    let f = Fixture::build("ub_bad_module").await;
    let record = UsageRecord {
        module: "wrong-module".to_owned(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(
        matches!(err, crate::error::UsageEmitterError::InvalidRecord { .. }),
        "expected InvalidRecord for mismatched module, got {err:?}"
    );
}

#[tokio::test]
async fn builder_rejects_mismatched_subject_id() {
    let f = Fixture::build("ub_bad_subj_id").await;
    let record = UsageRecord {
        subject_id: Some(Uuid::new_v4()), // differs from Some(Uuid::nil()) in the token
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(
        matches!(err, crate::error::UsageEmitterError::InvalidRecord { .. }),
        "expected InvalidRecord for mismatched subject_id, got {err:?}"
    );
}

#[tokio::test]
async fn builder_rejects_mismatched_subject_type() {
    let f = Fixture::build("ub_bad_subj_type").await;
    let record = UsageRecord {
        subject_type: Some("other.subject".to_owned()), // differs from Some("test.subject") in the token
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(
        matches!(err, crate::error::UsageEmitterError::InvalidRecord { .. }),
        "expected InvalidRecord for mismatched subject_type, got {err:?}"
    );
}

#[tokio::test]
async fn builder_enqueues_record_with_subject() {
    // The current Fixture always has subject_id/subject_type set (via Uuid::nil() / "test.subject").
    // After Phase 5, enqueue_in produces Some(subject_id) / Some(subject_type) from the emitter,
    // and the record built by Fixture::record() matches — enqueue must succeed.
    let f = Fixture::build("ub_with_subject").await;
    f.emitter
        .build_usage_record("test.gauge", 1.0)
        .enqueue_in(&f.conn())
        .await
        .unwrap();
}

#[tokio::test]
async fn builder_enqueues_record_without_subject() {
    // Exercises the no-subject path: both subject_id and subject_type are None.
    let db = build_db("ub_without_subject").await;
    let handle = build_outbox(db.clone()).await;
    let tenant_id = Uuid::new_v4();
    let resource_id = Uuid::new_v4();
    let allowed_metrics = vec![AllowedMetric {
        name: "test.gauge".to_owned(),
        kind: UsageKind::Gauge,
    }];
    let emitter = AuthorizedUsageEmitter::new(
        Arc::new(UsageEmitterConfig::default()),
        db.clone(),
        Arc::clone(handle.outbox()),
        "test-module".to_owned(),
        tenant_id,
        resource_id,
        "test.resource".to_owned(),
        allowed_metrics,
        None, // subject_id absent
        None, // subject_type absent
    );
    // A record with no subject fields must match the None-subject token.
    let record = UsageRecord {
        tenant_id,
        module: "test-module".to_owned(),
        metric: "test.gauge".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id,
        resource_type: "test.resource".to_owned(),
        subject_id: None,
        subject_type: None,
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        metadata: None,
    };
    let conn = db.conn().unwrap();
    emitter
        .enqueue_in(&conn, record)
        .await
        .expect("enqueue must succeed when subject is absent in both token and record");
}
