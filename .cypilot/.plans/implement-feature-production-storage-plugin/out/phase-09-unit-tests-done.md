# Phase 9 Unit Tests — Complete

## Files Created
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client_tests.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/insert_port.rs`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/pg_insert_port.rs`

## Files Modified
- `src/domain/metrics.rs` — added `record_ingestion_error()` to `PluginMetrics` trait and `NoopMetrics`
- `src/domain/mod.rs` — added `pub mod insert_port;`
- `src/domain/client.rs` — refactored `create_usage_record` to use `Arc<dyn InsertPort>`, added `record_ingestion_error` calls, added test module reference
- `src/infra/mod.rs` — added `pub mod pg_insert_port;`
- `src/module.rs` — updated constructor to pass `PgInsertPort`

## @cpt-dod marker
`// @cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability`
Present as first comment line in `client_tests.rs`.

## Test Results
`cargo test -p timescaledb-usage-collector-storage-plugin` — 20 tests pass (12 new + 8 existing scope tests), 0 failed.

## 12 New Test Functions

| # | Test Name | Status |
|---|-----------|--------|
| 1 | `test_create_usage_record_valid_counter` | PASS |
| 2 | `test_create_usage_record_valid_gauge` | PASS |
| 3 | `test_create_usage_record_negative_counter_value_rejected` | PASS |
| 4 | `test_create_usage_record_missing_idempotency_key_for_counter_rejected` | PASS |
| 5 | `test_create_usage_record_transient_db_error` | PASS |
| 6 | `test_create_usage_record_idempotent_insert` | PASS |
| 7 | `test_create_usage_record_counter_increments_ingestion_latency` | PASS |
| 8 | `test_create_usage_record_gauge_no_accumulation` | PASS |
| 9 | `test_scope_to_sql_single_group` | PASS |
| 10 | `test_scope_to_sql_multiple_groups_or_of_ands_preserved` | PASS |
| 11 | `test_scope_to_sql_empty_scope_fail_closed` | PASS |
| 12 | `test_scope_to_sql_ingroup_predicate_rejection` | PASS |

## Acceptance Criteria
- [x] `client_tests.rs` exists and is non-empty — PASS
- [x] All 8 `create_usage_record` test cases present — PASS
- [x] All 4 `scope_to_sql` test cases present — PASS
- [x] `test_scope_to_sql_ingroup_predicate_rejection` asserts `UnsupportedPredicate` — PASS
- [x] No test opens a real database connection — PASS (PgPool::connect_lazy, never executed)
- [x] `cargo test` passes — PASS (20/20)
- [x] `@cpt-dod` marker present as first comment — PASS
- [x] No unresolved variables — PASS
