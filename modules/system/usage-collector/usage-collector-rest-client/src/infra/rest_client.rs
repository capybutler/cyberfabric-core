//! REST client wiring and [`usage_collector_sdk::UsageCollectorClientV1`] implementation.

use std::sync::Arc;

use async_trait::async_trait;
use authn_resolver_sdk::{AuthNResolverClient, ClientCredentialsRequest};
use http::StatusCode;
use modkit_http::{HttpClient, HttpClientBuilder, HttpClientConfig, HttpError, HttpResponse};
use tower::ServiceExt;
use usage_collector_sdk::models::UsageRecord;
use usage_collector_sdk::{
    ModuleConfig, ModuleConfigError, UsageCollectorClientV1, UsageCollectorError, UsageRecordError,
};

use crate::config::UsageCollectorRestClientConfig;
use crate::infra::BearerTokenAuthLayer;

// @cpt-dod:cpt-cf-usage-collector-dod-rest-ingest-rest-client-crate:p1
/// REST-backed [`usage_collector_sdk::UsageCollectorClientV1`].
pub struct UsageCollectorRestClient {
    cfg: UsageCollectorRestClientConfig,
    http_client: HttpClient,
}

impl UsageCollectorRestClient {
    /// Build a client from module config and the shared `AuthN` resolver.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(
        cfg: &UsageCollectorRestClientConfig,
        authn_client: Arc<dyn AuthNResolverClient>,
    ) -> Result<Self, modkit_http::HttpError> {
        let credentials = ClientCredentialsRequest {
            client_id: cfg.oauth.client_id.clone(),
            client_secret: cfg.oauth.client_secret.clone(),
            scopes: cfg.oauth.scopes.clone(),
        };
        let layer = BearerTokenAuthLayer::new(authn_client, credentials);
        let http_client = HttpClientBuilder::with_config(HttpClientConfig::default())
            .with_auth_layer(move |svc| {
                tower::ServiceBuilder::new()
                    .layer(layer)
                    .service(svc)
                    .boxed_clone()
            })
            .build()?;

        Ok(Self {
            cfg: cfg.clone(),
            http_client,
        })
    }
}

#[async_trait]
impl UsageCollectorClientV1 for UsageCollectorRestClient {
    // @cpt-flow:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-1
        // inst-dlv-4: called from DeliveryHandler::handle — see delivery_handler.rs

        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-5
        let mut url = self.cfg.collector_url.clone();
        url.path_segments_mut()
            .map_err(|()| UsageCollectorError::internal("collector_url is not a hierarchical URL").create())?
            .clear()
            .extend(["usage-collector", "v1", "records"]);

        let response = self
            .http_client
            .post(url.as_str())
            .json(&record)
            .map_err(|e| {
                UsageCollectorError::internal(format!("failed to serialize usage record: {e}")).create()
            })?
            .send()
            .await
            .map_err(|e| match e {
                // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-3a
                // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-4a
                // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-7a
                // `BearerTokenAuthLayer` wraps every AuthN resolver failure (transient or
                // permanent credential rejection) as `HttpError::Transport`, so this arm
                // returns the `ServiceUnavailable` mapping for `inst-rem-3a`/`inst-rem-4a`.
                // Genuine HTTP transport errors (connection refused, DNS failure, TLS
                // failure, …) flow through the same arm and satisfy `inst-rem-7a`.
                HttpError::Transport(inner) => UsageCollectorError::service_unavailable()
                    .with_detail(format!("REST request failed: {inner}"))
                    .create(),
                // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-7a
                // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-4a
                // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-3a
                // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-6
                // Timeout variants map to DeadlineExceeded to keep the circuit-breaker semantics intact.
                HttpError::Timeout(_) | HttpError::DeadlineExceeded(_) => {
                    UsageRecordError::deadline_exceeded("HTTP request deadline exceeded").create()
                }
                // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-6
                // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-7
                // Residual non-Transport, non-Timeout HttpError variants
                // (`InvalidHeaderValue`, `BodyTooLarge`, `Tls`, …) are also transient from
                // the outbox's point of view: the request never reached the server, so
                // retrying is appropriate.
                other => UsageCollectorError::service_unavailable()
                    .with_detail(format!("REST request failed: {other}"))
                    .create(),
                // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-7
            })?;

