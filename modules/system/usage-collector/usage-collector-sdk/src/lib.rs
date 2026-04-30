//! Usage Collector SDK
//!
//! Transport-agnostic contracts for the usage-collector module family.
//!
//! ## What this crate provides
//!
//! - [`UsageCollectorClientV1`] — ingest trait implemented by gateway/remote client modules
//!   (passed by constructor argument to the emitter, never via `ClientHub`).
//! - [`UsageCollectorPluginClientV1`] — storage-plugin trait implemented by backend plugins.
//! - [`UsageRecord`], [`UsageKind`], [`ModuleConfig`], [`AllowedMetric`] — transport-agnostic models.
//! - [`UsageCollectorError`] — error type shared by both traits.
//! - [`UsageCollectorStoragePluginSpecV1`] — GTS schema for storage plugin registration.
//!
//! ## Emitting usage
//!
//! Use the `usage-emitter` crate, which wraps [`UsageCollectorClientV1`] with PDP authorization
//! and outbox buffering.

// @cpt-dod:cpt-cf-usage-collector-dod-sdk-and-ingest-core-sdk-crate:p1

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod api;
pub mod error;
pub mod gts;
pub mod models;
pub mod plugin_api;

pub use api::UsageCollectorClientV1;
pub use error::UsageCollectorError;
pub use gts::UsageCollectorStoragePluginSpecV1;
pub use models::{
    AggregationFn, AggregationQuery, AggregationResult, AllowedMetric, BucketSize,
    GroupByDimension, ModuleConfig, RawQuery, UsageKind, UsageRecord,
};
// @cpt-begin:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-6
pub use modkit_odata::{CursorV1, Page, PageInfo};
// @cpt-end:cpt-cf-usage-collector-algo-query-api-sdk-types:p2:inst-sdk-6
pub use plugin_api::UsageCollectorPluginClientV1;
