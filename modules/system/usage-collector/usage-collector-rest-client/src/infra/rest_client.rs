//! REST client wiring and [`usage_collector_sdk::UsageCollectorClientV1`] implementation.

use std::sync::Arc;

use async_trait::async_trait;
use authn_resolver_sdk::{AuthNResolverClient, AuthNResolverError, ClientCredentialsRequest};
use http::StatusCode;
use modkit_http::{HttpClient, HttpClientBuilder, HttpError};
use secrecy::{ExposeSecret, SecretString};
use usage_collector_sdk::models::UsageRecord;
use usage_collector_sdk::{ModuleConfig, UsageCollectorClientV1, UsageCollectorError};

use crate::api::rest::dto::CreateUsageRecordBody;
use crate::config::UsageCollectorRestClientConfig;

/// REST-backed [`usage_collector_sdk::UsageCollectorClientV1`].
pub struct UsageCollectorRestClient {
    http_client: HttpClient,
    authn_client: Arc<dyn AuthNResolverClient>,
    client_id: String,
    client_secret: SecretString,
    scopes: Vec<String>,
    base_url: String,
}

impl UsageCollectorRestClient {
    /// Build a client from module config and the shared `AuthN` resolver.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(
        cfg: &UsageCollectorRestClientConfig,
        authn: Arc<dyn AuthNResolverClient>,
    ) -> Result<Self, modkit_http::HttpError> {
        let http_client = HttpClientBuilder::new()
            .timeout(cfg.request_timeout)
            .build()?;

        let base_url = cfg.base_url.trim_end_matches('/').to_owned();

        Ok(Self {
            http_client,
            authn_client: authn,
            client_id: cfg.client_id.clone(),
            client_secret: cfg.client_secret.clone(),
            scopes: cfg.scopes.clone(),
            base_url,
        })
    }

    async fn bearer_token(&self) -> Result<String, UsageCollectorError> {
        let request = ClientCredentialsRequest {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            scopes: self.scopes.clone(),
        };

        let auth_result = self
            .authn_client
            .exchange_client_credentials(&request)
            .await
            .map_err(authn_error_to_usage_collector_error)?;

        let token = auth_result
            .security_context
            .bearer_token()
            .ok_or_else(|| {
                UsageCollectorError::internal(
                    "AuthN exchange succeeded but SecurityContext has no bearer token",
                )
            })?
            .expose_secret()
            .to_owned();

        Ok(token)
    }
}

#[async_trait]
impl UsageCollectorClientV1 for UsageCollectorRestClient {
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        // inst-dlv-4: called from DeliveryHandler::handle — see delivery_handler.rs
        let token = self.bearer_token().await?;
        let auth_header = format!("Bearer {token}");

        let url = format!("{}/usage-collector/v1/records", self.base_url);

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-3
        let body = CreateUsageRecordBody {
            module: record.module,
            tenant_id: record.tenant_id,
            resource_type: record.resource_type,
            resource_id: record.resource_id,
            metric: record.metric,
            kind: record.kind,
            idempotency_key: record.idempotency_key,
            value: record.value,
            timestamp: record.timestamp,
        };
        // @cpt-end:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-3

        let response = self
            .http_client
            .post(&url)
            .header("authorization", &auth_header)
            .json(&body)
            .map_err(|e| {
                UsageCollectorError::internal(format!("failed to serialize usage record: {e}"))
            })?
            .send()
            .await
            .map_err(http_send_error_to_usage_collector_error)?;

        match response.status() {
            StatusCode::NO_CONTENT => Ok(()),
            status => Err(http_status_to_usage_collector_error(status)),
        }
    }

    async fn get_module_config(
        &self,
        module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        let token = self.bearer_token().await?;
        let auth_header = format!("Bearer {token}");

        let url = format!(
            "{}/usage-collector/v1/modules/{module_name}/config",
            self.base_url
        );

        let response = self
            .http_client
            .get(&url)
            .header("authorization", &auth_header)
            .send()
            .await
            .map_err(http_send_error_to_usage_collector_error)?;

        match response.status() {
            StatusCode::OK => response.json::<ModuleConfig>().await.map_err(|e| {
                UsageCollectorError::internal(format!(
                    "failed to parse module config response: {e}"
                ))
            }),
            status => Err(http_status_to_usage_collector_error(status)),
        }
    }
}

fn authn_error_to_usage_collector_error(e: AuthNResolverError) -> UsageCollectorError {
    match e {
        AuthNResolverError::Unauthorized(msg) => {
            UsageCollectorError::authorization_failed(format!("client credentials: {msg}"))
        }
        other => {
            UsageCollectorError::internal(format!("client credentials exchange failed: {other}"))
        }
    }
}

fn http_send_error_to_usage_collector_error(e: HttpError) -> UsageCollectorError {
    match e {
        HttpError::Timeout(_) | HttpError::DeadlineExceeded(_) => {
            UsageCollectorError::plugin_timeout()
        }
        other => UsageCollectorError::internal(format!("REST request failed: {other}")),
    }
}

fn http_status_to_usage_collector_error(status: StatusCode) -> UsageCollectorError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            UsageCollectorError::authorization_failed(format!(
                "usage collector rejected request with HTTP {status}"
            ))
        }
        // inst-dlv-6: 429 and 5xx are transient — mapped to PluginTimeout to trigger Retry
        s if s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error() => {
            UsageCollectorError::plugin_timeout()
        }
        // inst-dlv-7: other 4xx (excluding 429) and unexpected statuses are permanent
        _ => UsageCollectorError::internal(format!(
            "unexpected HTTP status from usage collector: {status}"
        )),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "rest_client_tests.rs"]
mod rest_client_tests;
