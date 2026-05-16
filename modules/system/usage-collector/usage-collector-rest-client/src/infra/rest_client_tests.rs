use std::sync::Arc;

use authn_resolver_sdk::AuthNResolverClient;
use httpmock::prelude::*;
use serde_json::json;
use usage_collector_sdk::models::{UsageKind, UsageRecord};
use usage_collector_sdk::{ModuleConfig, UsageCollectorClientV1, UsageCollectorError};
use uuid::Uuid;

use super::UsageCollectorRestClient;
use super::super::test_support::{MockAuthN, test_cfg, test_record};

// --- Integration tests with mock HTTP server ---

fn make_client(base_url: &str, authn: Arc<dyn AuthNResolverClient>) -> UsageCollectorRestClient {
    UsageCollectorRestClient::new(&test_cfg(base_url), authn).unwrap()
}

#[tokio::test]
async fn create_usage_record_success_on_204() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(204);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    assert!(client.create_usage_record(test_record()).await.is_ok());
    mock.assert();
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
async fn create_usage_record_authn_unauthorized_returns_service_unavailable() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::unauthorized());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_authn_no_plugin_returns_service_unavailable() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::no_plugin());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_authn_token_acquisition_failed_returns_service_unavailable() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::token_acquisition_failed());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_authn_service_unavailable_returns_service_unavailable() {
    // ServiceUnavailable is transient: the identity service is temporarily unreachable.
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::service_unavailable());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_authn_without_token_returns_service_unavailable() {
    // exchange_client_credentials succeeds but SecurityContext has no bearer token — treated as transient.
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::without_token());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_server_401_returns_service_unavailable() {
    // 401 is transient — expired token; next attempt acquires a fresh bearer token (inst-rem-9)
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_server_403_returns_permission_denied() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(403);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::PermissionDenied { .. }));
}

#[tokio::test]
async fn create_usage_record_server_429_returns_resource_exhausted() {
    // 429 is transient — delivery handler will Retry (inst-dlv-6)
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(429);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ResourceExhausted { .. }));
}

#[tokio::test]
async fn create_usage_record_server_500_returns_service_unavailable() {
    // 500 is transient — delivery handler will Retry (inst-dlv-6)
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(500);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn create_usage_record_server_400_returns_internal() {
    // Unexpected 4xx is permanent
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(400);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
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
async fn create_usage_record_sends_subject_fields_when_present() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/usage-collector/v1/records")
            .body_includes("\"subject_id\"")
            .body_includes("\"subject_type\"");
        then.status(204);
    });

    let record = UsageRecord {
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
        ..test_record()
    };
    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    client.create_usage_record(record).await.unwrap();
    mock.assert();
}

#[tokio::test]
async fn create_usage_record_omits_subject_fields_when_absent() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/usage-collector/v1/records")
            .body_excludes("\"subject_id\"")
            .body_excludes("\"subject_type\"");
        then.status(204);
    });

    let record = UsageRecord {
        subject_id: None,
        subject_type: None,
        ..test_record()
    };
    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    client.create_usage_record(record).await.unwrap();
    mock.assert();
}

#[tokio::test]
async fn get_module_config_success() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my-module/config");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let cfg = client.get_module_config("my-module").await.unwrap();
    assert!(cfg.allowed_metrics.is_empty());
    mock.assert();
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
async fn get_module_config_server_401_returns_service_unavailable() {
    // 401 is transient — expired token; next attempt acquires a fresh bearer token (inst-rem-9)
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn get_module_config_server_404_returns_not_found() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/unknown-mod/config");
        then.status(404);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("unknown-mod").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::NotFound { .. }));
}

#[tokio::test]
async fn get_module_config_server_403_returns_permission_denied() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(403);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::PermissionDenied { .. }));
}

#[tokio::test]
async fn get_module_config_server_429_returns_resource_exhausted() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(429);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ResourceExhausted { .. }));
}

#[tokio::test]
async fn get_module_config_server_500_returns_service_unavailable() {
    // 500 from the config endpoint is transient
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(500);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn get_module_config_server_400_returns_internal() {
    // Unexpected 4xx is permanent
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(400);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
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
    assert_eq!(allowed_metrics[0].kind, UsageKind::Gauge);
    assert_eq!(allowed_metrics[1].name, "req.count");
    assert_eq!(allowed_metrics[1].kind, UsageKind::Counter);
}

// inst-cfg-rem-3: percent-encoding of module_name in get_module_config URL path

#[tokio::test]
async fn get_module_config_percent_encodes_slash_in_module_name() {
    // inst-cfg-rem-3
    // A module_name containing a '/' MUST be percent-encoded in the URL path
    // segment so the raw '/' does not appear unencoded, and the server receives
    // the encoded form '%2F' rather than an extra path separator.
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my%2Fmodule/config");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let result = client.get_module_config("my/module").await;
    assert!(
        result.is_ok(),
        "expected Ok but got Err: {result:?} — the mock server only matches '%2F', \
         so a failure here means the slash was not percent-encoded"
    );
    mock.assert();
}

#[tokio::test]
async fn get_module_config_percent_encodes_space_in_module_name() {
    // inst-cfg-rem-3
    // A module_name containing a space MUST be percent-encoded ('%20') in the
    // URL path segment.
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my%20module/config");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let result = client.get_module_config("my module").await;
    assert!(
        result.is_ok(),
        "expected Ok but got Err: {result:?} — the mock only matches '%20', \
         so a failure means the space was not percent-encoded"
    );
    mock.assert();
}

#[tokio::test]
async fn get_module_config_authn_unauthorized_returns_service_unavailable() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::unauthorized());

    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn get_module_config_authn_no_plugin_returns_service_unavailable() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::no_plugin());

    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn get_module_config_authn_token_acquisition_failed_returns_service_unavailable() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::token_acquisition_failed());

    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn get_module_config_authn_service_unavailable_returns_service_unavailable() {
    // ServiceUnavailable is transient: the identity service is temporarily unreachable.
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::service_unavailable());

    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

#[tokio::test]
async fn get_module_config_authn_without_token_returns_service_unavailable() {
    // exchange_client_credentials succeeds but SecurityContext has no bearer token — treated as transient.
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::without_token());

    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ServiceUnavailable { .. }));
}

