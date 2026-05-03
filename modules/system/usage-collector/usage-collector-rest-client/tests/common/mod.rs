#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use authn_resolver_sdk::{
    AuthNResolverClient, AuthNResolverError, AuthenticationResult, ClientCredentialsRequest,
};
use chrono::Utc;
use modkit_security::SecurityContext;
use serde_json::json;
use usage_collector_rest_client::{UsageCollectorRestClient, UsageCollectorRestClientConfig};
use usage_collector_sdk::models::{UsageKind, UsageRecord};
use uuid::Uuid;

pub enum MockAuthN {
    WithToken(String),
    WithoutToken,
    Unauthorized,
    NoPlugin,
}

impl MockAuthN {
    pub fn with_token(token: impl Into<String>) -> Arc<Self> {
        Arc::new(Self::WithToken(token.into()))
    }

    pub fn without_token() -> Arc<Self> {
        Arc::new(Self::WithoutToken)
    }

    pub fn unauthorized() -> Arc<Self> {
        Arc::new(Self::Unauthorized)
    }

    pub fn no_plugin() -> Arc<Self> {
        Arc::new(Self::NoPlugin)
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
        match self {
            Self::WithToken(token) => {
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
            Self::WithoutToken => {
                let ctx = SecurityContext::builder()
                    .subject_id(nil)
                    .subject_tenant_id(nil)
                    .build()
                    .unwrap();
                Ok(AuthenticationResult {
                    security_context: ctx,
                })
            }
            Self::Unauthorized => Err(AuthNResolverError::Unauthorized(
                "bad credentials".to_owned(),
            )),
            Self::NoPlugin => Err(AuthNResolverError::NoPluginAvailable),
        }
    }
}

pub fn test_cfg(base_url: &str) -> UsageCollectorRestClientConfig {
    serde_json::from_value(json!({
        "client_id": "test-client",
        "client_secret": "test-secret",
        "base_url": base_url
    }))
    .unwrap()
}

pub fn test_record() -> UsageRecord {
    UsageRecord {
        module: "test-module".to_owned(),
        tenant_id: Uuid::nil(),
        resource_type: "vm".to_owned(),
        resource_id: Uuid::nil(),
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
        metric: "cpu.usage".to_owned(),
        kind: UsageKind::Gauge,
        idempotency_key: "idem-1".to_owned(),
        value: 1.5,
        timestamp: Utc::now(),
        metadata: None,
    }
}

pub fn make_client(base_url: &str, authn: Arc<dyn AuthNResolverClient>) -> UsageCollectorRestClient {
    UsageCollectorRestClient::new(&test_cfg(base_url), authn).expect("client build")
}
