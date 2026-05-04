//! Durable usage emission for source modules.
//!
//! Provides the complete emitter pipeline: module-scoped PDP authorization, transactional
//! outbox enqueue, and async delivery to the usage collector via `modkit-db` outbox workers.
//!
//! # Usage
//!
//! Source modules should not construct `UsageEmitter` directly. The `usage-collector`
//! or `usage-collector-rest-client` `ModKit` module builds and registers
//! `dyn UsageEmitterV1` in `ClientHub` during `init()`.
//!
//! ```ignore
//! // In init():
//! let emitter = hub.get::<dyn UsageEmitterV1>()?;
//! let scoped = emitter.for_module(Self::MODULE_NAME);
//!
//! // In a handler:
//! let authorized = scoped
//!     .authorize(&ctx, resource_id, "resource_type".to_owned())
//!     .await?;
//! authorized
//!     .build_usage_record("requests", 1.0)
//!     .enqueue()
//!     .await?;
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod api;
mod authorized_emitter;
mod config;
mod domain;
mod emitter;
mod error;
mod infra;
mod scoped_emitter;
mod usage_builder;

pub use api::UsageEmitterV1;
pub use authorized_emitter::AuthorizedUsageEmitter;
pub use config::UsageEmitterConfig;
pub use emitter::UsageEmitter;
pub use error::UsageEmitterError;
pub use infra::delivery_handler::DeliveryHandler;
pub use scoped_emitter::ScopedUsageEmitter;
pub use usage_builder::UsageRecordBuilder;
