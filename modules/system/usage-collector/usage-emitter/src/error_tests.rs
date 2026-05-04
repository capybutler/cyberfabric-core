use usage_collector_sdk::models::UsageKind;

use super::*;

// ── Display ───────────────────────────────────────────────────────

#[test]
fn display_authorization_expired() {
    let err = UsageEmitterError::authorization_expired();
    assert_eq!(err.to_string(), "emit authorization token has expired");
}

#[test]
fn display_authorization_failed() {
    let err = UsageEmitterError::authorization_failed("denied by policy");
    assert_eq!(err.to_string(), "authorization failed: denied by policy");
}

#[test]
fn display_negative_counter_value() {
    let err = UsageEmitterError::negative_counter_value(-1.0);
    assert_eq!(
        err.to_string(),
        "counter usage record has a negative value: -1"
    );
}

#[test]
fn display_invalid_record() {
    let err = UsageEmitterError::invalid_record("missing tenant_id");
    assert_eq!(err.to_string(), "invalid usage record: missing tenant_id");
}

#[test]
fn display_metric_kind_mismatch() {
    let err =
        UsageEmitterError::metric_kind_mismatch("cpu.usage", UsageKind::Counter, UsageKind::Gauge);
    assert_eq!(
        err.to_string(),
        "metric 'cpu.usage' expects kind Counter but record specifies Gauge"
    );
}

#[test]
fn display_metric_not_allowed() {
    let err = UsageEmitterError::metric_not_allowed("unknown.metric");
    assert_eq!(
        err.to_string(),
        "metric not allowed for this module: unknown.metric"
    );
}

#[test]
fn display_internal() {
    let err = UsageEmitterError::internal("connection timeout");
    assert_eq!(err.to_string(), "internal error: connection timeout");
}
