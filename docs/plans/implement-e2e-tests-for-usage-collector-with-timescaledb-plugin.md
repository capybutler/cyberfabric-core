# Implement e2e tests for usage-collector with timescaledb plugin

**Type**: implement | **Phases**: 5

**Scope**: Server Assembly, E2E Test Infrastructure, Ingestion Tests, Raw Query Tests, Aggregated Query Tests

## Validation Commands

No validation commands defined.

### Task 1: Server Assembly

**Original Phase File:**
- `.plans/implement-feature-e2e-tests-timescaledb/phase-01-server-assembly.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Wire `usage-collector` and `timescaledb-usage-collector-storage-plugin` into the `hyperspot-server` binary as optional Cargo features, and register both crates in `registered_modules.rs` using the existing feature-gate pattern. The scope is limited to exactly two files: `apps/hyperspot-server/Cargo.toml` and `apps/hyperspot-server/src/registered_modules.rs`. No new source files are introduced.
- Read `apps/hyperspot-server/Cargo.toml` (whole file); note existing feature names, the path-dependency style used for optional crates, and the relative path format used to reference modules from that directory
- Read `apps/hyperspot-server/src/registered_modules.rs` (whole file); note the `#[cfg(feature = "...")] use ... as _;` pattern used for existing modules such as `mini-chat`
- Read `modules/system/usage-collector/usage-collector/Cargo.toml` (whole file); record the `[package] name` field — this is the crate name to reference in `hyperspot-server`
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml` (whole file); record the `[package] name` field and note any existing features (especially `integration`)
- Edit `apps/hyperspot-server/Cargo.toml`:
- Edit `apps/hyperspot-server/src/registered_modules.rs`:
- EXECUTE: `cargo check --package hyperspot-server --features usage-collector,timescaledb-usage-collector-storage-plugin`
- Self-verify against all acceptance criteria below and report results

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `apps/hyperspot-server/Cargo.toml`
- Input file: `apps/hyperspot-server/src/registered_modules.rs`
- Input file: `modules/system/usage-collector/usage-collector/Cargo.toml`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`
- Output file: `apps/hyperspot-server/Cargo.toml`
- Output file: `apps/hyperspot-server/src/registered_modules.rs`

### Task 2: E2E Test Infrastructure

**Original Phase File:**
- `.plans/implement-feature-e2e-tests-timescaledb/phase-02-e2e-infrastructure.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Create the Python e2e test module for `usage-collector` under `testing/e2e/modules/usage_collector/`. This phase delivers: a `TimescaleDB` Docker sidecar implementing `SidecarProtocol`, a two-server `conftest.py` supporting the federation topology (Instance 1 = emitter, Instance 2 = gateway with timescaledb plugin), a `config/base.yaml` template with placeholder markers, and a `helpers.py` module with `wait_for_record` and `encode_dt`. No test files are written in this phase; only the shared infrastructure that test phases 3–5 depend on.
- Read `testing/e2e/lib/orchestrator.py` (whole file). Extract: `ModuleTestEnv` dataclass fields (`binary`, `config_file`, `config_patch`, `log_suffix`, `sidecars`, `port`), `SidecarProtocol` interface (`name: str`, `port: int | None`, `start()`, `stop()`), `RunningTestEnv` fields, and the `test_env` fixture lifecycle pattern
- Read `testing/e2e/conftest.py` (whole file). Note global fixtures (`base_url`, `auth_headers`, `test_env`, `module_test_env`), environment variable naming conventions, and how fixtures are scoped
- Read `testing/e2e/modules/mini_chat/conftest.py` (whole file). Extract: the `config_patch` callable pattern (how it reads/modifies YAML), how sidecars are listed in `ModuleTestEnv`, port override convention, and `log_suffix` usage
- Read `modules/system/usage-collector/usage-collector/src/config.rs` (whole file). Identify the YAML config field names for: `vendor`, `plugin_timeout`, circuit breaker settings, and the `emitter` section — specifically the field that sets the remote collector URL (`collector_url` or equivalent) used in federation topology [B]
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs` (whole file). Identify the YAML field names for `database_url`, `pool_size_min`, `pool_size_max`, and `connection_timeout`
- Create `testing/e2e/modules/usage_collector/__init__.py` — may be empty
- Create `testing/e2e/modules/usage_collector/timescaledb_sidecar.py`. Implement `SidecarProtocol` for a TimescaleDB Docker container:
- Create `testing/e2e/modules/usage_collector/config/base.yaml`. Build a YAML config template for the hyperspot-server with usage-collector and timescaledb plugin. Include placeholder markers:
- Create `testing/e2e/modules/usage_collector/conftest.py`. Define:
- Create `testing/e2e/modules/usage_collector/helpers.py`. Implement:
- Self-verify against all acceptance criteria: confirm all 5 output files exist at declared paths, `timescaledb_sidecar.py` implements the full `SidecarProtocol`, `config/base.yaml` contains all three placeholder markers, `conftest.py` defines both `module_test_env` and `emitter_test_env`, `helpers.py` defines both `wait_for_record` and `encode_dt`, no port is hardcoded in Python sources, and no unresolved `{...}` variables appear outside code fences in this file

