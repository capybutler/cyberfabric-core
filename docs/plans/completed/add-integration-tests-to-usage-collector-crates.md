# Add integration tests to usage-collector crates

**Type**: implement | **Phases**: 3

**Scope**: Extend TimescaleDB Plugin Integration Tests, Integration Tests for usage-emitter Crate, Integration Tests for usage-collector Gateway Crate

## Validation Commands

No validation commands defined.

### Task 1: Extend TimescaleDB Plugin Integration Tests

**Original Phase File:**
- `.plans/implement-usage-collector-integration-tests/phase-01-tsdb-tests.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Extend the existing integration test file for the TimescaleDB usage-collector storage plugin with 36 new tests covering all aggregation functions on both the raw and continuous-aggregate query paths, all `GroupByDimension` variants, all filter combinations, scope isolation, `QueryResultTooLarge` error detection, raw-query filters, and `create_usage_record` validation errors. All new tests are appended after the 5 existing tests in the same `integration.rs` file. No other files are created or modified.
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs` (full file). Study all helpers (`TestDb`, `setup_container_and_pool`, `make_client`, `counter_record`) and the 5 existing tests. Note the existing imports
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs` (full file). Understand `query_aggregated` routing logic (raw path conditions: `resource_id.is_some() || subject_id.is_some() || group_by.contains(Resource) || group_by.contains(Subject)`), all filter bindings on both paths, and `query_raw` filter bindings
- Read `modules/system/usage-collector/usage-collector-sdk/src/models.rs` (full file). Confirm the exact variants for `AggregationFn` (Sum, Count, Min, Max, Avg), `BucketSize` (Minute, Hour, Day, Week, Month), `GroupByDimension` (TimeBucket(BucketSize), UsageType, Subject, Resource, Source), `AggregationQuery`, `RawQuery`, `UsageKind`, and `UsageRecord`
- Identify any missing imports needed for the new tests (e.g. `BucketSize`, `GroupByDimension` if not already imported). Prepare updated import lines to add at the top of the file if required
- Add any needed helper functions (`gauge_record()`, `record_with_subject()`) immediately before the new test blocks, using the same style as `counter_record()`
- Append all 36 new tests to `integration.rs` in the order: Group A, Group B, Group C, Group D, Group E, Group F, Group G, Group H, Group I. Each test MUST:
- For cagg path tests (Groups B, C cagg variants, E, and `group_by_time_bucket_cagg`): use `Utc::now() - chrono::Duration::hours(3)` for record timestamps and issue the refresh call:
- For Group G (`query_aggregated_result_too_large`): insert 3 records with distinct idempotency keys, then call `query_aggregated` with `max_rows: 2` and assert the result matches `Err(e) if matches!(e, UsageCollectorError::QueryResultTooLarge { .. })`
- For Group I validation error tests: construct the `UsageRecord` directly and call `client.create_usage_record(record).await`; assert error using `matches!(err, UsageCollectorError::Internal { .. })`
- Self-verify: confirm no `{...}` variables remain outside code fences, all 9 groups are represented (A through I), the file compiles structurally (correct Rust syntax), and the line count is within the 600-line target for this phase file

**Success Checks:**
- `tests/integration.rs` is the only file modified.
- All 36 new tests are present and appended after the 5 existing tests.
- All 9 groups (A through I) from the coverage matrix are represented with the correct test names.
- Every new test is guarded with `#[cfg(feature = "integration")]` and `#[tokio::test]`.
- Every new test creates its own isolated DB via `setup_container_and_pool().await`.
- Every new test uses a distinct `Uuid::new_v4()` for `tenant_id`.
- Raw-path tests set `resource_id: Some(...)` or `subject_id: Some(...)` in `AggregationQuery` to force raw routing.
- cagg-path tests do not set `resource_id` or `subject_id` in `AggregationQuery`, insert records 3+ hours ago, and call the manual refresh SQL before querying.
- `max_rows` is `100` on all `query_aggregated` calls except `query_aggregated_result_too_large` which uses `max_rows: 2`.
- `query_aggregated_result_too_large` asserts `Err(UsageCollectorError::QueryResultTooLarge { .. })`.
- Group I tests assert `Err(UsageCollectorError::Internal { .. })` using pattern matching.
- No unresolved `{...}` variables appear outside code fences in the phase file.
- Phase file line count is within the 1000-line budget.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs`

### Task 2: Integration Tests for usage-emitter Crate

**Original Phase File:**
- `.plans/implement-usage-collector-integration-tests/phase-02-emitter-tests.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Create four integration test files for the `cf-usage-emitter` crate under `modules/system/usage-collector/usage-emitter/tests/`. The tests promote existing in-module unit tests (which already use in-memory SQLite) to proper top-level integration tests following project conventions. This phase also adds any missing dev-dependencies to the crate's `Cargo.toml`. No source code other than `Cargo.toml` is modified.
- **Read source files** — Read all eight input files to extract the exact
- **Check and update Cargo.toml** — Inspect the `[dev-dependencies]` section
- **Create `tests/common/mod.rs`** — Write the shared helper module. Include:
- **Create `tests/delivery_handler_tests.rs`** — Write the six promoted tests
- **Create `tests/emitter_tests.rs`** — Write the promoted `UsageEmitter` tests
- **Create `tests/authorized_emitter_tests.rs`** — Write the promoted
- **Self-verify** — Check all acceptance criteria:

