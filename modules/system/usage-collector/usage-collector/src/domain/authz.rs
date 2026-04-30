//! Authorization algorithm for query endpoints: `authorize_and_compile_scope`.
//!
//! Implements the PDP call, fail-closed error handling, and OR-of-ANDs scope
//! compilation for the aggregated and raw query endpoints.

use std::sync::Arc;

use authz_resolver_sdk::AuthZResolverClient;
use authz_resolver_sdk::EnforcerError;
use authz_resolver_sdk::pep::{AccessRequest, PolicyEnforcer, ResourceType};
use modkit_security::{AccessScope, SecurityContext, pep_properties};
use tracing::error;
use usage_collector_sdk::UsageCollectorError;

/// Usage record resource type for read (query) operations.
///
/// Used by [`authorize_and_compile_scope`] when calling the PDP for LIST access.
/// Supports `owner_tenant_id` so the PDP can return tenant-scoped constraints.
pub const USAGE_RECORD_READ: ResourceType = ResourceType {
    name: "gts.x.core.usage.record.v1~",
    supported_properties: &[pep_properties::OWNER_TENANT_ID],
};

/// PDP action constants for usage-record query authorization.
pub mod actions {
    /// List (query) action for usage records.
    pub const LIST: &str = "list";
}

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
/// Returns [`UsageCollectorError::AuthorizationFailed`] on any PDP error (Denied or
/// non-Denied). No allow-all path exists for any PDP error condition.
pub async fn authorize_and_compile_scope(
    ctx: &SecurityContext,
    authz: Arc<dyn AuthZResolverClient>,
    resource_type: &ResourceType,
    action: &str,
) -> Result<AccessScope, UsageCollectorError> {
    let request = AccessRequest::new().require_constraints(true);

    let result = PolicyEnforcer::new(authz)
        .access_scope_with(ctx, resource_type, action, None, &request)
        .await;

    match result {
        Ok(scope) => Ok(scope),

        Err(EnforcerError::Denied { .. }) => {
            Err(UsageCollectorError::authorization_failed("permission denied"))
        }

        Err(e) => {
            error!(
                subject_id = %ctx.subject_id(),
                pdp_error_variant = pdp_error_variant_name(&e),
                "PDP infrastructure error (non-Denied): {}; correlation_id={}; access denied (fail-closed)",
                pdp_error_variant_name(&e),
                ctx.subject_id(),
            );
            Err(UsageCollectorError::authorization_failed("permission denied"))
        }
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
mod authz_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use authz_resolver_sdk::constraints::{Constraint, InPredicate, Predicate};
    use authz_resolver_sdk::models::{
        EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
    };
    use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError, DenyReason};
    use modkit_security::SecurityContext;
    use modkit_security::access_scope::pep_properties;
    use usage_collector_sdk::UsageCollectorError;
    use uuid::Uuid;

    use super::{USAGE_RECORD_READ, actions, authorize_and_compile_scope};

    // ── Mock AuthZ clients ────────────────────────────────────────────────────

    /// Mock PDP that always returns a deny decision.
    struct DenyAuthZ;

    #[async_trait]
    impl AuthZResolverClient for DenyAuthZ {
        async fn evaluate(
            &self,
            _request: EvaluationRequest,
        ) -> Result<EvaluationResponse, AuthZResolverError> {
            Ok(EvaluationResponse {
                decision: false,
                context: EvaluationResponseContext {
                    deny_reason: Some(DenyReason {
                        error_code: "POLICY_DENIED".to_owned(),
                        details: None,
                    }),
                    ..EvaluationResponseContext::default()
                },
            })
        }
    }

    /// Mock PDP that simulates a network/infrastructure failure.
    struct NetworkErrorAuthZ;

    #[async_trait]
    impl AuthZResolverClient for NetworkErrorAuthZ {
        async fn evaluate(
            &self,
            _request: EvaluationRequest,
        ) -> Result<EvaluationResponse, AuthZResolverError> {
            Err(AuthZResolverError::ServiceUnavailable(
                "PDP unreachable".to_owned(),
            ))
        }
    }

    /// Mock PDP that returns a single tenant constraint.
    struct SingleConstraintAuthZ {
        tenant_id: Uuid,
    }

    #[async_trait]
    impl AuthZResolverClient for SingleConstraintAuthZ {
        async fn evaluate(
            &self,
            _request: EvaluationRequest,
        ) -> Result<EvaluationResponse, AuthZResolverError> {
            Ok(EvaluationResponse {
                decision: true,
                context: EvaluationResponseContext {
                    constraints: vec![Constraint {
                        predicates: vec![Predicate::In(InPredicate::new(
                            pep_properties::OWNER_TENANT_ID,
                            [self.tenant_id],
                        ))],
                    }],
                    ..EvaluationResponseContext::default()
                },
            })
        }
    }

    /// Mock PDP that returns two separate tenant constraints (OR-of-ANDs structure).
    struct MultiConstraintAuthZ {
        tenant_a: Uuid,
        tenant_b: Uuid,
    }

    #[async_trait]
    impl AuthZResolverClient for MultiConstraintAuthZ {
        async fn evaluate(
            &self,
            _request: EvaluationRequest,
        ) -> Result<EvaluationResponse, AuthZResolverError> {
            Ok(EvaluationResponse {
                decision: true,
                context: EvaluationResponseContext {
                    constraints: vec![
                        Constraint {
                            predicates: vec![Predicate::In(InPredicate::new(
                                pep_properties::OWNER_TENANT_ID,
                                [self.tenant_a],
                            ))],
                        },
                        Constraint {
                            predicates: vec![Predicate::In(InPredicate::new(
                                pep_properties::OWNER_TENANT_ID,
                                [self.tenant_b],
                            ))],
                        },
                    ],
                    ..EvaluationResponseContext::default()
                },
            })
        }
    }

    fn test_ctx() -> SecurityContext {
        SecurityContext::builder()
            .subject_id(Uuid::new_v4())
            .subject_tenant_id(Uuid::new_v4())
            .token_scopes(vec!["*".to_owned()])
            .build()
            .expect("valid SecurityContext")
    }

    // ── TEST-FDESIGN-001 unit tests ───────────────────────────────────────────

    /// TEST-FDESIGN-001 (1/4): PDP returns Err(Denied) → PermissionDenied (AuthorizationFailed).
    #[tokio::test]
    async fn test_authz_denied() {
        let ctx = test_ctx();
        let authz = Arc::new(DenyAuthZ);

        let result =
            authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

        assert!(
            matches!(result, Err(UsageCollectorError::AuthorizationFailed { .. })),
            "expected AuthorizationFailed, got {result:?}"
        );
    }

    /// TEST-FDESIGN-001 (2/4): Non-Denied PDP error → PermissionDenied (fail-closed).
    ///
    /// Verifies that a network/infrastructure error does not allow access through.
    #[tokio::test]
    async fn test_authz_non_denied_pdp_error() {
        let ctx = test_ctx();
        let authz = Arc::new(NetworkErrorAuthZ);

        let result =
            authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

        assert!(
            matches!(result, Err(UsageCollectorError::AuthorizationFailed { .. })),
            "expected AuthorizationFailed (fail-closed), got {result:?}"
        );
    }

    /// TEST-FDESIGN-001 (3/4): Single constraint → Ok(AccessScope) compiled correctly.
    #[tokio::test]
    async fn test_authz_single_constraint() {
        let tenant_id = Uuid::new_v4();
        let ctx = test_ctx();
        let authz = Arc::new(SingleConstraintAuthZ { tenant_id });

        let result =
            authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

        let scope = result.expect("expected Ok(AccessScope)");
        assert!(
            !scope.is_deny_all(),
            "scope must not be deny-all for a valid single constraint"
        );
        assert_eq!(
            scope.constraints().len(),
            1,
            "expected exactly one constraint group"
        );
        assert!(
            scope.contains_uuid(pep_properties::OWNER_TENANT_ID, tenant_id),
            "scope must contain the tenant_id from the single PDP constraint"
        );
    }

    /// TEST-FDESIGN-001 (4/4): Multiple constraints (OR-of-ANDs) → all groups preserved.
    ///
    /// Verifies that `compile_to_access_scope` inside PolicyEnforcer does not flatten
    /// multiple constraint groups into a single AND list (which would widen the scope
    /// and violate `cpt-cf-usage-collector-constraint-or-of-ands-preservation`).
    #[tokio::test]
    async fn test_authz_multi_constraint_or_of_ands() {
        let tenant_a = Uuid::new_v4();
        let tenant_b = Uuid::new_v4();
        let ctx = test_ctx();
        let authz = Arc::new(MultiConstraintAuthZ { tenant_a, tenant_b });

        let result =
            authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

        let scope = result.expect("expected Ok(AccessScope)");
        assert!(
            !scope.is_deny_all(),
            "scope must not be deny-all with two valid constraint groups"
        );
        assert_eq!(
            scope.constraints().len(),
            2,
            "both constraint groups must be preserved (OR-of-ANDs; flattening is a security violation)"
        );
        assert!(
            scope.contains_uuid(pep_properties::OWNER_TENANT_ID, tenant_a),
            "scope must contain tenant_a"
        );
        assert!(
            scope.contains_uuid(pep_properties::OWNER_TENANT_ID, tenant_b),
            "scope must contain tenant_b"
        );
    }
}
