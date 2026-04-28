use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use authz_resolver_sdk::models::{
    BarrierMode, EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::pep::ConstraintCompileError;
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError, DenyReason, EnforcerError};
use modkit_db::migration_runner::run_migrations_for_testing;
use modkit_db::outbox::outbox_migrations;
use modkit_db::{ConnectOpts, Db, connect_db};
use modkit_security::SecurityContext;
use modkit_security::pep_properties;
use usage_collector_sdk::models::UsageRecord;
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use uuid::Uuid;

use super::{UsageEmitter, enforcer_error_to_emitter_error};
use crate::UsageEmitterV1;
use crate::config::UsageEmitterConfig;
use crate::error::UsageEmitterError;

const TEST_MODULE: &str = "test-module";

// ── Mock AuthZ clients ────────────────────────────────────────────────────────

struct AllowAllAuthZ;

#[async_trait]
impl AuthZResolverClient for AllowAllAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Ok(EvaluationResponse {
            decision: true,
            context: EvaluationResponseContext::default(),
        })
    }
}

struct DenyAllAuthZ;

#[async_trait]
impl AuthZResolverClient for DenyAllAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Ok(EvaluationResponse {
            decision: false,
            context: EvaluationResponseContext::default(),
        })
    }
}

struct FailingAuthZ;

#[async_trait]
impl AuthZResolverClient for FailingAuthZ {
    async fn evaluate(
        &self,
        _request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        Err(AuthZResolverError::Internal("PDP unavailable".to_owned()))
    }
}

/// PDP mock: allow only if `owner_tenant_id` matches the subject's home tenant or an allowed extra id.
struct AllowOwnerTenantIfInSet {
    /// Tenants allowed in addition to the subject's `tenant_id` (e.g. sub-tenants).
    extra_allowed: Vec<Uuid>,
}

#[async_trait]
impl AuthZResolverClient for AllowOwnerTenantIfInSet {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        let subject_tenant = request
            .subject
            .properties
            .get("tenant_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());

        let Some(subject_tenant) = subject_tenant else {
            return Ok(EvaluationResponse {
                decision: false,
                context: EvaluationResponseContext::default(),
            });
        };

        let owner = request
            .resource
            .properties
            .get(pep_properties::OWNER_TENANT_ID)
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());

        let Some(owner) = owner else {
            return Ok(EvaluationResponse {
                decision: false,
                context: EvaluationResponseContext::default(),
            });
        };

        let allowed = owner == subject_tenant || self.extra_allowed.contains(&owner);

        Ok(EvaluationResponse {
            decision: allowed,
            context: EvaluationResponseContext::default(),
        })
    }
}

/// Asserts `tenant_context.barrier_mode` is `Ignore` and returns allow.
struct AssertBarrierIgnoreAuthZ;

#[async_trait]
impl AuthZResolverClient for AssertBarrierIgnoreAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        let barrier = request
            .context
            .tenant_context
            .as_ref()
            .expect("tenant_context set by AccessRequest::barrier_mode")
            .barrier_mode;
        assert_eq!(barrier, BarrierMode::Ignore);
        Ok(EvaluationResponse {
            decision: true,
            context: EvaluationResponseContext::default(),
        })
    }
}

// ── Mock collector ────────────────────────────────────────────────────────────

struct NoopCollector;

#[async_trait]
impl UsageCollectorClientV1 for NoopCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![],
        })
    }
}

struct FailingCollector {
    error: fn() -> UsageCollectorError,
}

