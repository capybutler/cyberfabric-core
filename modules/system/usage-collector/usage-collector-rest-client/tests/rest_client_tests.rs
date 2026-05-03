#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use httpmock::prelude::*;
use serde_json::json;
use usage_collector_rest_client::{UsageCollectorRestClient, UsageCollectorRestClientConfig};
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use uuid::Uuid;

use common::{MockAuthN, make_client, test_cfg, test_record};

// --- create_usage_record tests ---

#[tokio::test]
async fn create_usage_record_returns_ok_on_204() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(204);
    });

    let _cfg: UsageCollectorRestClientConfig = test_cfg(&server.base_url());
    let client: UsageCollectorRestClient = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    assert!(client.create_usage_record(test_record()).await.is_ok());
}

#[tokio::test]
async fn create_usage_record_sends_bearer_token() {
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
async fn create_usage_record_returns_authorization_failed_when_authn_unauthorized() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::unauthorized());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn create_usage_record_returns_internal_when_authn_no_plugin() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::no_plugin());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
}

#[tokio::test]
async fn create_usage_record_returns_internal_when_no_bearer_token() {
    let server = MockServer::start();
    let client = make_client(&server.base_url(), MockAuthN::without_token());

    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
}

#[tokio::test]
async fn create_usage_record_returns_authorization_failed_on_401() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn create_usage_record_returns_authorization_failed_on_403() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(403);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.create_usage_record(test_record()).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn create_usage_record_returns_plugin_timeout_on_500() {
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
async fn create_usage_record_trims_trailing_slash_from_base_url() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST).path("/usage-collector/v1/records");
        then.status(204);
    });

    let url_with_slash = format!("{}/", server.base_url());
    let client = make_client(&url_with_slash, MockAuthN::with_token("tok"));
    assert!(client.create_usage_record(test_record()).await.is_ok());
    mock.assert();
}

#[tokio::test]
async fn create_usage_record_sends_subject_fields() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/usage-collector/v1/records")
            .body_includes("\"subject_id\"")
            .body_includes("\"subject_type\"");
        then.status(204);
    });

    use usage_collector_sdk::models::UsageRecord;
    let record = UsageRecord {
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
        ..test_record()
    };
    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    client.create_usage_record(record).await.unwrap();
    mock.assert();
}

// --- get_module_config tests ---

#[tokio::test]
async fn get_module_config_returns_config_on_200() {
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
async fn get_module_config_sends_bearer_token() {
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
async fn get_module_config_returns_authorization_failed_on_401() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/mod-x/config");
        then.status(401);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("mod-x").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn get_module_config_returns_plugin_timeout_on_500() {
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
async fn get_module_config_returns_internal_on_invalid_json() {
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
async fn get_module_config_returns_module_not_found_on_404() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/missing-mod/config");
        then.status(404);
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let err = client.get_module_config("missing-mod").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ModuleNotFound { .. }));
}

#[tokio::test]
async fn get_module_config_percent_encodes_slash_in_module_name() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my%2Fmodule/config");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let result = client.get_module_config("my/module").await;
    assert!(
        result.is_ok(),
        "expected Ok but got Err: {result:?} — slash was not percent-encoded"
    );
}

#[tokio::test]
async fn get_module_config_percent_encodes_space_in_module_name() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage-collector/v1/modules/my%20module/config");
        then.status(200).json_body(json!({"allowed_metrics": []}));
    });

    let client = make_client(&server.base_url(), MockAuthN::with_token("tok"));
    let result = client.get_module_config("my module").await;
    assert!(
        result.is_ok(),
        "expected Ok but got Err: {result:?} — space was not percent-encoded"
    );
}
