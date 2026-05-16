use crate::domain::emitter::UsageEmitter;

/// Source-facing trait for emitting usage records with module-scoped authorization.
///
/// Obtain from `ClientHub`: `hub.get::<dyn UsageEmitterFactoryV1>()?`
///
/// The emitter factory is registered in `ClientHub` by the `usage-collector` module
/// (in-process delivery) or the `usage-collector-rest-client` module (HTTP delivery to a
/// remote collector).
///
/// Call [`Self::for_module`] with the module's name constant to obtain a [`UsageEmitter`]
/// that carries the module name and knows the allowed metrics for that module.
///
/// ```ignore
/// // In init():
/// let factory = hub.get::<dyn UsageEmitterFactoryV1>()?;
/// let emitter = factory.for_module(Self::MODULE_NAME);
///
/// // In handlers:
/// let authorized = emitter
///     .authorize(&ctx, resource_id, "resource_type".to_owned())
///     .await?;
/// authorized
///     .build_usage_record("requests", 1.0)
///     .enqueue()
///     .await?;
/// ```
pub trait UsageEmitterFactoryV1: Send + Sync {
    /// Obtain a [`UsageEmitter`] bound to `module_name`.
    ///
    /// The returned emitter stores the module name and uses it to fetch the allowed metrics list
    /// from the collector during [`UsageEmitter::authorize_for`].
    fn for_module(&self, module_name: &str) -> UsageEmitter;
}
