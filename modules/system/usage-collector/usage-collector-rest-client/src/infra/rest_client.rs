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

// @cpt-dod:cpt-cf-usage-collector-dod-rest-ingest-rest-client-crate:p1
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
    // @cpt-flow:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-1
        // inst-dlv-4: called from DeliveryHandler::handle — see delivery_handler.rs
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-2
        let token = self.bearer_token().await?;
        let auth_header = format!("Bearer {token}");
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-2

        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-5
        let url = format!("{}/usage-collector/v1/records", self.base_url);

        // @cpt-begin:cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery:p1:inst-dlv-3
        let body = CreateUsageRecordBody {
            module: record.module,
            tenant_id: record.tenant_id,
            resource_type: record.resource_type,
            resource_id: record.resource_id,
            metric: record.metric,
            kind: record.kind,
            subject_id: record.subject_id,
            subject_type: record.subject_type,
            idempotency_key: record.idempotency_key,
            value: record.value,
            timestamp: record.timestamp,
            metadata: record.metadata,
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
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-8
            StatusCode::NO_CONTENT => Ok(()),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-8
            status => Err(http_status_to_usage_collector_error(status)),
        }
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-5
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-1
    }

    // @cpt-flow:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2
    async fn get_module_config(
        &self,
        module_name: &str,
    ) -> Result<ModuleConfig, UsageCollectorError> {
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-1
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-2
        let token = self.bearer_token().await?;
        let auth_header = format!("Bearer {token}");
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-2

        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-3
        let encoded_module_name = urlencoding::encode(module_name);
        let url = format!(
            "{}/usage-collector/v1/modules/{encoded_module_name}/config",
            self.base_url
        );
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-3

        let response = self
            .http_client
            .get(&url)
            .header("authorization", &auth_header)
            .send()
            .await
            .map_err(http_send_error_to_usage_collector_error)?;

        match response.status() {
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-4
            StatusCode::OK => response.json::<ModuleConfig>().await.map_err(|e| {
                UsageCollectorError::internal(format!(
                    "failed to parse module config response: {e}"
                ))
            }),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-4
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-5
            StatusCode::NOT_FOUND => Err(UsageCollectorError::module_not_found(module_name)),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-5
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-6
            status => Err(http_status_to_usage_collector_error(status)),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-6
        }
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-1
    }
}

fn authn_error_to_usage_collector_error(e: AuthNResolverError) -> UsageCollectorError {
    match e {
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-4
        // Permanent: the configured client credentials are actively rejected.
        AuthNResolverError::Unauthorized(msg) => {
            UsageCollectorError::authorization_failed(format!("client credentials: {msg}"))
        }
        // Permanent misconfiguration: no AuthN plugin is registered in the hub.
        // Retrying will not help; this requires operator intervention.
        AuthNResolverError::NoPluginAvailable => UsageCollectorError::internal(
            "no AuthN plugin available for client credentials exchange",
        ),
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-4
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-3
        // Transient: the identity service is temporarily unreachable (network outage,
        // service restart, etc.). Retrying after backoff is appropriate.
        other => {
            UsageCollectorError::unavailable(format!("client credentials exchange failed: {other}"))
        } // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-3
    }
}

fn http_send_error_to_usage_collector_error(e: HttpError) -> UsageCollectorError {
    match e {
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-6
        // Timeout variants map to PluginTimeout to keep the circuit-breaker semantics intact.
        HttpError::Timeout(_) | HttpError::DeadlineExceeded(_) => {
            UsageCollectorError::plugin_timeout()
        }
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-6
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-7
        // All other transport-level errors (connection refused, DNS failure, TLS error, etc.)
        // are transient: the request never reached the server and retrying is appropriate.
        other => UsageCollectorError::unavailable(format!("REST request failed: {other}")),
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-7
    }
}

fn http_status_to_usage_collector_error(status: StatusCode) -> UsageCollectorError {
    match status {
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-9
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            UsageCollectorError::authorization_failed(format!(
                "usage collector rejected request with HTTP {status}"
            ))
        }
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-9
        // inst-dlv-6: 429 and 5xx are transient — mapped to PluginTimeout to trigger Retry
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-10
        s if s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error() => {
            UsageCollectorError::plugin_timeout()
        }
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-10
        // inst-dlv-7: other 4xx (excluding 429) and unexpected statuses are permanent
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-11
        _ => UsageCollectorError::internal(format!(
            "unexpected HTTP status from usage collector: {status}"
        )),
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-11
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "rest_client_tests.rs"]
mod rest_client_tests;
