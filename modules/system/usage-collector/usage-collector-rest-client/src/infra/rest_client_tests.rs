use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use authn_resolver_sdk::{
    AuthNResolverClient, AuthNResolverError, AuthenticationResult, ClientCredentialsRequest,
};
use chrono::Utc;
use http::StatusCode;
use httpmock::prelude::*;
use modkit_http::HttpError;
use modkit_security::SecurityContext;
use serde_json::json;
use usage_collector_sdk::models::{UsageKind, UsageRecord};
use usage_collector_sdk::{ModuleConfig, UsageCollectorClientV1, UsageCollectorError};
use uuid::Uuid;

use super::{
    UsageCollectorRestClient, authn_error_to_usage_collector_error,
    http_send_error_to_usage_collector_error, http_status_to_usage_collector_error,
};
use crate::config::UsageCollectorRestClientConfig;

// --- Error-mapping unit tests (pure, no I/O) ---

#[derive(Debug)]
struct DummyTransport(&'static str);

impl fmt::Display for DummyTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for DummyTransport {}

#[test]
fn http_timeout_maps_to_plugin_timeout() {
    let err = HttpError::Timeout(Duration::from_secs(5));
    let out = http_send_error_to_usage_collector_error(err);
    assert!(matches!(out, UsageCollectorError::PluginTimeout));
}

#[test]
fn http_deadline_exceeded_maps_to_plugin_timeout() {
    let err = HttpError::DeadlineExceeded(Duration::from_secs(30));
    let out = http_send_error_to_usage_collector_error(err);
    assert!(matches!(out, UsageCollectorError::PluginTimeout));
}

#[test]
fn http_transport_maps_to_internal() {
    let err = HttpError::Transport(Box::new(DummyTransport("connection refused")));
    let out = http_send_error_to_usage_collector_error(err);
    assert!(matches!(out, UsageCollectorError::Internal { .. }));
}

#[test]
fn http_status_unauthorized_maps_to_authorization_failed() {
    let out = http_status_to_usage_collector_error(StatusCode::UNAUTHORIZED);
    assert!(matches!(
        out,
        UsageCollectorError::AuthorizationFailed { ref message }
        if message.contains("401")
    ));
}

#[test]
fn http_status_forbidden_maps_to_authorization_failed() {
    let out = http_status_to_usage_collector_error(StatusCode::FORBIDDEN);
    assert!(matches!(
        out,
        UsageCollectorError::AuthorizationFailed { ref message }
        if message.contains("403")
    ));
}

#[test]
fn http_status_internal_server_error_maps_to_plugin_timeout() {
    // 5xx responses are transient — trigger Retry in DeliveryHandler (inst-dlv-6)
    let out = http_status_to_usage_collector_error(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(matches!(out, UsageCollectorError::PluginTimeout));
}

#[test]
fn http_status_too_many_requests_maps_to_plugin_timeout() {
    // 429 is transient — trigger Retry in DeliveryHandler (inst-dlv-6)
    let out = http_status_to_usage_collector_error(StatusCode::TOO_MANY_REQUESTS);
    assert!(matches!(out, UsageCollectorError::PluginTimeout));
}

#[test]
fn http_status_service_unavailable_maps_to_plugin_timeout() {
    let out = http_status_to_usage_collector_error(StatusCode::SERVICE_UNAVAILABLE);
    assert!(matches!(out, UsageCollectorError::PluginTimeout));
}

#[test]
fn http_status_ok_maps_to_internal() {
    // Success codes other than 204 are still unexpected for this API.
    let out = http_status_to_usage_collector_error(StatusCode::OK);
    assert!(matches!(out, UsageCollectorError::Internal { .. }));
}

#[test]
fn authn_unauthorized_maps_to_authorization_failed() {
    let out = authn_error_to_usage_collector_error(AuthNResolverError::Unauthorized(
        "invalid client".to_owned(),
    ));
    assert!(matches!(
        out,
        UsageCollectorError::AuthorizationFailed { ref message }
        if message.contains("invalid client")
    ));
}

#[test]
fn authn_token_acquisition_failed_maps_to_internal() {
    let out = authn_error_to_usage_collector_error(AuthNResolverError::TokenAcquisitionFailed(
        "idp down".to_owned(),
    ));
    assert!(matches!(out, UsageCollectorError::Internal { .. }));
}

#[test]
fn authn_no_plugin_maps_to_internal() {
    let out = authn_error_to_usage_collector_error(AuthNResolverError::NoPluginAvailable);
    assert!(matches!(out, UsageCollectorError::Internal { .. }));
}

// --- Integration tests with mock HTTP server ---

enum AuthNOutcome {
    WithToken(String),
    WithoutToken,
    Unauthorized,
    NoPlugin,
}

struct MockAuthN {
    outcome: AuthNOutcome,
}

impl MockAuthN {
    fn with_token(token: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            outcome: AuthNOutcome::WithToken(token.into()),
        })
    }

    fn without_token() -> Arc<Self> {
        Arc::new(Self {
            outcome: AuthNOutcome::WithoutToken,
        })
    }

    fn unauthorized() -> Arc<Self> {
        Arc::new(Self {
            outcome: AuthNOutcome::Unauthorized,
        })
    }

    fn no_plugin() -> Arc<Self> {
        Arc::new(Self {
            outcome: AuthNOutcome::NoPlugin,
        })
    }
}