**Success Checks:**
- `tests/common/mod.rs` exists and exports all required helpers and mocks
- `tests/delivery_handler_tests.rs` exists with at least 6 tests covering
- `tests/emitter_tests.rs` exists with at least 8 tests covering the
- `tests/authorized_emitter_tests.rs` exists with at least 14 tests covering
- None of the four files use `#[cfg(test)]`, `#[path = "..."]`, or `use super::*`
- Every test function is annotated `#[tokio::test]`
- `#![allow(clippy::unwrap_used, clippy::expect_used)]` is present at the
- No file is gated behind a feature flag
- `Cargo.toml` dev-dependencies include `tokio` with `macros` and
- No unresolved `{...}` variables outside code fences in any output file

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-emitter/src/infra/delivery_handler.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/infra/delivery_handler_tests.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/emitter.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/emitter_tests.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/authorized_emitter.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/authorized_emitter_tests.rs`
- Input file: `modules/system/usage-collector/usage-emitter/src/api.rs`
- Input file: `modules/system/usage-collector/usage-emitter/Cargo.toml`
- Output file: `modules/system/usage-collector/usage-emitter/tests/common/mod.rs`
- Output file: `modules/system/usage-collector/usage-emitter/tests/delivery_handler_tests.rs`
- Output file: `modules/system/usage-collector/usage-emitter/tests/emitter_tests.rs`
- Output file: `modules/system/usage-collector/usage-emitter/tests/authorized_emitter_tests.rs`

### Task 3: Integration Tests for usage-collector Gateway Crate

**Original Phase File:**
- `.plans/implement-usage-collector-integration-tests/phase-03-gateway-tests.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Create an integration test suite in `modules/system/usage-collector/usage-collector/tests/` that exercises all four HTTP handlers (`handle_create_usage_record`, `handle_get_module_config`, `handle_query_aggregated`, `handle_query_raw`) end-to-end through axum routing using `tower::ServiceExt::oneshot` — no real server, no real database. The deliverable is five new files: `tests/common/mod.rs` (shared harness and mocks) plus four test-module files, one per handler group. All required Cargo dev-dependencies must also be confirmed or added.
- **Read source files.** Read the following files in full:
- **Update Cargo.toml dev-dependencies.** From the `Cargo.toml` read above, determine which dev-dependencies are missing or incomplete. The following MUST be present in `[dev-dependencies]`:
- **Write `tests/common/mod.rs`.** Create the shared test harness file at `modules/system/usage-collector/usage-collector/tests/common/mod.rs`. This file MUST:
- **Write `tests/create_record_tests.rs`.** Create `modules/system/usage-collector/usage-collector/tests/create_record_tests.rs` with all 5 required test functions. For `create_record_happy_path` and `create_record_emitter_authorization_failed`, use a real `UsageEmitter` backed by in-memory SQLite (same as `build_handler_emitter` in unit tests), since the emitter validation chain requires a real implementation. For the three validation tests (`create_record_metadata_too_large`, `create_record_subject_type_without_subject_id`, `create_record_invalid_subject_id_uuid`), the handler returns early before calling the emitter, so a simple mock emitter suffices
- **Write `tests/module_config_tests.rs`.** Create `modules/system/usage-collector/usage-collector/tests/module_config_tests.rs` with 2 required test functions. Use mock collector that returns a `ModuleConfig` for `module_config_found` and a `NotFoundCollector` for `module_config_not_found`
- **Write `tests/query_aggregated_tests.rs`.** Create `modules/system/usage-collector/usage-collector/tests/query_aggregated_tests.rs` with all 6 required test functions. Build time-range query strings using `chrono::Utc::now()` and RFC 3339 formatting for URL encoding. Construct query strings as URL-encoded parameters
- **Write `tests/query_raw_tests.rs`.** Create `modules/system/usage-collector/usage-collector/tests/query_raw_tests.rs` with all 8 required test functions. For `query_raw_cursor_expired`, encode the cursor using `cursor_encode` from `usage_collector::api::rest::dto` with `issued_at` set 25 hours in the past. For `query_raw_pagination_next_cursor`, use a mock plugin that returns items with a `next_cursor`
- **Self-verify.** Check all acceptance criteria:

