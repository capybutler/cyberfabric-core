use url::Url;

use super::{UsageCollectorRestClientConfig, is_insecure_non_loopback_http};

const CLIENT_ID: &str = "CLIENT_ID";
const CLIENT_SECRET: &str = "CLIENT_SECRET";

fn valid_cfg_json() -> serde_json::Value {
    serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET
        }
    })
}

#[test]
fn collector_url_is_parsed_as_url() {
    let json = serde_json::json!({
        "collector_url": "http://collector:9090",
        "oauth": {"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET}
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    assert_eq!(cfg.collector_url.host_str(), Some("collector"));
    assert_eq!(cfg.collector_url.port(), Some(9090));
}

#[test]
fn scopes_default_to_empty() {
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(valid_cfg_json()).unwrap();
    assert!(cfg.oauth.scopes.is_empty());
}

#[test]
fn scopes_can_be_set_via_serde() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {
            "client_id": CLIENT_ID,
            "client_secret": CLIENT_SECRET,
            "scopes": ["read:usage", "write:usage"]
        }
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    assert_eq!(cfg.oauth.scopes, ["read:usage", "write:usage"]);
}

#[test]
fn collector_url_is_required() {
    let json = serde_json::json!({
        "oauth": {"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET}
    });
    assert!(serde_json::from_value::<UsageCollectorRestClientConfig>(json).is_err());
}

#[test]
fn client_id_is_required() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_secret": CLIENT_SECRET}
    });
    assert!(serde_json::from_value::<UsageCollectorRestClientConfig>(json).is_err());
}

#[test]
fn client_secret_is_required() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_id": CLIENT_ID}
    });
    assert!(serde_json::from_value::<UsageCollectorRestClientConfig>(json).is_err());
}

#[test]
fn rejects_unknown_fields() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET},
        "extra": true
    });
    assert!(serde_json::from_value::<UsageCollectorRestClientConfig>(json).is_err());
}

// validate: collector_url checks

#[test]
fn validate_rejects_non_hierarchical_collector_url() {
    // "cannot-be-a-base" (opaque) URLs like data: have no host or path hierarchy.
    let json = serde_json::json!({
        "collector_url": "data:text/plain,hello",
        "oauth": {"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET}
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("hierarchical"),
        "error must mention hierarchical URL, got: {err}"
    );
}

// validate: S2S credential checks

#[test]
fn validate_rejects_empty_client_id() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_id": "", "client_secret": CLIENT_SECRET}
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_id"),
        "error must mention client_id, got: {err}"
    );
}

#[test]
fn validate_rejects_whitespace_only_client_id() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_id": "   ", "client_secret": CLIENT_SECRET}
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_id"),
        "error must mention client_id, got: {err}"
    );
}

#[test]
fn validate_rejects_empty_client_secret() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_id": CLIENT_ID, "client_secret": ""}
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_secret"),
        "error must mention client_secret, got: {err}"
    );
}

#[test]
fn validate_rejects_whitespace_only_client_secret() {
    let json = serde_json::json!({
        "collector_url": "http://127.0.0.1:8080",
        "oauth": {"client_id": CLIENT_ID, "client_secret": "   "}
    });
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(json).unwrap();
    let err = cfg.validate().unwrap_err();
    assert!(
        err.to_string().contains("client_secret"),
        "error must mention client_secret, got: {err}"
    );
}

#[test]
fn validate_accepts_valid_credentials() {
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(valid_cfg_json()).unwrap();
    assert!(cfg.validate().is_ok());
}

#[test]
fn debug_output_redacts_oauth_credentials() {
    let cfg: UsageCollectorRestClientConfig = serde_json::from_value(valid_cfg_json()).unwrap();
    let debug = format!("{cfg:?}");
    assert!(
        !debug.contains(CLIENT_ID),
        "Debug output must not contain client_id value, got: {debug}"
    );
    assert!(
        !debug.contains(CLIENT_SECRET),
        "Debug output must not contain client_secret value, got: {debug}"
    );
    assert!(
        debug.contains("[REDACTED]"),
        "Debug output must contain [REDACTED], got: {debug}"
    );
}

// cpt-cf-dod-rest-ingest-tls-config: TLS/HTTPS startup check

#[test]
fn test_http_non_localhost_emits_warn_or_fails() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http:// with a non-loopback host MUST be flagged as insecure so the
    // module initialisation can emit a WARN or return an error.
    assert!(
        is_insecure_non_loopback_http(&Url::parse("http://example.com").unwrap()),
        "http://example.com must be detected as insecure (non-loopback http)"
    );
    assert!(
        is_insecure_non_loopback_http(&Url::parse("http://example.com:8080").unwrap()),
        "http://example.com:8080 must be detected as insecure"
    );
}

#[test]
fn test_http_localhost_is_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http://localhost is a permitted loopback address; must not be flagged.
    assert!(
        !is_insecure_non_loopback_http(&Url::parse("http://localhost:8080").unwrap()),
        "http://localhost:8080 is a loopback address and must NOT be flagged as insecure"
    );
    assert!(
        !is_insecure_non_loopback_http(&Url::parse("http://localhost").unwrap()),
        "http://localhost must NOT be flagged as insecure"
    );
}

#[test]
fn test_http_127_0_0_1_is_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http://127.0.0.1 is a loopback address; must not be flagged as insecure.
    assert!(
        !is_insecure_non_loopback_http(&Url::parse("http://127.0.0.1:8080").unwrap()),
        "http://127.0.0.1:8080 is a loopback address and must NOT be flagged as insecure"
    );
}

#[test]
fn test_https_always_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // https:// with any host (including non-localhost) must NOT be flagged as
    // insecure — TLS is always acceptable.
    assert!(
        !is_insecure_non_loopback_http(&Url::parse("https://example.com").unwrap()),
        "https://example.com must NOT be flagged as insecure"
    );
    assert!(
        !is_insecure_non_loopback_http(&Url::parse("https://collector.internal:443").unwrap()),
        "https://collector.internal:443 must NOT be flagged as insecure"
    );
}

#[test]
fn test_http_ipv6_loopback_is_allowed() {
    // cpt-cf-dod-rest-ingest-tls-config
    // http://[::1] is the IPv6 loopback address; must not be flagged as insecure.
    assert!(
        !is_insecure_non_loopback_http(&Url::parse("http://[::1]:8080").unwrap()),
        "http://[::1]:8080 is the IPv6 loopback and must NOT be flagged as insecure"
    );
}
