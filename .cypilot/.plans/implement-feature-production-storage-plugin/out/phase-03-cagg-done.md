# Phase 3 Output: Continuous Aggregate

## Files Created

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/continuous_aggregate.rs`

## Files Modified

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs` — added `pub mod continuous_aggregate;`
- `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` — marked inst-cagg-1 through inst-cagg-5 and parent algo ID as [x]

## Function Signature

```rust
pub async fn setup_continuous_aggregate(pool: &PgPool) -> Result<(), MigrationError>
```

## CDSL Marker Pairs Placed

Algo scope marker: `@cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1`

All 5 block marker pairs in `src/infra/continuous_aggregate.rs`:

1. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-1`
2. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-2`
3. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-3`
4. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-4`
5. `@cpt-begin/end:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1:inst-cagg-5`

## FEATURE Checkboxes Updated

- `inst-cagg-1` through `inst-cagg-5` all marked `[x]`
- Parent algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate` marked `[x]` (all 5 steps complete)

## Acceptance Criteria

- [x] continuous_aggregate.rs exists with setup_continuous_aggregate — PASS
- [x] WITH NO DATA present in view creation SQL — PASS
- [x] Refresh policy step present with if_not_exists => true idempotent guard — PASS
- [x] All 5 inst-cagg steps have individual @cpt-begin/@cpt-end pairs — PASS
- [x] Algo scope marker present above function signature — PASS
- [x] infra/mod.rs declares pub mod continuous_aggregate — PASS
- [x] FEATURE checkboxes inst-cagg-1 through inst-cagg-5 marked [x] — PASS
- [x] out/phase-03-cagg-done.md exists — PASS
- [x] No unresolved {...} variables outside code fences — PASS
