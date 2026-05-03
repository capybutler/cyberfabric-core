#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use authz_resolver_sdk::models::{
    BarrierMode, EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::{AuthZResolverClient, AuthZResolverError};
use modkit_security::pep_properties;
use usage_collector_sdk::models::UsageRecord;
use usage_collector_sdk::{UsageCollectorClientV1, UsageCollectorError};
use usage_emitter::{UsageEmitter, UsageEmitterConfig, UsageEmitterError, UsageEmitterV1};
use uuid::Uuid;

const TEST_MODULE: &str = "test-module";

// ── Test-specific AuthZ mocks ─────────────────────────────────────────────────

struct AllowOwnerTenantIfInSet {
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
        let module_val = request
            .resource
            .properties
            .get("module")
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
        let subj_id = request
            .resource
            .properties
            .get("subject_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let subj_type = request
            .resource
            .properties
            .get("subject_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ok =
            subj_id == self.expected_subject_id && subj_type == self.expected_subject_type;
        *self.matched.lock().unwrap() = ok;
        Ok(EvaluationResponse {
            decision: ok,
            context: EvaluationResponseContext::default(),
        })
    }
}

struct AssertNoSubjectPropertiesAuthZ {
    asserted: Arc<Mutex<bool>>,
}

#[async_trait]
impl AuthZResolverClient for AssertNoSubjectPropertiesAuthZ {
    async fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, AuthZResolverError> {
        let has_subject_id = request.resource.properties.contains_key("subject_id");
        let has_subject_type = request.resource.properties.contains_key("subject_type");
        let ok = !has_subject_id && !has_subject_type;
        *self.asserted.lock().unwrap() = ok;
        Ok(EvaluationResponse {
            decision: ok,
            context: EvaluationResponseContext::default(),
        })
    }
}

// ── Test-specific collector mock ──────────────────────────────────────────────

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

async fn build_emitter(
    name: &str,
    authz: Arc<dyn AuthZResolverClient>,
) -> UsageEmitter {
    let db = common::build_db(name).await;
    UsageEmitter::build(
        UsageEmitterConfig::default(),
        db,
        authz,
        Arc::new(common::NoopCollector),
    )
    .await
    .unwrap()
}

async fn build_emitter_with_collector(
    name: &str,
    authz: Arc<dyn AuthZResolverClient>,
    collector: Arc<dyn UsageCollectorClientV1>,
) -> UsageEmitter {
    let db = common::build_db(name).await;
    UsageEmitter::build(UsageEmitterConfig::default(), db, authz, collector)
        .await
        .unwrap()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn build_creates_emitter_with_valid_config() {
    build_emitter("emit_build", Arc::new(common::AllowAllAuthZ)).await;
}

#[tokio::test]
async fn authorize_returns_handle_on_allow_all_authz() {
    let emitter = build_emitter("emit_authz_allow", Arc::new(common::AllowAllAuthZ)).await;
    let ctx = common::make_ctx();
    emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await
        .unwrap();
}

#[tokio::test]
async fn authorize_returns_error_on_deny_all_authz() {
    let emitter = build_emitter("emit_authz_deny", Arc::new(common::DenyAllAuthZ)).await;
    let ctx = common::make_ctx();
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
    let emitter = build_emitter("emit_authz_fail", Arc::new(common::FailingAuthZ)).await;
    let ctx = common::make_ctx();
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
    let emitter = build_emitter(
        "emit_pdp_deny_foreign",
        Arc::new(AllowOwnerTenantIfInSet {
            extra_allowed: vec![],
        }),
    )
    .await;

    let subject_tenant = Uuid::new_v4();
    let foreign_tenant = Uuid::new_v4();
    let ctx = modkit_security::SecurityContext::builder()
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
    let parent = Uuid::new_v4();
    let child = Uuid::new_v4();
    let emitter = build_emitter(
        "emit_pdp_allow_child",
        Arc::new(AllowOwnerTenantIfInSet {
            extra_allowed: vec![child],
        }),
    )
    .await;

    let ctx = modkit_security::SecurityContext::builder()
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
    let emitter = build_emitter("emit_barrier_ignore", Arc::new(AssertBarrierIgnoreAuthZ)).await;
    let ctx = common::make_ctx();
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

#[tokio::test]
async fn authorize_for_maps_collector_plugin_timeout_to_internal() {
    let emitter = build_emitter_with_collector(
        "emit_collector_timeout",
        Arc::new(common::AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: UsageCollectorError::plugin_timeout,
        }),
    )
    .await;
    let ctx = common::make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_maps_collector_circuit_open_to_internal() {
    let emitter = build_emitter_with_collector(
        "emit_collector_circuit_open",
        Arc::new(common::AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: UsageCollectorError::circuit_open,
        }),
    )
    .await;
    let ctx = common::make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_maps_collector_unavailable_to_internal() {
    let emitter = build_emitter_with_collector(
        "emit_collector_unavailable",
        Arc::new(common::AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageCollectorError::unavailable("gateway unreachable"),
        }),
    )
    .await;
    let ctx = common::make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_maps_collector_internal_error_to_internal() {
    let emitter = build_emitter_with_collector(
        "emit_collector_internal",
        Arc::new(common::AllowAllAuthZ),
        Arc::new(FailingCollector {
            error: || UsageCollectorError::internal("unexpected state"),
        }),
    )
    .await;
    let ctx = common::make_ctx();
    let result = emitter
        .for_module(TEST_MODULE)
        .authorize(&ctx, Uuid::new_v4(), "test.resource".to_owned())
        .await;
    assert!(matches!(result, Err(UsageEmitterError::Internal { .. })));
}

#[tokio::test]
async fn authorize_for_pdp_request_includes_module_resource_property() {
    let matched = Arc::new(Mutex::new(false));
    let emitter = build_emitter(
        "emit_module_prop",
        Arc::new(AssertModulePropertyAuthZ {
            expected_module: TEST_MODULE.to_owned(),
            matched: Arc::clone(&matched),
        }),
    )
    .await;
    let ctx = common::make_ctx();
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

#[tokio::test]
async fn authorize_for_pdp_request_includes_subject_id_and_subject_type_resource_properties() {
    let matched = Arc::new(Mutex::new(false));
    let subject_id = Uuid::new_v4();
    let subject_type = "test.subject.type".to_owned();
    let emitter = build_emitter(
        "emit_subject_props",
        Arc::new(AssertSubjectPropertiesAuthZ {
            expected_subject_id: subject_id.to_string(),
            expected_subject_type: subject_type.clone(),
            matched: Arc::clone(&matched),
        }),
    )
    .await;
    let ctx = common::make_ctx();
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
        "PDP request must include SUBJECT_ID and SUBJECT_TYPE resource properties"
    );
}

#[tokio::test]
async fn authorize_for_without_subject_succeeds() {
    let emitter = build_emitter("emit_no_subject_allow", Arc::new(common::AllowAllAuthZ)).await;
    let ctx = common::make_ctx();
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
    let emitter = build_emitter(
        "emit_no_subject_props",
        Arc::new(AssertNoSubjectPropertiesAuthZ {
            asserted: Arc::clone(&asserted),
        }),
    )
    .await;
    let ctx = common::make_ctx();
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
        "PDP request must NOT include SUBJECT_ID or SUBJECT_TYPE when subject is None"
    );
}
