#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use authz_resolver_sdk::models::{EvaluationRequest, EvaluationResponse, EvaluationResponseContext};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError};
use chrono::Utc;
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::{
    LeasedMessageHandler, MessageResult, Outbox, OutboxHandle, OutboxMessage, Partitions,
    outbox_migrations,
};
use modkit_db::{ConnectOpts, Db, connect_db};
use modkit_security::SecurityContext;
use usage_collector_sdk::models::{UsageKind, UsageRecord};
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use usage_emitter::UsageEmitterConfig;
use uuid::Uuid;

// ── Outbox helpers ────────────────────────────────────────────────────────────

struct NoopHandler;

#[async_trait]
impl LeasedMessageHandler for NoopHandler {
    async fn handle(&self, _msg: &OutboxMessage) -> MessageResult {
        MessageResult::Ok
    }
}

pub async fn build_db(name: &str) -> Db {
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

pub async fn build_outbox(db: Db) -> OutboxHandle {
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

// ── Mock collectors ───────────────────────────────────────────────────────────

enum CollectorOutcome {
    Ok,
    Transient,
    Permanent,
    Unavailable,
}

pub struct MockCollector {
    outcome: CollectorOutcome,
}

impl MockCollector {
    pub fn ok() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Ok,
        })
    }

    pub fn transient() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Transient,
        })
    }

    pub fn permanent() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Permanent,
        })
    }

    pub fn unavailable() -> Arc<Self> {
        Arc::new(Self {
            outcome: CollectorOutcome::Unavailable,
        })
    }
}

#[async_trait]
impl UsageCollectorClientV1 for MockCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        match self.outcome {
            CollectorOutcome::Ok => Ok(()),
            CollectorOutcome::Transient => Err(UsageCollectorError::plugin_timeout()),
            CollectorOutcome::Permanent => {
                Err(UsageCollectorError::authorization_failed("permanent"))
            }
            CollectorOutcome::Unavailable => {
                Err(UsageCollectorError::unavailable("connection refused"))
            }
        }
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![],
        })
    }
}

pub struct NoopCollector;

#[async_trait]
impl UsageCollectorClientV1 for NoopCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![],
        })
    }
}

pub struct CapturingCollector {
    pub captured: Arc<Mutex<Option<UsageRecord>>>,
}

impl CapturingCollector {
    pub fn new(captured: Arc<Mutex<Option<UsageRecord>>>) -> Arc<Self> {
        Arc::new(Self { captured })
    }
}

#[async_trait]
impl UsageCollectorClientV1 for CapturingCollector {
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        *self.captured.lock().unwrap() = Some(record);
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![],
        })
    }
}

// ── Mock AuthZ ────────────────────────────────────────────────────────────────

pub struct AllowAllAuthZ;

#[async_trait]
impl AuthZResolverClient for AllowAllAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Ok(EvaluationResponse {
            decision: true,
            context: EvaluationResponseContext::default(),
        })
    }
}

pub struct DenyAllAuthZ;

#[async_trait]
impl AuthZResolverClient for DenyAllAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Ok(EvaluationResponse {
            decision: false,
            context: EvaluationResponseContext::default(),
        })
    }
}

pub struct FailingAuthZ;

#[async_trait]
impl AuthZResolverClient for FailingAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Err(AuthZResolverError::Internal("PDP unavailable".to_owned()))
    }
}

// ── Record / message helpers ──────────────────────────────────────────────────

pub fn valid_usage_record() -> UsageRecord {
    UsageRecord {
        tenant_id: Uuid::new_v4(),
        module: "test-module".to_owned(),
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

pub fn make_msg(payload_bytes: Vec<u8>) -> OutboxMessage {
    OutboxMessage {
        partition_id: 0,
        seq: 1,
        payload: payload_bytes,
        payload_type: "application/json".to_owned(),
        created_at: Utc::now(),
        attempts: 0,
    }
}

pub fn make_ctx() -> SecurityContext {
    SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .unwrap()
}