**Success Checks:**
- All 5 output files exist at their declared paths under `testing/e2e/modules/usage_collector/`.
- `timescaledb_sidecar.py` implements `SidecarProtocol`: has `name`, `port`, `start()`, and `stop()` members plus a `connection_string` property.
- `config/base.yaml` contains placeholder markers `__DB_URL__`, `__PORT__`, and `__COLLECTOR_URL__`.
- `conftest.py` defines both a `module_test_env` fixture (gateway, Instance 2) and an `emitter_test_env` fixture (emitter, Instance 1).
- `helpers.py` defines `wait_for_record` and `encode_dt` with the exact signatures specified in the Rules section.
- No port numbers are hardcoded in any Python source file created by this phase.
- No unresolved `{...}` variables appear outside code fences in this phase file.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1

**Declared Scope:**
- Input file: `testing/e2e/lib/orchestrator.py`
- Input file: `testing/e2e/conftest.py`
- Input file: `testing/e2e/modules/mini_chat/conftest.py`
- Input file: `modules/system/usage-collector/usage-collector/src/config.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs`
- Output file: `testing/e2e/modules/usage_collector/__init__.py`
- Output file: `testing/e2e/modules/usage_collector/timescaledb_sidecar.py`
- Output file: `testing/e2e/modules/usage_collector/conftest.py`
- Output file: `testing/e2e/modules/usage_collector/config/base.yaml`
- Output file: `testing/e2e/modules/usage_collector/helpers.py`

### Task 3: Ingestion Tests

**Original Phase File:**
- `.plans/implement-feature-e2e-tests-timescaledb/phase-03-ingestion-tests.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Write `testing/e2e/modules/usage_collector/test_ingest.py` containing four async pytest test functions that cover the full ingestion path: local ingest, local idempotency deduplication, remote federation ingest (emitter to gateway), and remote idempotency deduplication via the plugin. Each test verifies the complete async delivery path from the SQLite outbox through the TimescaleDB plugin by using `wait_for_record` to confirm records appear in the GET /raw endpoint on the gateway.
- Read `testing/e2e/modules/usage_collector/conftest.py` (whole file). Note:
- Read `testing/e2e/modules/usage_collector/helpers.py` (whole file). Note
- Read `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Read `modules/system/usage-collector/usage-collector-sdk/src/types.rs`
- Write `testing/e2e/modules/usage_collector/test_ingest.py` with exactly
- Self-verify against all acceptance criteria and report results

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 2

**Declared Scope:**
- Input file: `testing/e2e/modules/usage_collector/conftest.py`
- Input file: `testing/e2e/modules/usage_collector/helpers.py`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/types.rs`
- Output file: `testing/e2e/modules/usage_collector/test_ingest.py`

### Task 4: Raw Query Tests

**Original Phase File:**
- `.plans/implement-feature-e2e-tests-timescaledb/phase-04-raw-query-tests.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Write `testing/e2e/modules/usage_collector/test_query_raw.py` containing exactly five async pytest test functions that exercise the raw query endpoint (`GET /usage-collector/v1/raw`) against the timescaledb-backed gateway instance. The tests cover basic retrieval, time-range exclusion, cursor-based pagination, ascending sort order, and multi-metric retrieval. Validation-boundary cases already covered by unit tests are explicitly out of scope.
- Read `testing/e2e/modules/usage_collector/conftest.py`. Note the names of available fixtures — specifically `gateway_client`, any ingest fixture or helper, and session vs. function scoping. Record the `gateway_client` base-URL construction pattern
- Read `testing/e2e/modules/usage_collector/helpers.py`. Note the exact signatures of `wait_for_record` and `encode_dt`, including parameter names, types, and return values
- Read `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`. Confirm the exact query parameter names accepted by the GET /raw handler and the JSON field names in the response struct. Reconcile any discrepancy with the Input section above; the handlers source is authoritative for field names
- Read `modules/system/usage-collector/usage-collector/tests/query_raw_tests.rs`. Note which cases are already unit-tested (invalid time range, `page_size=0`, cursor format errors, etc.) to confirm they are absent from the e2e test file
- Write `testing/e2e/modules/usage_collector/test_query_raw.py` containing exactly the following five test functions, in order:
- Self-verify the written file against the acceptance criteria: confirm 5 test functions exist with correct names, all are `async def` with `@pytest.mark.asyncio`, pagination follows cursor and asserts second page, time-range exclusion checks by `resource_id`, no hardcoded timestamps/URLs/ports, and no unit-test validation duplication

