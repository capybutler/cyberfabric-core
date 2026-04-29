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

// validate: S2S credential checks

#[test]
fn validate_rejects_empty_client_id() {
    let json = r#"{"client_id": "", "client_secret": "secret"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_id"),
        "error must mention client_id, got: {err}"
    );
}

#[test]
fn validate_rejects_whitespace_only_client_id() {
    let json = r#"{"client_id": "   ", "client_secret": "secret"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_id"),
        "error must mention client_id, got: {err}"
    );
}

#[test]
fn validate_rejects_empty_client_secret() {
    let json = r#"{"client_id": "svc", "client_secret": ""}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_secret"),
        "error must mention client_secret, got: {err}"
    );
}

#[test]
fn validate_rejects_whitespace_only_client_secret() {
    let json = r#"{"client_id": "svc", "client_secret": "   "}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_secret"),
        "error must mention client_secret, got: {err}"
    );
}

#[test]
fn validate_accepts_valid_credentials() {
    let json = r#"{"client_id": "svc", "client_secret": "secret"}"#;
    let cfg: UsageCollectorRestClientConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.validate().is_ok());
}

// cpt-cf-dod-rest-ingest-tls-config: TLS/HTTPS startup check

#[test]
fn test_http_non_localhost_emits_warn_or_fails() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http:// with a non-loopback host MUST be flagged as insecure so the
    // module initialisation can emit a WARN or return an error.
    assert!(
        super::is_insecure_non_loopback_http("http://example.com"),
        "http://example.com must be detected as insecure (non-loopback http)"
    );
    assert!(
        super::is_insecure_non_loopback_http("http://example.com:8080"),
        "http://example.com:8080 must be detected as insecure"
    );
}

#[test]
fn test_http_localhost_is_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http://localhost is a permitted loopback address; must not be flagged.
    assert!(
        !super::is_insecure_non_loopback_http("http://localhost:8080"),
        "http://localhost:8080 is a loopback address and must NOT be flagged as insecure"
    );
    assert!(
        !super::is_insecure_non_loopback_http("http://localhost"),
        "http://localhost must NOT be flagged as insecure"
    );
}

#[test]
fn test_http_127_0_0_1_is_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http://127.0.0.1 is a loopback address; must not be flagged as insecure.
    assert!(
        !super::is_insecure_non_loopback_http("http://127.0.0.1:8080"),
        "http://127.0.0.1:8080 is a loopback address and must NOT be flagged as insecure"
    );
}

#[test]
fn test_https_always_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // https:// with any host (including non-localhost) must NOT be flagged as
    // insecure — TLS is always acceptable.
    assert!(
        !super::is_insecure_non_loopback_http("https://example.com"),
        "https://example.com must NOT be flagged as insecure"
    );
    assert!(
        !super::is_insecure_non_loopback_http("https://collector.internal:443"),
        "https://collector.internal:443 must NOT be flagged as insecure"
    );
}

#[test]
fn test_http_ipv6_loopback_is_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http://[::1] is the IPv6 loopback address; must not be flagged as insecure.
    assert!(
        !super::is_insecure_non_loopback_http("http://[::1]:8080"),
        "http://[::1]:8080 is the IPv6 loopback and must NOT be flagged as insecure"
    );
}
