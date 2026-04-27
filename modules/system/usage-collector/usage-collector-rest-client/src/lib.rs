//! `usage-collector-rest-client` crate
//!
//! Provides [`UsageCollectorRestClientModule`] — a `ModKit` module that satisfies
//! the `"usage-collector-client"` dependency when usage is emitted from a **separate**
//! `CyberFabric` binary that must reach the collector over **HTTP/REST** (Scenario C).
//!
//! Each `create_usage_record` exchanges `OAuth2` client credentials via [`AuthNResolverClient`],
//! reads the bearer token from the returned [`SecurityContext`], and POSTs the record
//! to `POST {base_url}/usage-collector/v1/records`.
//!
//! ## Configuration
//!
//! `client_id` and `client_secret` are required. `base_url`, `scopes`, and
//! `request_timeout` are optional and use defaults when omitted.
//!
//! ```yaml
//! modules:
//!   usage-collector-client:
//!     config:
//!       client_id: "my-client"
//!       client_secret: "${CLIENT_SECRET}"
//!       base_url: "http://127.0.0.1:8080"
//!       scopes: []
//!       request_timeout: "30s"
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod api;
mod config;
mod infra;
mod module;
