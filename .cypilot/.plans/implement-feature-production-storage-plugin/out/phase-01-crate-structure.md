# Phase 1 Output: Crate Scaffolding

## Files Created

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/lib.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/module.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs`

## Files Modified

- `Cargo.toml` (root) — added `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin` to workspace members

## Key Type Names

- `TimescaleDbConfig` — config struct with 5 parameters (database_url, pool_size_min, pool_size_max, retention_default, connection_timeout)
- `TimescaleDbStoragePlugin` — ModKit module struct (registered via `#[modkit::module(...)]`)
- `TimescaleDbPluginClient` — storage client struct holding a `sqlx::PgPool`
- `StoragePluginError` — error enum with all required variants

## Trait Implemented

- `UsageCollectorPluginClientV1` on `TimescaleDbPluginClient` (3 stub methods)

## GTS Schema Type

- `UsageCollectorStoragePluginSpecV1`

## Stub Method Count

- 3 methods stubbed with `todo!()`: `create_usage_record`, `query_aggregated`, `query_raw`

## StoragePluginError Variants

- `InvalidRecord(String)`
- `Transient(String)`
- `Configuration(String)`
- `Migration(String)`
- `ContinuousAggregateSetupFailed(String)`
- `QueryFailed(String)`
- `ConnectionPool(String)`

## Cargo Check Result

```
cargo check -p timescaledb-usage-collector-storage-plugin
Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.98s
```

Exit code: 0 (10 dead-code warnings expected for scaffold; no errors)

## Deviations from Noop Pattern

- No separate `service.rs` — `TimescaleDbPluginClient` struct lives directly in `domain/client.rs`
- Added `src/infra/mod.rs` placeholder (absent in noop plugin)
- Added `src/domain/error.rs` for plugin-specific error enum (absent in noop plugin)
- Uses `humantime` for Duration deserialization in config
- Uses `thiserror` for error type derivation
