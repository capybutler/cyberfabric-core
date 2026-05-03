# Phase 10 — Integration Tests: Done

## Five integration test functions

1. `migration_idempotency` — Runs migrations twice, asserts second run succeeds and hypertable exists.
2. `concurrent_upsert_exactly_one_row` — Spawns 5 concurrent tasks with the same idempotency key, asserts exactly one row in DB.
3. `query_aggregated_routing_decision` — Inserts data, manually refreshes cagg, then asserts raw path returns data and cagg path also returns data.
4. `cursor_stability_under_concurrent_inserts` — Gets page 1 of 5 records, concurrently inserts outside-range records, asserts page 2 returns the remaining 2 original records without phantoms.
5. `health_check_metric` — Asserts `SELECT 1` succeeds on healthy pool and fails after `pool.close()`.

## Cargo.toml changes

- `[features]` block with `integration = []` added.
- `[dev-dependencies]` extended with `testcontainers = { workspace = true }`.

## Traceability marker

`// @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability:p10`
is the first comment line in `tests/integration.rs`.

## FEATURE checkbox

`dod-testing-and-observability` marked `[x]` in
`modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`.

## Acceptance criteria self-verification

- [x] `tests/integration.rs` exists at the crate root under `tests/`.
- [x] All five integration test functions are present.
- [x] Every test function is annotated with `#[cfg(feature = "integration")]`.
- [x] Every test function is annotated with `#[tokio::test]`.
- [x] Container drop handle held in each test function (`_container` field in `TestDb`).
- [x] `[features]` block with `integration = []` present in `Cargo.toml`.
- [x] `testcontainers` dev-dependency present in `Cargo.toml`.
- [x] `// @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability:p10` annotation present at top of file.
- [x] `dod-testing-and-observability` checkbox marked `[x]` in FEATURE document.
- [x] No unresolved `{...}` variables outside code fences.

## Validation

`cargo check -p timescaledb-usage-collector-storage-plugin --features integration` — PASS (0 errors, 0 warnings).
`cargo test -p timescaledb-usage-collector-storage-plugin` — PASS (20 unit tests, 0 integration tests run without feature flag).
