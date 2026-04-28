use std::time::Duration;

use super::UsageEmitterConfig;

#[test]
fn default_authorization_max_age_is_30_seconds() {
    let cfg = UsageEmitterConfig::default();
    assert_eq!(cfg.authorization_max_age, Duration::from_secs(30));
}

#[test]
fn default_outbox_queue_is_usage_records() {
    let cfg = UsageEmitterConfig::default();
    assert_eq!(cfg.outbox_queue, "usage-records");
}

#[test]
fn default_outbox_partition_count_is_4() {
    let cfg = UsageEmitterConfig::default();
    assert_eq!(cfg.outbox_partition_count, 4);
}

#[test]
fn validate_accepts_defaults() {
    UsageEmitterConfig::default().validate().unwrap();
}

#[test]
fn validate_rejects_empty_outbox_queue() {
    let cfg = UsageEmitterConfig {
        outbox_queue: "   ".to_owned(),
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("outbox_queue"));
}

#[test]
fn validate_rejects_invalid_outbox_partition_count_zero() {
    let cfg = UsageEmitterConfig {
        outbox_partition_count: 0,
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("outbox_partition_count"));
}

#[test]
fn validate_rejects_invalid_outbox_partition_count_not_power_of_two() {
    let cfg = UsageEmitterConfig {
        outbox_partition_count: 3,
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("outbox_partition_count"));
}

#[test]
fn validate_rejects_invalid_outbox_partition_count_above_64() {
    let cfg = UsageEmitterConfig {
        outbox_partition_count: 65,
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("outbox_partition_count"));
}

#[test]
fn validate_rejects_zero_authorization_max_age() {
    let cfg = UsageEmitterConfig {
        authorization_max_age: Duration::ZERO,
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("authorization_max_age"));
}

#[test]
fn default_outbox_backoff_max_is_10_minutes() {
    let cfg = UsageEmitterConfig::default();
    assert_eq!(cfg.outbox_backoff_max, Duration::from_mins(10));
}

#[test]
fn validate_rejects_zero_outbox_backoff_max() {
    let cfg = UsageEmitterConfig {
        outbox_backoff_max: Duration::ZERO,
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("outbox_backoff_max"));
}

#[test]
fn validate_rejects_outbox_backoff_max_at_or_above_15_minutes() {
    let cfg = UsageEmitterConfig {
        outbox_backoff_max: Duration::from_mins(15),
        ..UsageEmitterConfig::default()
    };
    let err = cfg.validate().unwrap_err();
    assert!(err.to_string().contains("outbox_backoff_max"));
}

#[test]
fn validate_accepts_outbox_backoff_max_below_15_minutes() {
    let cfg = UsageEmitterConfig {
        outbox_backoff_max: Duration::from_secs(899),
        ..UsageEmitterConfig::default()
    };
    cfg.validate().unwrap();
}