**Success Checks:**
- `tests/common/mod.rs` exists and defines `AppHarness`, `MockUsageEmitterV1`, `MockUsageCollectorClientV1`, `MockUsageCollectorPluginClientV1`, `MockAuthZResolverClient`.
- `tests/create_record_tests.rs` exists and contains exactly the 5 required test functions: `create_record_happy_path`, `create_record_metadata_too_large`, `create_record_subject_type_without_subject_id`, `create_record_emitter_authorization_failed`, `create_record_invalid_subject_id_uuid`.
- `tests/module_config_tests.rs` exists and contains exactly the 2 required test functions: `module_config_found`, `module_config_not_found`.
- `tests/query_aggregated_tests.rs` exists and contains exactly the 6 required test functions: `query_aggregated_invalid_time_range`, `query_aggregated_time_range_too_wide`, `query_aggregated_missing_bucket_size`, `query_aggregated_forbidden`, `query_aggregated_result_too_large`, `query_aggregated_happy_path`.
- `tests/query_raw_tests.rs` exists and contains exactly the 8 required test functions: `query_raw_invalid_time_range`, `query_raw_time_range_too_wide`, `query_raw_invalid_page_size_zero`, `query_raw_page_size_exceeds_max`, `query_raw_cursor_expired`, `query_raw_forbidden`, `query_raw_happy_path`, `query_raw_pagination_next_cursor`.
- `Cargo.toml` `[dev-dependencies]` contains `tower = { workspace = true }`.
- Every test file starts with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
- No `use super::*` in any test file.
- All test functions use `#[tokio::test]` for async tests and `#[test]` for sync tests.
- `AppHarness` builds a router using direct axum routing (not `register_routes()`), with all required Extension layers including `SecurityContext`.
- No unresolved `{...}` variables outside code fences in this phase file.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/dto.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/routes.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers_tests.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/domain/local_client.rs`
- Input file: `modules/system/usage-collector/usage-collector/Cargo.toml`
- Output file: `modules/system/usage-collector/usage-collector/tests/common/mod.rs`
- Output file: `modules/system/usage-collector/usage-collector/tests/create_record_tests.rs`
- Output file: `modules/system/usage-collector/usage-collector/tests/module_config_tests.rs`
- Output file: `modules/system/usage-collector/usage-collector/tests/query_aggregated_tests.rs`
- Output file: `modules/system/usage-collector/usage-collector/tests/query_raw_tests.rs`