#[async_trait]
impl AuthNResolverClient for MockAuthN {
    async fn authenticate(
        &self,
        _bearer_token: &str,
    ) -> Result<AuthenticationResult, AuthNResolverError> {
        unimplemented!()
    }

    async fn exchange_client_credentials(
        &self,
        _request: &ClientCredentialsRequest,
    ) -> Result<AuthenticationResult, AuthNResolverError> {
        let nil = Uuid::nil();
        match &self.outcome {
            AuthNOutcome::WithToken(token) => {
                let ctx = SecurityContext::builder()
                    .subject_id(nil)
                    .subject_tenant_id(nil)
                    .bearer_token(token.clone())
                    .build()
                    .unwrap();
                Ok(AuthenticationResult {
                    security_context: ctx,
                })
            }
            AuthNOutcome::WithoutToken => {
                let ctx = SecurityContext::builder()
                    .subject_id(nil)
                    .subject_tenant_id(nil)
                    .build()
                    .unwrap();
                Ok(AuthenticationResult {
                    security_context: ctx,
                })
            }
            AuthNOutcome::Unauthorized => Err(AuthNResolverError::Unauthorized(
                "bad credentials".to_owned(),
            )),
            AuthNOutcome::NoPlugin => Err(AuthNResolverError::NoPluginAvailable),
        }
    }
}

fn test_cfg(base_url: &str) -> UsageCollectorRestClientConfig {
    serde_json::from_value(json!({
        "client_id": "test-client",
        "client_secret": "test-secret",
        "base_url": base_url
    }))
    .unwrap()
}

fn test_record() -> UsageRecord {
    UsageRecord {
        module: "test-module".to_owned(),
        tenant_id: Uuid::nil(),
        resource_type: "vm".to_owned(),
        resource_id: Uuid::nil(),
        subject_id: Uuid::nil(),
        subject_type: "test.subject".to_owned(),
        metric: "cpu.usage".to_owned(),
        kind: UsageKind::Gauge,
        idempotency_key: "idem-1".to_owned(),
        value: 1.5,
        timestamp: Utc::now(),
        metadata: None,
    }
}

fn make_client(base_url: &str, authn: Arc<dyn AuthNResolverClient>) -> UsageCollectorRestClient {
    UsageCollectorRestClient::new(&test_cfg(base_url), authn).unwrap()
}

#[tokio::test]
async fn create_usage_record_success_on_204() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(204);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    assert!(client.create_usage_record(test_record()).await.is_ok());
}

#[tokio::test]
async fn create_usage_record_sends_bearer_token_header() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/usage-collector/v1/records")
            .header("authorization", "Bearer my-token");
        then.status(204);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("my-token"));
    client.create_usage_record(test_record()).await.unwrap();
    mock.assert();
}

#[tokio::test]
async fn create_usage_record_authn_unauthorized_returns_authorization_failed() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::unauthorized());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(
        err,
        UsageCollectorError::AuthorizationFailed { .. }
    ));
}

#[tokio::test]
async fn create_usage_record_authn_no_plugin_returns_internal() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::no_plugin());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
}

#[tokio::test]
async fn create_usage_record_missing_bearer_token_returns_internal() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::without_token());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
}

#[tokio::test]
async fn create_usage_record_server_401_returns_authorization_failed() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(
        err,
        UsageCollectorError::AuthorizationFailed { .. }
    ));
}

#[tokio::test]
async fn create_usage_record_server_403_returns_authorization_failed() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(403);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(
        err,
        UsageCollectorError::AuthorizationFailed { .. }
    ));
}

#[tokio::test]
async fn create_usage_record_server_500_returns_plugin_timeout() {
    // 500 is transient — delivery handler will Retry (inst-dlv-6)
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(500);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::PluginTimeout));
}

#[tokio::test]
async fn create_usage_record_base_url_trailing_slash_is_trimmed() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(204);
    });

    let url_with_slash = format!("{}/", server.base_url());
    let client = make_client(&url_with_slash, MockAuthN::with_token("tok"));
    assert!(client.create_usage_record(test_record()).await.is_ok());
}

#[tokio::test]
async fn get_module_config_success() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my-module/config");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let cfg = client.get_module_config("my-module").await.unwrap();
    assert!(cfg.allowed_metrics.is_empty());
}

#[tokio::test]
async fn get_module_config_sends_bearer_token_header() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config")
            .header("authorization", "Bearer cfg-token");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("cfg-token"));
    client.get_module_config("mod-x").await.unwrap();
    mock.assert();
}

#[tokio::test]
async fn get_module_config_server_401_returns_authorization_failed() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(
        err,
        UsageCollectorError::AuthorizationFailed { .. }
    ));
}

#[tokio::test]
async fn get_module_config_server_500_returns_plugin_timeout() {
    // 500 from the config endpoint is transient
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(500);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::PluginTimeout));
}

#[tokio::test]
async fn get_module_config_invalid_json_response_returns_internal() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(200).body("not-json");
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
}

#[tokio::test]
async fn get_module_config_returns_allowed_metrics() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my-mod/config");
        then.status(200).json_body(json!({
            "allowed_metrics": [
                {"name": "cpu.usage", "kind": "gauge"},
                {"name": "req.count", "kind": "counter"}
            ]
        }));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let ModuleConfig { allowed_metrics } = client.get_module_config("my-mod").await.unwrap();
    assert_eq!(allowed_metrics.len(), 2);
    assert_eq!(allowed_metrics[0].name, "cpu.usage");
    assert_eq!(allowed_metrics[1].name, "req.count");
}
