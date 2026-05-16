//! Authorization algorithm for query endpoints: `authorize_and_compile_scope`.
//!
//! Implements the PDP call, fail-closed error handling, and OR-of-ANDs scope
//! compilation for the aggregated and raw query endpoints.
//!
//! Resource type, action, and property constants live in
//! [`usage_collector_sdk::authz`].

use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use authz_resolver_sdk::EnforcerError;
use authz_resolver_sdk::pep::{AccessRequest, PolicyEnforcer, ResourceType};
use modkit_security::{AccessScope, SecurityContext};
use tracing::error;
use usage_collector_sdk::{UsageCollectorError, UsageRecordError};

/// Call the PDP, compile constraints into an [`AccessScope`], and return it.
///
/// Implements the authorize-and-compile-scope algorithm
/// (`cpt-cf-usage-collector-algo-query-api-authz-delegate`):
///
/// 1. Builds an `AccessRequest` with `require_constraints(true)`.
///    `BarrierMode::Respect` is the `AccessRequest` default and is preserved.
/// 2. Calls `PolicyEnforcer::access_scope_with` — the enforcer internally compiles the
///    PDP constraints into an `AccessScope`, preserving the OR-of-ANDs structure.
/// 3. Maps `Err(Denied)` → `Err(PermissionDenied)` immediately.
/// 4. Maps any other PDP error (`EvaluationFailed`, `CompileFailed`) →
///    `Err(PermissionDenied)` (fail-closed). Logs at ERROR level with the caller's
///    subject ID as correlation; never logs raw PDP error details or PII.
/// 5. Returns `Ok(scope)` on success.
///
/// # Errors
///
/// Returns [`UsageCollectorError::PermissionDenied`] on any PDP error (Denied or
/// non-Denied). No allow-all path exists for any PDP error condition.
// @cpt-algo:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1
pub async fn authorize_and_compile_scope(
    ctx: &SecurityContext,
    authz: Arc<dyn AuthZResolverClient>,
    resource_type: &ResourceType,
    action: &str,
) -> Result<AccessScope, UsageCollectorError> {
    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-1
    let request = AccessRequest::new().require_constraints(true);
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-1

    // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-2
    let result = PolicyEnforcer::new(authz)
        .access_scope_with(ctx, resource_type, action, None, &request)
        .await;
    // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-2

    match result {
        // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-4
        // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-5
        Ok(scope) => Ok(scope),
        // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-5
        // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-4

        // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-3
        Err(EnforcerError::Denied { .. }) => {
            // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-3a
            Err(UsageRecordError::permission_denied()
                .with_reason("AUTHORIZATION_DENIED")
                .create())
            // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-3a
        }
        // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-3

        // @cpt-begin:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-3b
        Err(e) => {
            error!(
                subject_id = %ctx.subject_id(),
                pdp_error_variant = pdp_error_variant_name(&e),
                "PDP infrastructure error (non-Denied): access denied (fail-closed)",
            );
            Err(UsageRecordError::permission_denied()
                .with_reason("AUTHORIZATION_DENIED")
                .create())
        } // @cpt-end:cpt-cf-usage-collector-algo-query-api-authz-delegate:p1:inst-authz-3b
    }
}

/// Returns a static variant name string for the given `EnforcerError`.
///
/// Used for structured logging — never includes raw error details or PII.
fn pdp_error_variant_name(e: &EnforcerError) -> &'static str {
    match e {
        EnforcerError::Denied { .. } => "Denied",
        EnforcerError::EvaluationFailed(_) => "EvaluationFailed",
        EnforcerError::CompileFailed(_) => "CompileFailed",
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "authz_tests.rs"]
mod authz_tests;
