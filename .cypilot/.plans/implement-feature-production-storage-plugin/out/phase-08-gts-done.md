# Phase 8 Output: GTS Registration and Health

## Files Modified

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs`
  - Removed `#[derive(Debug)]`; added custom `Debug` impl redacting `database_url`
  - Implemented `TimescaleDbConfig::validate()` enforcing: non-empty URL, sslmode=require,
    pool_size_min >= 1, pool_size_max >= pool_size_min, retention in [7d, 7y], timeout in (0s, 60s]

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/module.rs`
  - Replaced stub with full `init` implementation
  - Added `health_check` async function
  - Added `run_health_check_loop` background task (spawned from `init`)

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`
  - Added `opentelemetry = { workspace = true }`
  - Added `tokio = { workspace = true }`

- `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
  - Marked `dod-plugin-crate` as `[x]`
  - Marked `dod-encryption-and-gts` as `[x]`

## Marker IDs Added

Scope markers (at `impl Module` block):
- `@cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate:p1`
- `@cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts:p1`

Block markers in `init`:
- `@cpt-begin/end:...:inst-validate-config`
- `@cpt-begin/end:...:inst-build-secure-conn` (encryption-and-gts)
- `@cpt-begin/end:...:inst-build-pool`
- `@cpt-begin/end:...:inst-run-migrations`
- `@cpt-begin/end:...:inst-setup-continuous-aggregate`
- `@cpt-begin/end:...:inst-register-gts` (encryption-and-gts)
- `@cpt-begin/end:...:inst-register-client`

## DoD Checkbox States

- `cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate`: [x]
- `cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts`: [x]

## Summary

TLS enforcement is implemented via `sslmode=require` validation in `TimescaleDbConfig::validate()`
before pool creation; plaintext connections are rejected as a hard startup error.
GTS registration uses the same pattern as the noop plugin (`BaseModkitPluginV1`, `TypesRegistryClient`);
registration failure propagates as a hard error. The `database_url` never appears in any log output
(custom `Debug` impl redacts it; pool errors log only the error kind).
The `health_check` function emits the `storage_health_status` gauge (1.0 healthy, 0.0 unreachable)
via OpenTelemetry and is called every 30 seconds from a background Tokio task.

## cargo check Result

```
cargo check -p timescaledb-usage-collector-storage-plugin
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.80s
```

Exit code: 0 (one dead-code warning for unused error variants — expected for scaffold)
