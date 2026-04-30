use std::sync::Arc;

use async_trait::async_trait;
use authz_resolver_sdk::constraints::{Constraint, InPredicate, Predicate};
use authz_resolver_sdk::models::{
    EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError, DenyReason};
use modkit_macros::domain_model;
use modkit_security::SecurityContext;
use modkit_security::access_scope::pep_properties;
use usage_collector_sdk::UsageCollectorError;
use uuid::Uuid;

use super::{USAGE_RECORD_READ, actions, authorize_and_compile_scope};

// ── Mock AuthZ clients ────────────────────────────────────────────────────

/// Mock PDP that always returns a deny decision.
#[domain_model]
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
#[domain_model]
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
#[domain_model]
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
#[domain_model]
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

/// TEST-FDESIGN-001 (1/4): PDP returns Err(Denied) → `PermissionDenied` (`AuthorizationFailed`).
#[tokio::test]
async fn test_authz_denied() {
    let ctx = test_ctx();
    let authz = Arc::new(DenyAuthZ);

    let result = authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

    assert!(
        matches!(result, Err(UsageCollectorError::AuthorizationFailed { .. })),
        "expected AuthorizationFailed, got {result:?}"
    );
}

/// TEST-FDESIGN-001 (2/4): Non-Denied PDP error → `PermissionDenied` (fail-closed).
///
/// Verifies that a network/infrastructure error does not allow access through.
#[tokio::test]
async fn test_authz_non_denied_pdp_error() {
    let ctx = test_ctx();
    let authz = Arc::new(NetworkErrorAuthZ);

    let result = authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

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

    let result = authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

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
/// Verifies that `compile_to_access_scope` inside `PolicyEnforcer` does not flatten
/// multiple constraint groups into a single AND list (which would widen the scope
/// and violate `cpt-cf-usage-collector-constraint-or-of-ands-preservation`).
#[tokio::test]
async fn test_authz_multi_constraint_or_of_ands() {
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    let ctx = test_ctx();
    let authz = Arc::new(MultiConstraintAuthZ { tenant_a, tenant_b });

    let result = authorize_and_compile_scope(&ctx, authz, &USAGE_RECORD_READ, actions::LIST).await;

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
