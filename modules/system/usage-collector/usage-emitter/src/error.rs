//! Error types for the usage emitter crate.
//!
//! [`UsageEmitterError`] is a type alias over [`CanonicalError`]: emitter operations report
//! failures using the canonical error taxonomy used elsewhere in the platform, so callers can
//! match on `CanonicalError::PermissionDenied`, `CanonicalError::Unauthenticated`, etc., without
//! a crate-specific error enum.

use authz_resolver_sdk::EnforcerError;
use modkit_canonical_errors::CanonicalError;
use tracing::error;
use usage_collector_sdk::error::UsageRecordError;

/// Errors returned by the usage emitter pipeline.
///
/// Alias of [`CanonicalError`] — pattern-match on its variants directly.
pub type UsageEmitterError = CanonicalError;

/// Maps a PDP [`EnforcerError`] to a fail-closed [`UsageEmitterError`].
///
/// All enforcer outcomes (denial, compile failure, evaluation failure) map to
/// [`CanonicalError::PermissionDenied`] with reason `AUTHORIZATION_DENIED` so that PDP
/// infrastructure issues never leak as availability problems to source modules.
#[allow(clippy::needless_pass_by_value)]
pub fn enforcer_error_to_emitter_error(e: EnforcerError) -> UsageEmitterError {
    match e {
        EnforcerError::Denied { .. } => UsageRecordError::permission_denied()
            .with_reason("AUTHORIZATION_DENIED")
            .create(),
        EnforcerError::CompileFailed(source) => {
            error!(
                pdp_error_variant = "CompileFailed",
                error = %source,
                "PDP constraint compilation failed; access denied (fail-closed)",
            );
            UsageRecordError::permission_denied()
                .with_reason("AUTHORIZATION_DENIED")
                .create()
        }
        EnforcerError::EvaluationFailed(source) => {
            error!(
                pdp_error_variant = "EvaluationFailed",
                error = %source,
                "PDP policy evaluation failed; access denied (fail-closed)",
            );
            UsageRecordError::permission_denied()
                .with_reason("AUTHORIZATION_DENIED")
                .create()
        }
    }
}
