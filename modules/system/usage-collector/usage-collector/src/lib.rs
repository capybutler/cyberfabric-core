//! Usage Collector gateway.
//!
//! Centralized ingest for usage records from the SDK outbox pipeline and
//! delegation to the GTS-selected storage plugin.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod api;
mod config;
mod domain;
mod module;
