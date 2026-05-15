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
use usage_collector_sdk::models::{Subject, UsageRecord};
use usage_collector_sdk::{UsageCollectorClientV1, UsageRecordError};
use uuid::Uuid;

use super::UsageEmitterFactory;
use crate::api::UsageEmitterRuntimeV1;
use crate::config::UsageEmitterConfig;
use crate::domain::runtime::UsageEmitterRuntime;
use crate::error::{UsageEmitterError, enforcer_error_to_emitter_error};

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
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageEmitterError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageEmitterError> {
        Ok(usage_collector_sdk::ModuleConfig {
            allowed_metrics: vec![],
            // Non-default value so factory-plumbing tests can prove the
            // emitter copies `max_metadata_bytes` from the collector
            // response rather than re-defaulting.
            max_metadata_bytes: 4096,
        })
    }
}

struct FailingCollector {
    error: fn() -> UsageEmitterError,
}

#[async_trait]
impl UsageCollectorClientV1 for FailingCollector {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageEmitterError> {
        Ok(())
    }

    async fn get_module_config(
        &self,
        _module_name: &str,
    ) -> Result<usage_collector_sdk::ModuleConfig, UsageEmitterError> {
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

async fn build_factory(db: Db, authz: Arc<dyn AuthZResolverClient>) -> UsageEmitterFactory {
    let runtime = UsageEmitterRuntime::build(
        UsageEmitterConfig::default(),
        db,
        authz,
        Arc::new(NoopCollector),
    )
    .await
    .unwrap();
    runtime.factory(TEST_MODULE)
}

async fn build_factory_with_collector(
    db: Db,
    authz: Arc<dyn AuthZResolverClient>,
    collector: Arc<dyn UsageCollectorClientV1>,
) -> UsageEmitterFactory {
    let runtime = UsageEmitterRuntime::build(UsageEmitterConfig::default(), db, authz, collector)
        .await
        .unwrap();
    runtime.factory(TEST_MODULE)
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
fn enforcer_denied_maps_to_permission_denied() {
    let err = enforcer_error_to_emitter_error(EnforcerError::Denied { deny_reason: None });
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[test]
fn enforcer_denied_with_code_and_details() {
    let deny_reason = Some(DenyReason {
        error_code: "ERR_FORBIDDEN".to_owned(),
        details: Some("tenant not allowed".to_owned()),
    });
    let err = enforcer_error_to_emitter_error(EnforcerError::Denied { deny_reason });
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[test]
fn enforcer_denied_with_code_no_details() {
    let deny_reason = Some(DenyReason {
        error_code: "ERR_FORBIDDEN".to_owned(),
        details: None,
    });
    let err = enforcer_error_to_emitter_error(EnforcerError::Denied { deny_reason });
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[test]
fn enforcer_compile_failed_maps_to_permission_denied() {
    let err = enforcer_error_to_emitter_error(EnforcerError::CompileFailed(
        ConstraintCompileError::ConstraintsRequiredButAbsent,
    ));
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[test]
fn enforcer_evaluation_failed_maps_to_permission_denied() {
    let err = enforcer_error_to_emitter_error(EnforcerError::EvaluationFailed(
        AuthZResolverError::Internal("rpc error".to_owned()),
    ));
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn build_creates_emitter_with_valid_config() {
    let db = build_db("emit_build").await;
    build_factory(db, Arc::new(AllowAllAuthZ)).await;
}

#[tokio::test]
async fn authorize_returns_handle_on_allow_all_authz() {
    let db = build_db("emit_authz_allow").await;
    let factory = build_factory(db, Arc::new(AllowAllAuthZ)).await;
    let ctx = make_ctx();
    factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
        .unwrap();
}

#[tokio::test]
async fn authorize_populates_max_metadata_bytes_from_module_config() {
    // `NoopCollector::get_module_config` returns `max_metadata_bytes: 4096`
    // (a non-default value). The authorized emitter must surface that exact
    // value, proving the factory copies the field from the collector
    // response rather than substituting a local default.
    let db = build_db("emit_max_metadata_bytes_plumbed").await;
    let factory = build_factory(db, Arc::new(AllowAllAuthZ)).await;
    let ctx = make_ctx();
    let emitter = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
        .unwrap();
    assert_eq!(emitter.max_metadata_bytes, 4096);
}

#[tokio::test]
async fn authorize_returns_error_on_deny_all_authz() {
    let db = build_db("emit_authz_deny").await;
    let factory = build_factory(db, Arc::new(DenyAllAuthZ)).await;
    let ctx = make_ctx();
    let Err(err) = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
    else {
        panic!("expected authorization to fail");
    };
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn authorize_returns_permission_denied_on_authz_failure() {
    let db = build_db("emit_authz_fail").await;
    let factory = build_factory(db, Arc::new(FailingAuthZ)).await;
    let ctx = make_ctx();
    let Err(err) = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
    else {
        panic!("expected authorization to fail");
    };
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn authorize_denies_when_pdp_rejects_owner_tenant() {
    let db = build_db("emit_pdp_deny_foreign").await;
    let factory = build_factory(
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

    let Err(err) = factory
        .clone()
        .with_tenant(foreign_tenant)
        .with_subject(Subject::with_type(Uuid::new_v4(), "test.subject"))
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
    else {
        panic!("expected authorization to fail");
    };
    assert!(matches!(err, UsageEmitterError::PermissionDenied { .. }));
}

#[tokio::test]
async fn authorize_allows_subtenant_when_pdp_allows_extra_tenant() {
    let db = build_db("emit_pdp_allow_child").await;
    let parent = Uuid::new_v4();
    let child = Uuid::new_v4();
    let factory = build_factory(
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

    factory
        .clone()
        .with_tenant(child)
        .with_subject(Subject::with_type(Uuid::new_v4(), "test.subject"))
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
        .expect("PDP allows emit for allowed sub-tenant");
}

#[tokio::test]
async fn authorize_sends_barrier_mode_ignore_to_pdp() {
    let db = build_db("emit_barrier_ignore").await;
    let factory = build_factory(db, Arc::new(AssertBarrierIgnoreAuthZ)).await;
    let ctx = make_ctx();
    factory
        .clone()
        .with_tenant(ctx.subject_tenant_id())
        .with_subject(Subject::with_type(Uuid::new_v4(), "test.subject"))
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
        .expect("barrier assert + allow");
}

// ── Collector infrastructure failures propagate the canonical variant ──

#[tokio::test]
async fn authorize_propagates_collector_deadline_exceeded() {
    let db = build_db("emit_collector_timeout").await;
    let factory = build_factory_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageRecordError::deadline_exceeded("plugin timed out").create(),
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await;
    assert!(matches!(
        result,
        Err(UsageEmitterError::DeadlineExceeded { .. })
    ));
}

#[tokio::test]
async fn authorize_propagates_collector_service_unavailable() {
    let db = build_db("emit_collector_unavailable").await;
    let factory = build_factory_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageEmitterError::service_unavailable().create(),
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await;
    assert!(matches!(
        result,
        Err(UsageEmitterError::ServiceUnavailable { .. })
    ));
}

#[tokio::test]
async fn authorize_propagates_collector_resource_exhausted() {
    let db = build_db("emit_collector_resource_exhausted").await;
    let factory = build_factory_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || {
                UsageRecordError::resource_exhausted("rate limited")
                    .with_quota_violation("requests", "rate limit exceeded")
                    .create()
            },
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await;
    assert!(matches!(
        result,
        Err(UsageEmitterError::ResourceExhausted { .. })
    ));
}

#[tokio::test]
async fn authorize_propagates_collector_internal_error() {
    let db = build_db("emit_collector_internal").await;
    let factory = build_factory_with_collector(
        db,
        Arc::new(AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageEmitterError::internal("unexpected state").create(),
        }),
    )
    .await;
    let ctx = make_ctx();
    let result = factory
        .clone()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
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
        use usage_collector_sdk::authz::properties;
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
        use usage_collector_sdk::authz::properties;
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
async fn authorize_pdp_request_includes_module_resource_property() {
    let matched = Arc::new(Mutex::new(false));
    let db = build_db("emit_module_prop").await;
    let factory = build_factory(
        db,
        Arc::new(AssertModulePropertyAuthZ {
            expected_module: TEST_MODULE.to_owned(),
            matched: Arc::clone(&matched),
        }),
    )
    .await;
    let ctx = make_ctx();
    drop(
        factory
            .clone()
            .with_tenant(ctx.subject_tenant_id())
            .with_subject(Subject::with_type(Uuid::new_v4(), "test.subject"))
            .authorize(&ctx, Uuid::new_v4(), "test.resource")
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
async fn authorize_pdp_request_includes_subject_id_and_subject_type_resource_properties() {
    let matched = Arc::new(Mutex::new(false));
    let subject_id = Uuid::new_v4();
    let subject_type = "test.subject.type".to_owned();
    let db = build_db("emit_subject_props").await;
    let factory = build_factory(
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
        factory
            .clone()
            .with_tenant(ctx.subject_tenant_id())
            .with_subject(Subject::with_type(subject_id, subject_type))
            .authorize(&ctx, Uuid::new_v4(), "test.resource")
            .await,
    );
    assert!(
        *matched.lock().unwrap(),
        "PDP request must include SUBJECT_ID and SUBJECT_TYPE resource properties with the passed values"
    );
}

/// Captures the `SUBJECT_ID` / `SUBJECT_TYPE` resource properties as `Option<String>` so
/// tests can distinguish "property absent" from "property present with some value".
/// Always allows.
struct CapturingSubjectPropertiesAuthZ {
    captured_subject_id: Arc<Mutex<Option<String>>>,
    captured_subject_type: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl AuthZResolverClient for CapturingSubjectPropertiesAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        use usage_collector_sdk::authz::properties;
        let subj_id = request
            .resource
            .properties
            .get(properties::SUBJECT_ID)
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let subj_type = request
            .resource
            .properties
            .get(properties::SUBJECT_TYPE)
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        *self.captured_subject_id.lock().unwrap() = subj_id;
        *self.captured_subject_type.lock().unwrap() = subj_type;
        Ok(EvaluationResponse {
            decision: true,
            context: EvaluationResponseContext::default(),
        })
    }
}

#[tokio::test]
async fn authorize_without_subject_omits_subject_properties_from_pdp_request() {
    // `.without_subject()` is the explicit "no subject" intent. The factory
    // binds `subject = None` and the PDP request carries neither SUBJECT_ID
    // nor SUBJECT_TYPE — the ctx-derived subject is *not* substituted.
    let captured_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_type: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let db = build_db("emit_without_subject_pdp_props").await;
    let factory = build_factory(
        db,
        Arc::new(CapturingSubjectPropertiesAuthZ {
            captured_subject_id: Arc::clone(&captured_id),
            captured_subject_type: Arc::clone(&captured_type),
        }),
    )
    .await;
    let ctx = make_ctx();
    factory
        .clone()
        .with_tenant(ctx.subject_tenant_id())
        .without_subject()
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
        .expect("authorize must succeed when PDP allows and no subject is bound");
    assert!(
        captured_id.lock().unwrap().is_none(),
        "PDP must receive no SUBJECT_ID when `without_subject()` is used"
    );
    assert!(
        captured_type.lock().unwrap().is_none(),
        "PDP must receive no SUBJECT_TYPE when `without_subject()` is used"
    );
}

#[tokio::test]
async fn authorize_default_subject_choice_falls_back_to_ctx_subject_in_pdp() {
    // Neither `.with_subject(...)` nor `.without_subject()` was called —
    // `SubjectChoice::DefaultFromCtx` (the runtime default) makes the
    // factory resolve the subject from `SecurityContext` at `.authorize()`
    // time. Ctx in this test has no subject type, so SUBJECT_TYPE must be
    // absent in the PDP request.
    let captured_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_type: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let db = build_db("emit_default_subject_pdp_fallback").await;
    let factory = build_factory(
        db,
        Arc::new(CapturingSubjectPropertiesAuthZ {
            captured_subject_id: Arc::clone(&captured_id),
            captured_subject_type: Arc::clone(&captured_type),
        }),
    )
    .await;
    let ctx = make_ctx();
    factory
        .clone()
        .with_tenant(ctx.subject_tenant_id())
        .authorize(&ctx, Uuid::new_v4(), "test.resource")
        .await
        .expect("ctx-derived subject is sent and PDP allows");
    assert_eq!(
        captured_id.lock().unwrap().as_deref(),
        Some(ctx.subject_id().to_string().as_str()),
        "PDP must receive the ctx-derived SUBJECT_ID when neither with_subject nor without_subject was called"
    );
    assert!(
        captured_type.lock().unwrap().is_none(),
        "PDP must omit SUBJECT_TYPE when ctx has no subject type"
    );
}
