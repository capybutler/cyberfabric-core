use std::time::Duration;

use super::UsageCollectorRestClientConfig;

#[test]
fn serde_defaults_apply_for_base_url_and_timeout() {
    let json = r#"{"client_id": "svc", "client_secret": "secret"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.base_url, "http://127.0.0.1:8080");
    assert_eq!(cfg.request_timeout, Duration::from_secs(30));
}

#[test]
fn base_url_can_be_overridden_via_serde() {
    let json =
        r#"{"client_id": "svc", "client_secret": "secret", "base_url": "http://collector:9090"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.base_url, "http://collector:9090");
}

#[test]
fn request_timeout_parses_humantime_duration() {
    let json = r#"{"client_id": "svc", "client_secret": "secret", "request_timeout": "10s"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.request_timeout, Duration::from_secs(10));
}

#[test]
fn scopes_default_to_empty() {
    let json = r#"{"client_id": "svc", "client_secret": "secret"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.scopes.is_empty());
}

#[test]
fn scopes_can_be_set_via_serde() {
    let json = r#"{"client_id": "svc", "client_secret": "secret", "scopes": ["read:usage", "write:usage"]}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.scopes, ["read:usage", "write:usage"]);
}

#[test]
fn client_id_is_required() {
    let json = r#"{"client_secret": "secret"}"#;
    assert!(serde_json::from_str::<UsageCollectorRestClientConfig>(json).is_err());
}

#[test]
fn client_secret_is_required() {
    let json = r#"{"client_id": "svc"}"#;
    assert!(serde_json::from_str::<UsageCollectorRestClientConfig>(json).is_err());
}

#[test]
fn rejects_unknown_fields() {
    let json = r#"{"client_id": "svc", "client_secret": "secret", "extra": true}"#;
    assert!(serde_json::from_str::<UsageCollectorRestClientConfig>(json).is_err());
}