        match response.status() {
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-8
            StatusCode::NO_CONTENT => Ok(()),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-8
            status => {
                let body = truncated_response_body(response).await;
                match status {
                    // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-9
                    // 401 is transient: the bearer token may have expired between acquisition and the
                    // request reaching the gateway. The next delivery attempt acquires a fresh token.
                    StatusCode::UNAUTHORIZED => Err(UsageCollectorError::service_unavailable()
                        .with_detail(format!(
                            "usage collector rejected request with HTTP {}: {body}",
                            StatusCode::UNAUTHORIZED
                        ))
                        .create()),
                    // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-9
                    // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-11
                    // 403 is permanent: the gateway PDP denied the forwarder's service identity.
                    // Implements the `PermissionDenied` half of `inst-rem-11a`; the catch-all
                    // status arm below handles the `Internal` half for residual 4xx.
                    StatusCode::FORBIDDEN => Err(UsageRecordError::permission_denied()
                        .with_reason(format!(
                            "usage collector rejected request with HTTP {}: {body}",
                            StatusCode::FORBIDDEN
                        ))
                        .create()),
                    // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-11
                    // inst-dlv-6: 429 and 5xx are transient — mapped to ResourceExhausted/ServiceUnavailable to trigger Retry
                    // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-10
                    StatusCode::TOO_MANY_REQUESTS => Err(UsageRecordError::resource_exhausted(
                        "usage collector rejected request: rate limit exceeded",
                    )
                    .with_quota_violation(
                        "requests",
                        format!("rate limit exceeded by usage collector: {body}"),
                    )
                    .create()),
                    s if s.is_server_error() => Err(UsageCollectorError::service_unavailable()
                        .with_detail(format!("usage collector returned HTTP {s}: {body}"))
                        .create()),
                    // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-10
                    // inst-dlv-7: other 4xx (excluding 429) and unexpected statuses are permanent
                    // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-11
                    status => Err(UsageCollectorError::internal(format!(
                        "unexpected HTTP status from usage collector: {status}: {body}"
                    ))
                    .create()),
                    // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-remote-emit:p1:inst-rem-11
                }
            }
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

        // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-3
        let mut url = self.cfg.collector_url.clone();
        url.path_segments_mut()
            .map_err(|()| UsageCollectorError::internal("collector_url is not a hierarchical URL").create())?
            .clear()
            .extend(["usage-collector", "v1", "modules", module_name, "config"]);
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-3

        let response = self
            .http_client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| match e {
                HttpError::Transport(inner) => UsageCollectorError::service_unavailable()
                    .with_detail(format!("REST request failed: {inner}"))
                    .create(),
                HttpError::Timeout(_) | HttpError::DeadlineExceeded(_) => {
                    ModuleConfigError::deadline_exceeded("HTTP request deadline exceeded").create()
                }
                other => UsageCollectorError::service_unavailable()
                    .with_detail(format!("REST request failed: {other}"))
                    .create(),
            })?;

        match response.status() {
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-4
            StatusCode::OK => response.json::<ModuleConfig>().await.map_err(|e| {
                UsageCollectorError::internal(format!(
                    "failed to parse module config response: {e}"
                ))
                .create()
            }),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-4
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-5
            StatusCode::NOT_FOUND => Err(ModuleConfigError::not_found(format!(
                "module '{module_name}' is not configured"
            ))
            .with_resource(module_name)
            .create()),
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-5
            // @cpt-begin:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-6
            status => {
                let body = truncated_response_body(response).await;
                match status {
                    StatusCode::UNAUTHORIZED => Err(UsageCollectorError::service_unavailable()
                        .with_detail(format!(
                            "usage collector rejected request with HTTP {}: {body}",
                            StatusCode::UNAUTHORIZED
                        ))
                        .create()),
                    StatusCode::FORBIDDEN => Err(ModuleConfigError::permission_denied()
                        .with_reason(format!(
                            "usage collector rejected request with HTTP {}: {body}",
                            StatusCode::FORBIDDEN
                        ))
                        .create()),
                    StatusCode::TOO_MANY_REQUESTS => Err(ModuleConfigError::resource_exhausted(
                        "usage collector rejected request: rate limit exceeded",
                    )
                    .with_quota_violation(
                        "requests",
                        format!("rate limit exceeded by usage collector: {body}"),
                    )
                    .create()),
                    s if s.is_server_error() => Err(UsageCollectorError::service_unavailable()
                        .with_detail(format!("usage collector returned HTTP {s}: {body}"))
                        .create()),
                    status => Err(UsageCollectorError::internal(format!(
                        "unexpected HTTP status from usage collector: {status}: {body}"
                    ))
                    .create()),
                }
            }
            // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-6
        }
        // @cpt-end:cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config:p2:inst-cfg-rem-1
    }
}

async fn truncated_response_body(response: HttpResponse) -> String {
    const MAX: usize = 4_096;
    let mut body = response
        .bytes()
        .await
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    if body.len() > MAX {
        body.truncate(body.floor_char_boundary(MAX));
    }
    body
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "rest_client_tests.rs"]
mod rest_client_tests;
