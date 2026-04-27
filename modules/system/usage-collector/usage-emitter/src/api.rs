use crate::scoped_emitter::ScopedUsageEmitter;

/// Source-facing trait for emitting usage records with module-scoped authorization.
///
/// Obtain from `ClientHub`: `hub.get::<dyn UsageEmitterV1>()?`
///
/// The emitter is registered in `ClientHub` by the `usage-collector` module (in-process delivery)
/// or the `usage-collector-rest-client` module (HTTP delivery to a remote collector).
///
/// Call [`Self::for_module`] with the module's name constant to obtain a [`ScopedUsageEmitter`]
/// that carries the module name and knows the allowed metrics for that module.
///
/// ```ignore
/// // In init():
/// let emitter = hub.get::<dyn UsageEmitterV1>()?;
/// let scoped = emitter.for_module(Self::MODULE_NAME);
///
/// // In handlers:
/// let authorized = scoped
///     .authorize(&ctx, resource_id, "resource_type".to_owned())
///     .await?;
/// authorized
///     .build_usage_record("requests", 1.0)
///     .enqueue()
///     .await?;
/// ```
pub trait UsageEmitterV1: Send + Sync {
    /// Obtain a [`ScopedUsageEmitter`] bound to `module_name`.
    ///
    /// The scoped emitter stores the module name and uses it to fetch the allowed metrics list
    /// from the collector during [`ScopedUsageEmitter::authorize_for`].
    fn for_module(&self, module_name: &str) -> ScopedUsageEmitter;
}
