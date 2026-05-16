//! Durable usage emission for source modules.
//!
//! Provides the complete emitter pipeline: module-scoped PDP authorization, transactional
//! outbox enqueue, and async delivery to the usage collector via `modkit-db` outbox workers.
//!
//! # Usage
//!
//! Source modules should not construct [`UsageEmitterFactory`] directly. The `usage-collector`
//! or `usage-collector-rest-client` `ModKit` module builds and registers
//! `dyn UsageEmitterFactoryV1` in `ClientHub` during `init()`.
//!
//! ```ignore
//! // In init():
//! let factory = hub.get::<dyn UsageEmitterFactoryV1>()?;
//! let emitter = factory.for_module(Self::MODULE_NAME);
//!
//! // In a handler:
//! let authorized = emitter
//!     .authorize(&ctx, resource_id, "resource_type".to_owned())
//!     .await?;
//! authorized
//!     .build_usage_record("requests", 1.0)
//!     .enqueue()
//!     .await?;
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod api;
mod config;
mod domain;
mod error;
mod infra;

pub use api::UsageEmitterFactoryV1;
pub use config::UsageEmitterConfig;
pub use domain::authorized_emitter::AuthorizedUsageEmitter;
pub use domain::emitter::UsageEmitter;
pub use domain::factory::UsageEmitterFactory;
pub use domain::usage_record_builder::UsageRecordBuilder;
pub use error::UsageEmitterError;
pub use infra::delivery_handler::DeliveryHandler;
