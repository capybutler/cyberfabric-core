use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::{
    LeasedMessageHandler, MessageResult, Outbox, OutboxHandle, OutboxMessage, Partitions,
    outbox_migrations,
};
use modkit_db::{ConnectOpts, Db, connect_db};
use usage_collector_sdk::models::{Subject, UsageKind, UsageRecord};
use uuid::Uuid;

use super::UsageEmitter;
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
    emitter: UsageEmitter,
    tenant: Uuid,
    resource_id: Uuid,
}

impl Fixture {
    async fn build(name: &str) -> Self {
        Self::build_with_config(name, UsageEmitterConfig::default()).await
    }

    async fn build_with_config(name: &str, config: UsageEmitterConfig) -> Self {
        Self::build_with(name, config, 8192).await
    }

    async fn build_with_max_metadata_bytes(name: &str, max_metadata_bytes: u32) -> Self {
        Self::build_with(name, UsageEmitterConfig::default(), max_metadata_bytes).await
    }

    async fn build_with(name: &str, config: UsageEmitterConfig, max_metadata_bytes: u32) -> Self {
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
        let emitter = UsageEmitter {
            config: Arc::new(config),
            db: db.clone(),
            outbox,
            module: "test-module".to_owned(),
            tenant_id: tenant,
            resource_id,
            resource_type: FIXTURE_RESOURCE_TYPE.to_owned(),
            allowed_metrics,
            max_metadata_bytes,
            subject: Some(Subject::with_type(Uuid::nil(), "test.subject")),
            issued_at: Instant::now(),
        };
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
            subject: Some(Subject::with_type(Uuid::nil(), "test.subject")),
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
    assert!(matches!(err, UsageEmitterError::Unauthenticated { .. }));
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
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_resource_id() {
    let f = Fixture::build("ap_bad_res").await;
    let record = UsageRecord {
        resource_id: Uuid::new_v4(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_resource_type() {
    let f = Fixture::build("ap_bad_rt").await;
    let record = UsageRecord {
        resource_type: "other.resource".to_owned(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

// ── Counter value ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_negative_counter_value() {
    let f = Fixture::build("ap_neg_ctr").await;
    let record = f.record_with_kind(UsageKind::Counter, -1.0);
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidArgument { .. }));
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
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
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
    assert!(matches!(err, UsageEmitterError::InvalidArgument { .. }));
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
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_subject_id() {
    let f = Fixture::build("ap_bad_subj_id").await;
    let record = UsageRecord {
        subject: Some(Subject::with_type(Uuid::new_v4(), "test.subject")), // differs from Uuid::nil() in the token
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn enqueue_rejects_mismatched_subject_type() {
    let f = Fixture::build("ap_bad_subj_type").await;
    let record = UsageRecord {
        subject: Some(Subject::with_type(Uuid::nil(), "other.subject")), // differs from "test.subject" in the token
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn enqueue_accepts_record_when_module_and_subject_match_token() {
    let f = Fixture::build("ap_match_ok").await;
    // f.record() already has module="test-module" and subject=Some(Subject::with_type(Uuid::nil(), "test.subject"))
    f.emitter.enqueue_in(&f.conn(), f.record()).await.unwrap();
}

// ── Counter idempotency key ───────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_counter_record_with_empty_idempotency_key() {
    let f = Fixture::build("ap_ctr_empty_key").await;
    let record = UsageRecord {
        metric: "test.counter".to_owned(),
        kind: UsageKind::Counter,
        idempotency_key: String::new(),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidArgument { .. }));
}

// ── Metadata size ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_rejects_record_with_oversized_metadata() {
    let f = Fixture::build("ap_metadata_large").await;
    let record = UsageRecord {
        metadata: Some(serde_json::Value::String("x".repeat(8193))),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    assert!(matches!(err, UsageEmitterError::InvalidArgument { .. }));
}

#[tokio::test]
async fn enqueue_rejects_metadata_above_configured_limit_below_default() {
    // 2 KiB payload against a 1024-byte limit: must be rejected with the
    // configured limit reflected in the error string.
    let f = Fixture::build_with_max_metadata_bytes("ap_metadata_limit_1024", 1024).await;
    let record = UsageRecord {
        metadata: Some(serde_json::Value::String("x".repeat(2048))),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    let UsageEmitterError::InvalidArgument { detail, .. } = &err else {
        panic!("expected InvalidArgument, got {err:?}");
    };
    assert!(
        detail.contains("exceeds the 1024-byte limit"),
        "error must reference the configured limit (1024); got `{detail}`"
    );
}

#[tokio::test]
async fn enqueue_accepts_metadata_below_configured_limit_above_default() {
    // 9 KiB payload would be rejected at the old hardcoded 8192 limit but
    // must be accepted when the configured limit is 16 384.
    let f = Fixture::build_with_max_metadata_bytes("ap_metadata_limit_16k", 16_384).await;
    let record = UsageRecord {
        metadata: Some(serde_json::Value::String("x".repeat(9 * 1024))),
        ..f.record()
    };
    f.emitter.enqueue_in(&f.conn(), record).await.unwrap();
}

#[tokio::test]
async fn enqueue_rejects_any_non_none_metadata_when_limit_is_zero() {
    // `max_metadata_bytes == 0` means metadata is disabled. Even
    // `Value::Null` (which serializes as `b"null"`, four bytes) exceeds 0
    // and must be rejected with `exceeds the 0-byte limit`.
    let f = Fixture::build_with_max_metadata_bytes("ap_metadata_limit_zero_null", 0).await;
    let record = UsageRecord {
        metadata: Some(serde_json::Value::Null),
        ..f.record()
    };
    let err = f.emitter.enqueue_in(&f.conn(), record).await.unwrap_err();
    let UsageEmitterError::InvalidArgument { detail, .. } = &err else {
        panic!("expected InvalidArgument, got {err:?}");
    };
    assert!(
        detail.contains("exceeds the 0-byte limit"),
        "error must reference the configured limit (0); got `{detail}`"
    );
}

#[tokio::test]
async fn enqueue_accepts_record_without_metadata_when_limit_is_zero() {
    // `metadata: None` carries no payload, so `max_metadata_bytes == 0`
    // must still accept the record — the disabled-metadata config does not
    // bar records that simply omit metadata.
    let f = Fixture::build_with_max_metadata_bytes("ap_metadata_limit_zero_none", 0).await;
    let record = UsageRecord {
        metadata: None,
        ..f.record()
    };
    f.emitter.enqueue_in(&f.conn(), record).await.unwrap();
}
