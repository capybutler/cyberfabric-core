//! Infrastructure adapters (HTTP, external clients).

pub use rest_client::UsageCollectorRestClient;
pub use bearer_token_auth_layer::BearerTokenAuthLayer;

pub mod bearer_token_auth_layer;
mod rest_client;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod test_support;