#[async_trait]
impl UsageCollectorClientV1 for FailingCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageCollectorError> {
        Err((self.error)())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn build_db(name: &str) -> Db {
    let url = format!("sqlite:file:{name}?mode=memory&cache=shared");
    let db = connect_db(
        &url,
        ConnectOpts {
            max_conns: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    run_migrations_for_testing(&db, outbox_migrations())
        .await
        .unwrap();
    db
}

async fn build_emitter(db: Db, authz: Arc<dyn AuthZResolverClient>) -> UsageEmitter {
    UsageEmitter::build(
        UsageEmitterConfig::default(),
        db,
        authz,
        Arc::new(NoopCollector),
    )
    .await
    .unwrap()
}

async fn build_emitter_with_collector(
    db: Db,
    authz: Arc<dyn AuthZResolverClient>,
    collector: Arc<dyn UsageCollectorClientV1>,
) -> UsageEmitter {
    UsageEmitter::build(UsageEmitterConfig::default(), db, authz, collector)
        .await
        .unwrap()
}

fn make_ctx() -> SecurityContext {
    SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(Uuid::new_v4())
        .build()
        .unwrap()
}

// ── enforcer_error_to_emitter_error ──────────────────────────────────────────

#[test]
fn enforcer_denied_without_reason_uses_default_message() {
    let err = enforcer_error_to_emitter_error(EnforcerError::Denied { deny_reason: None });
    assert!(
        matches!(err, UsageEmitterError::AuthorizationFailed { ref message } if message == "access denied by policy")
    );
}

#[test]
fn enforcer_denied_with_code_and_details() {
    let deny_reason = Some(DenyReason {
        error_code: "ERR_FORBIDDEN".to_owned(),
        details: Some("tenant not allowed".to_owned()),
    });
    let err = enforcer_error_to_emitter_error(EnforcerError::Denied { deny_reason });
    assert!(
        matches!(err, UsageEmitterError::AuthorizationFailed { ref message } if message == "ERR_FORBIDDEN: tenant not allowed")
    );
}

#[test]
fn enforcer_denied_with_code_no_details() {
    let deny_reason = Some(DenyReason {
        error_code: "ERR_FORBIDDEN".to_owned(),
        details: None,
    });
    let err = enforcer_error_to_emitter_error(EnforcerError::Denied { deny_reason });
    assert!(
        matches!(err, UsageEmitterError::AuthorizationFailed { ref message } if message == "ERR_FORBIDDEN")
    );
}

#[test]
fn enforcer_compile_failed_maps_to_internal() {
    let err = enforcer_error_to_emitter_error(EnforcerError::CompileFailed(
        ConstraintCompileError::ConstraintsRequiredButAbsent,
    ));
    assert!(matches!(err, UsageEmitterError::Internal { .. }));
}

#[test]
fn enforcer_evaluation_failed_maps_to_internal() {
    let err = enforcer_error_to_emitter_error(EnforcerError::EvaluationFailed(
        AuthZResolverError::Internal("rpc error".to_owned()),
    ));
    assert!(matches!(err, UsageEmitterError::Internal { .. }));
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn build_creates_emitter_with_valid_config() {
    let db = build_db("emit_build").await;
    build_emitter(db, Arc::new(AllowAllAuthZ)).await;
}

#[tokio::test]
async fn authorize_returns_handle_on_allow_all_authz() {
    let db = build_db("emit_authz_allow").await;
    let emitter = build_emitter(db, Arc::new(AllowAllAuthZ)).await;
    let ctx = make_ctx();
    emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await
        .unwrap();
}

#[tokio::test]
async fn authorize_returns_error_on_deny_all_authz() {
    let db = build_db("emit_authz_deny").await;
    let emitter = build_emitter(db, Arc::new(DenyAllAuthZ)).await;
    let ctx = make_ctx();
    let Err(err) = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await
    else {
        panic!("expected authorization to fail");
    };
    assert!(matches!(err, UsageEmitterError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn authorize_returns_internal_error_on_authz_failure() {
    let db = build_db("emit_authz_fail").await;
    let emitter = build_emitter(db, Arc::new(FailingAuthZ)).await;
    let ctx = make_ctx();
    let Err(err) = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await
    else {
        panic!("expected authorization to fail");
    };
    assert!(matches!(err, UsageEmitterError::Internal { .. }));
}

#[tokio::test]
async fn authorize_for_denies_when_pdp_rejects_owner_tenant() {
    let db = build_db("emit_pdp_deny_foreign").await;
    let emitter = build_emitter(
        db,
        Arc::new(AllowOwnerTenantIfInSet {
            extra_allowed: vec![],
        }),
    )
    .await;

    let subject_tenant = Uuid::new_v4();
    let foreign_tenant = Uuid::new_v4();
    let ctx = SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(subject_tenant)
        .build()
        .unwrap();

    let Err(err) = emitter
        .for_module(TEST_MODULE)
        .authorize_for(
            &ctx,
            foreign_tenant,
            Uuid::new_v4(),
            "test.resource".to_owned(),
            Some(Uuid::new_v4()),
            Some("test.subject".to_owned()),
        )
        .await
    else {
        panic!("expected authorization to fail");
    };
    assert!(matches!(err, UsageEmitterError::AuthorizationFailed { .. }));
}

#[tokio::test]
async fn authorize_for_allows_subtenant_when_pdp_allows_extra_tenant() {
    let db = build_db("emit_pdp_allow_child").await;
    let parent = Uuid::new_v4();
    let child = Uuid::new_v4();
    let emitter = build_emitter(
        db,
        Arc::new(AllowOwnerTenantIfInSet {
            extra_allowed: vec![child],
        }),
    )
    .await;

    let ctx = SecurityContext::builder()
        .subject_id(Uuid::new_v4())
        .subject_tenant_id(parent)
        .build()
        .unwrap();

    emitter
        .for_module(TEST_MODULE)
        .authorize_for(
            &ctx,
            child,
            Uuid::new_v4(),
            "test.resource".to_owned(),
            Some(Uuid::new_v4()),
            Some("test.subject".to_owned()),
        )
        .await
        .expect("PDP allows emit for allowed sub-tenant");
}

#[tokio::test]
async fn authorize_for_sends_barrier_mode_ignore_to_pdp() {
    let db = build_db("emit_barrier_ignore").await;
    let emitter = build_emitter(db, Arc::new(AssertBarrierIgnoreAuthZ)).await;
    let ctx = make_ctx();
    emitter
        .for_module(TEST_MODULE)
        .authorize_for(
            &ctx,
            ctx.subject_tenant_id(),
            Uuid::new_v4(),
            "test.resource".to_owned(),
            Some(Uuid::new_v4()),
            Some("test.subject".to_owned()),
        )
        .await
        .expect("barrier assert + allow");
}

// ── Collector infrastructure failures map to Internal (not AuthorizationFailed) ──

#[tokio::test]
async fn authorize_for_maps_collector_plugin_timeout_to_internal() {
    let db = build_db("emit_collector_timeout").await;
    let emitter = build_emitter_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: UsageCollectorError::plugin_timeout,
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_maps_collector_circuit_open_to_internal() {
    let db = build_db("emit_collector_circuit_open").await;
    let emitter = build_emitter_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: UsageCollectorError::circuit_open,
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_maps_collector_unavailable_to_internal() {
    let db = build_db("emit_collector_unavailable").await;
    let emitter = build_emitter_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageCollectorError::unavailable("gateway unreachable"),
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_maps_collector_internal_error_to_internal() {
    let db = build_db("emit_collector_internal").await;
    let emitter = build_emitter_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageCollectorError::internal("unexpected state"),
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

// ── PDP resource property assertions ─────────────────────────────────────────

/// PDP mock that asserts the MODULE resource property equals the expected value,
/// denies if the assertion fails, otherwise allows.
struct AssertModulePropertyAuthZ {
    expected_module: String,
    matched: Arc<Mutex<bool>>,
}

#[async_trait]
impl AuthZResolverClient for AssertModulePropertyAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        use crate::domain::authz::properties;
        let module_val = request
            .resource
            .properties
            .get(properties::MODULE)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ok = module_val == self.expected_module;
        *self.matched.lock().unwrap() = ok;
        Ok(EvaluationResponse {
            decision: ok,
            context: EvaluationResponseContext::default(),
        })
    }
}

/// PDP mock that asserts `SUBJECT_ID` and `SUBJECT_TYPE` resource properties equal
/// the expected values, denies if either assertion fails, otherwise allows.
struct AssertSubjectPropertiesAuthZ {
    expected_subject_id: String,
    expected_subject_type: String,
    matched: Arc<Mutex<bool>>,
}

#[async_trait]
impl AuthZResolverClient for AssertSubjectPropertiesAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        use crate::domain::authz::properties;
        let subj_id = request
            .resource
            .properties
            .get(properties::SUBJECT_ID)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subj_type = request
            .resource
            .properties
            .get(properties::SUBJECT_TYPE)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ok = subj_id == self.expected_subject_id && subj_type == self.expected_subject_type;
        *self.matched.lock().unwrap() = ok;
        Ok(EvaluationResponse {
            decision: ok,
            context: EvaluationResponseContext::default(),
        })
    }
}

#[tokio::test]
async fn authorize_for_pdp_request_includes_module_resource_property() {
    let matched = Arc::new(Mutex::new(false));
    let db = build_db("emit_module_prop").await;
    let emitter = build_emitter(
        db,
        Arc::new(AssertModulePropertyAuthZ {
            expected_module: TEST_MODULE.to_owned(),
            matched: Arc::clone(&matched),
        }),
    )
    .await;
    let ctx = make_ctx();
    drop(
        emitter
            .for_module(TEST_MODULE)
            .authorize_for(
                &ctx,
                ctx.subject_tenant_id(),
                Uuid::new_v4(),
                "test.resource".to_owned(),
                Some(Uuid::new_v4()),
                Some("test.subject".to_owned()),
            )
            .await,
    );
    assert!(
        *matched.lock().unwrap(),
        "PDP request must include MODULE resource property equal to the module name"
    );
}

/// PDP mock that asserts `SUBJECT_ID` and `SUBJECT_TYPE` resource properties equal
/// the expected values (with subject present), denies if either assertion fails, otherwise allows.
/// This mock documents the "with subject" variant of the subject-properties test.
#[tokio::test]
async fn authorize_for_pdp_request_includes_subject_id_and_subject_type_resource_properties() {
    let matched = Arc::new(Mutex::new(false));
    let subject_id = Uuid::new_v4();
    let subject_type = "test.subject.type".to_owned();
    let db = build_db("emit_subject_props").await;
    let emitter = build_emitter(
        db,
        Arc::new(AssertSubjectPropertiesAuthZ {
            expected_subject_id: subject_id.to_string(),
            expected_subject_type: subject_type.clone(),
            matched: Arc::clone(&matched),
        }),
    )
    .await;
    let ctx = make_ctx();
    drop(
        emitter
            .for_module(TEST_MODULE)
            .authorize_for(
                &ctx,
                ctx.subject_tenant_id(),
                Uuid::new_v4(),
                "test.resource".to_owned(),
                Some(subject_id),
                Some(subject_type),
            )
            .await,
    );
    assert!(
        *matched.lock().unwrap(),
        "PDP request must include SUBJECT_ID and SUBJECT_TYPE resource properties with the passed values"
    );
}

/// PDP mock that asserts neither `SUBJECT_ID` nor `SUBJECT_TYPE` resource property is present
/// in the PDP request, returns allow if the assertion passes, deny otherwise.
struct AssertNoSubjectPropertiesAuthZ {
    asserted: Arc<Mutex<bool>>,
}

#[async_trait]
impl AuthZResolverClient for AssertNoSubjectPropertiesAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        use crate::domain::authz::properties;
        let has_subject_id = request
            .resource
            .properties
            .contains_key(properties::SUBJECT_ID);
        let has_subject_type = request
            .resource
            .properties
            .contains_key(properties::SUBJECT_TYPE);
        let ok = !has_subject_id && !has_subject_type;
        *self.asserted.lock().unwrap() = ok;
        Ok(EvaluationResponse {
            decision: ok,
            context: EvaluationResponseContext::default(),
        })
    }
}

#[tokio::test]
async fn authorize_for_without_subject_succeeds() {
    let db = build_db("emit_no_subject_allow").await;
    let emitter = build_emitter(db, Arc::new(AllowAllAuthZ)).await;
    let ctx = make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize_for(
            &ctx,
            ctx.subject_tenant_id(),
            Uuid::new_v4(),
            "test.resource".to_owned(),
            None,
            None,
        )
        .await;
    assert!(
        result.is_ok(),
        "authorize_for with None subject must succeed when PDP allows"
    );
}

#[tokio::test]
async fn authorize_for_pdp_request_omits_subject_properties_when_none() {
    let asserted = Arc::new(Mutex::new(false));
    let db = build_db("emit_no_subject_props").await;
    let emitter = build_emitter(
        db,
        Arc::new(AssertNoSubjectPropertiesAuthZ {
            asserted: Arc::clone(&asserted),
        }),
    )
    .await;
    let ctx = make_ctx();
    drop(
        emitter
            .for_module(TEST_MODULE)
            .authorize_for(
                &ctx,
                ctx.subject_tenant_id(),
                Uuid::new_v4(),
                "test.resource".to_owned(),
                None,
                None,
            )
            .await,
    );
    assert!(
        *asserted.lock().unwrap(),
        "PDP request must NOT include SUBJECT_ID or SUBJECT_TYPE resource properties when subject is None"
    );
}
