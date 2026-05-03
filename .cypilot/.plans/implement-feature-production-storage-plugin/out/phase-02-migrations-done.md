# Phase 2 Output: Schema Migrations

## Function Signature

```rust
pub async fn run_migrations(pool: &PgPool) -> Result<(), MigrationError>
```

Where `MigrationError` is a type alias for `StoragePluginError` defined in `crate::domain::error`.

## CDSL Marker Pairs Placed

All 9 marker pairs placed in `src/infra/migrations.rs`:

1. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-1`
2. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-2`
3. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-3`
4. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-4`
5. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-5`
6. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-6`
7. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-7`
8. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8`
9. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-9`

Algo scope marker: `@cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1`

## Files Created

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`

## Files Modified

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs` — added `pub mod migrations;`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs` — added `pub type MigrationError = StoragePluginError;`

## FEATURE Checkboxes Updated

- `inst-mig-1` through `inst-mig-9` all marked `[x]`
- Parent algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations` NOT marked (awaits continuous aggregate phase)

## Acceptance Criteria

- [x] migrations.rs exists and is non-empty — PASS
- [x] run_migrations function present — PASS
- [x] All 9 inst-mig-* steps wrapped in @cpt-begin/@cpt-end pairs — PASS
- [x] Partial unique index includes WHERE idempotency_key IS NOT NULL — PASS
- [x] infra/mod.rs declares pub mod migrations — PASS
- [x] All 9 inst-mig-* FEATURE checkboxes marked [x] — PASS
- [x] out/phase-02-migrations-done.md exists with required content — PASS
- [x] No unresolved {...} variables outside code fences — PASS

## cargo check Result

Exit code 0 (only expected dead-code warnings for scaffold items; no errors).
