//! Unit tests for REST handlers and `emitter_error_to_problem`.

use std::sync::Arc;

use async_trait::async_trait;
use axum::Extension;
use axum::extract::Path;
use http::StatusCode;
use usage_collector_sdk::{
    AllowedMetric, ModuleConfig, UsageCollectorClientV1, UsageCollectorError, UsageKind,
    UsageRecord,
};
use usage_emitter::UsageEmitterError;

use super::emitter_error_to_problem;
use super::handle_get_module_config;

// ── emitter_error_to_problem ──────────────────────────────────────

#[test]
fn authorization_failed_maps_to_forbidden() {
    let err = UsageEmitterError::authorization_failed("pdp denied");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::FORBIDDEN);
    assert_eq!(p.detail, "pdp denied");
}

#[test]
fn authorization_expired_maps_to_forbidden() {
    let err = UsageEmitterError::authorization_expired();
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::FORBIDDEN);
}

#[test]
fn invalid_record_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::invalid_record("missing metric");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(p.detail, "missing metric");
}

#[test]
fn metric_not_allowed_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::metric_not_allowed("cpu.usage");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        p.detail.contains("cpu.usage"),
        "detail should name the metric"
    );
}

#[test]
fn metric_kind_mismatch_maps_to_unprocessable_entity() {
    let err =
        UsageEmitterError::metric_kind_mismatch("req.count", UsageKind::Counter, UsageKind::Gauge);
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(p.detail.contains("req.count"));
}

#[test]
fn negative_counter_value_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::negative_counter_value(-1.5);
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(p.detail.contains("-1.5"));
}

#[test]
fn internal_error_maps_to_500() {
    let err = UsageEmitterError::internal("something broke");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn outbox_error_maps_to_500() {
    let err = UsageEmitterError::Outbox(modkit_db::outbox::OutboxError::QueueNotRegistered(
        "usage".to_owned(),
    ));
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ── handle_get_module_config ──────────────────────────────────────

struct MockCollector {
    config: ModuleConfig,
}

#[async_trait]
impl UsageCollectorClientV1 for MockCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Ok(self.config.clone())
    }
}

struct FailingCollector;

#[async_trait]
impl UsageCollectorClientV1 for FailingCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Err(UsageCollectorError::internal("unavailable"))
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Err(UsageCollectorError::internal("unavailable"))
    }
}

struct NotFoundCollector;

#[async_trait]
impl UsageCollectorClientV1 for NotFoundCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        Err(UsageCollectorError::module_not_found(module_name))
    }
}

#[tokio::test]
async fn get_module_config_handler_returns_allowed_metrics() {
    let collector = Arc::new(MockCollector {
        config: ModuleConfig {
            allowed_metrics: vec![AllowedMetric {
                name: "cpu.usage".to_owned(),
                kind: UsageKind::Gauge,
            }],
        },
    }) as Arc<dyn UsageCollectorClientV1>;

    let result = handle_get_module_config(Path("my-module".to_owned()), Extension(collector)).await;

    let axum::Json(resp) = result.expect("handler should succeed");
    assert_eq!(resp.allowed_metrics.len(), 1);
    assert_eq!(resp.allowed_metrics[0].name, "cpu.usage");
}

#[tokio::test]
async fn get_module_config_handler_propagates_collector_error_as_500() {
    let collector = Arc::new(FailingCollector) as Arc<dyn UsageCollectorClientV1>;

    let result = handle_get_module_config(Path("my-module".to_owned()), Extension(collector)).await;

    let err = result.expect_err("handler should fail");
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn get_module_config_handler_returns_404_for_unknown_module() {
    let collector = Arc::new(NotFoundCollector) as Arc<dyn UsageCollectorClientV1>;

    let result =
        handle_get_module_config(Path("unknown-module".to_owned()), Extension(collector)).await;

    let err = result.expect_err("handler should return 404");
    assert_eq!(err.status, StatusCode::NOT_FOUND);
    assert!(err.detail.contains("unknown-module"));
}

#[test]
fn module_not_configured_maps_to_unprocessable_entity() {
    let err = UsageEmitterError::module_not_configured("my-module");
    let p = emitter_error_to_problem(err);
    assert_eq!(p.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(p.detail.contains("my-module"));
}