**Success Checks:**
- `testing/e2e/modules/usage_collector/test_query_raw.py` exists at the declared path
- The file contains exactly 5 test functions: `test_raw_query_basic`, `test_raw_query_time_range_excludes_outside`, `test_raw_query_pagination_cursor`, `test_raw_query_ascending_order`, `test_raw_query_multiple_metrics`
- All 5 test functions are `async def` decorated with `@pytest.mark.asyncio`
- `test_raw_query_pagination_cursor` follows the cursor from `page_info.next_cursor` and asserts the second page is non-empty
- `test_raw_query_time_range_excludes_outside` asserts absence by `resource_id`, not merely that `items` is empty
- No hardcoded timestamps, URLs, or ports appear in the file
- No unit-test validation cases (invalid time range, `page_size=0`, cursor format errors) are re-tested
- All `from`/`to` values are produced via `encode_dt`
- All GET /raw calls target `gateway_client`, not `emitter_client`
- No unresolved `{...}` variables outside code fences in this phase file

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 2

**Declared Scope:**
- Input file: `testing/e2e/modules/usage_collector/conftest.py`
- Input file: `testing/e2e/modules/usage_collector/helpers.py`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Input file: `modules/system/usage-collector/usage-collector/tests/query_raw_tests.rs`
- Output file: `testing/e2e/modules/usage_collector/test_query_raw.py`

### Task 5: Aggregated Query Tests

**Original Phase File:**
- `.plans/implement-feature-e2e-tests-timescaledb/phase-05-aggregated-query-tests.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Write `testing/e2e/modules/usage_collector/test_query_aggregated.py` containing exactly five async pytest test functions that exercise the `GET /usage-collector/v1/aggregated` endpoint: sum, count, avg, group-by-resource, and time-range scenarios. All queries MUST include `resource_id` or `subject_id` as a query parameter so that routing goes through the raw hypertable path, making tests deterministic without depending on TimescaleDB's background continuous-aggregate refresh cycle. The cagg path is intentionally out of scope for e2e tests and is covered by the Rust integration tests.
- Read `testing/e2e/modules/usage_collector/conftest.py` (whole file). Note: fixture names (`gateway_client`, and any others), base URL derivation, and any `db_connection` or session-scoped fixtures
- Read `testing/e2e/modules/usage_collector/helpers.py` (whole file). Note: `wait_for_record` signature and parameters, `encode_dt` signature, and whether `wait_for_aggregated_record` exists
- Read `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs` (whole file). Confirm: the exact query parameter names accepted by `GET /usage-collector/v1/aggregated`, the JSON response shape (field names and types), and any routing or branching logic
- Read `modules/system/usage-collector/usage-collector/tests/query_aggregated_tests.rs` (whole file). Note: which aggregation functions, `group_by` combinations, and response-item field names are already exercised in unit tests, so the e2e tests avoid redundant duplication and instead confirm end-to-end behavior
- Write `testing/e2e/modules/usage_collector/test_query_aggregated.py` with exactly the following five test functions, using correct fixture names, helper signatures, and query parameter names confirmed in steps 1–4:
- Self-verify the written file against every acceptance criterion listed below and report pass/fail for each

**Success Checks:**
- `testing/e2e/modules/usage_collector/test_query_aggregated.py` exists at the declared path.
- The file contains exactly 5 test functions: `test_aggregated_sum`, `test_aggregated_count`, `test_aggregated_avg`, `test_aggregated_group_by_resource`, `test_aggregated_time_range`.
- All 5 test functions are `async def` and decorated with `@pytest.mark.asyncio`.
- Every test includes `resource_id` or `subject_id` in the aggregated query parameters (no cagg-path queries).
- Float aggregate assertions use approximate equality (`abs(actual - expected) < 0.001`); integer count assertion uses exact equality.
- Each test generates a unique `resource_id` per invocation (e.g., `uuid4().hex`-based).
- Each test calls `wait_for_record` after ingest and before the aggregated query.
- No hardcoded timestamps, URLs, or port numbers outside fixtures/helpers.
- No direct database queries.
- No `time.sleep` calls.
- No unresolved `{...}` variables outside code fences in this phase file.
- Phase file line count is within the 600-line budget.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 2

**Declared Scope:**
- Input file: `testing/e2e/modules/usage_collector/conftest.py`
- Input file: `testing/e2e/modules/usage_collector/helpers.py`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Input file: `modules/system/usage-collector/usage-collector/tests/query_aggregated_tests.rs`
- Output file: `testing/e2e/modules/usage_collector/test_query_aggregated.py`
