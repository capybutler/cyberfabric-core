//! Configuration for the REST `usage-collector-client` module.

use std::time::Duration;

use secrecy::SecretString;
use serde::Deserialize;
use usage_emitter::UsageEmitterConfig;

/// Module configuration.
#[derive(Debug, Clone, Deserialize, modkit_macros::ExpandVars)]
#[serde(deny_unknown_fields)]
pub struct UsageCollectorRestClientConfig {
    /// Base URL of the usage-collector REST service (no trailing slash).
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// `OAuth2` client identifier for s2s authentication.
    pub client_id: String,

    /// `OAuth2` client secret for s2s authentication.
    #[expand_vars]
    pub client_secret: SecretString,

    /// `OAuth2` scopes to request (empty = `IdP` default scopes).
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Per-request HTTP timeout.
    #[serde(
        default = "default_request_timeout",
        with = "modkit_utils::humantime_serde"
    )]
    pub request_timeout: Duration,

    /// Outbox/authorization tuning for the embedded usage emitter.
    #[serde(default)]
    pub emitter: UsageEmitterConfig,
}

fn default_base_url() -> String {
    "http://127.0.0.1:8080".to_owned()
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Returns `true` when `base_url` uses the `http://` scheme with a host that
/// is **not** a loopback address (`127.0.0.1`, `::1`, or `localhost`).
///
/// This is used by the module initialisation to decide whether to emit a
/// `WARN`-level log message about insecure transport configuration
/// (`cpt-cf-dod-rest-ingest-tls-config`).
pub fn is_insecure_non_loopback_http(base_url: &str) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr};

    if let Ok(parsed) = url::Url::parse(base_url)
        && parsed.scheme() == "http"
    {
        let is_loopback = match parsed.host() {
            Some(url::Host::Ipv4(addr)) => addr == Ipv4Addr::LOCALHOST,
            Some(url::Host::Ipv6(addr)) => addr == Ipv6Addr::LOCALHOST,
            Some(url::Host::Domain(d)) => d == "localhost",
            None => false,
        };
        return !is_loopback;
    }
    false
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "config_tests.rs"]
mod config_tests;
