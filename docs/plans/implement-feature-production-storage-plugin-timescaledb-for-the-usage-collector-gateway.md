# Implement Feature: Production Storage Plugin (TimescaleDB) for the usage-collector gateway

**Type**: implement | **Phases**: 11

**Scope**: Crate Scaffolding, Schema Migrations, Continuous Aggregate, Scope Translator, Ingest Write Path, Query Aggregated, Query Raw, GTS Registration and Health, Unit Tests, Integration Tests, Final Integration and Verification

## Validation Commands

No validation commands defined.

### Task 1: Crate Scaffolding

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-01-crate-scaffolding.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Create the `timescaledb-usage-collector-storage-plugin` crate skeleton. All source files must compile cleanly, but no business logic is implemented yet. The stub `UsageCollectorPluginClientV1` implementation returns `unimplemented!()` or `todo!()` for every method. Error types (`StoragePluginError`) are defined with all variants needed by later phases. The connection pool configuration struct (`TimescaleDbConfig`) declares all five parameters from the FEATURE spec. The workspace `Cargo.toml` is updated to include the new crate as a member.
- **Read AGENTS.md** — Read `.gen/AGENTS.md` to extract Rust crate conventions: module layout, naming patterns, `Cargo.toml` dependency style, error type conventions, and any workspace-level rules. Record the conventions that apply to this crate
- **Read noop plugin and SDK files** — Read all five noop plugin files and all three SDK files listed in `input_files` to understand:
- **Read FEATURE §1 and §5** — Read `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` sections §1.1, §1.2, and §5 "Plugin Crate" to confirm: crate name, required trait, GTS schema type name (`UsageCollectorStoragePluginSpecV1`), all five config parameters and their types/defaults, and the error variants needed across all phases
- **Read workspace Cargo.toml** — Read `Cargo.toml` at the project root to identify:
- **Create `timescaledb-usage-collector-storage-plugin/Cargo.toml`** — Write `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`. Mirror the noop plugin's `Cargo.toml` structure. Include:
- **Create `src/` stub files** — Write all eight source files:
- **Register crate in workspace** — Edit the root `Cargo.toml` to add `"modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin"` to the `[workspace]` `members` list, following the same ordering/formatting as existing plugin entries
- **Write handoff report** — Write `out/phase-01-crate-structure.md` listing:

**Success Checks:**
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml` exists with correct package name and all required dependencies
- All eight source files listed in `output_files` exist
- The new crate is added as a member in the root `Cargo.toml`
- `cargo check -p timescaledb-usage-collector-storage-plugin` exits with code 0 (no compile errors)
- `TimescaleDbConfig` declares all five config parameters matching the FEATURE §5 table (types and field names correct)
- `StoragePluginError` enum is defined with at least the variants: `InvalidRecord`, `Transient`, `Configuration`, `Migration`, `ContinuousAggregateSetupFailed`, `QueryFailed`, `ConnectionPool`
- `TimescaleDbPluginClient` implements `UsageCollectorPluginClientV1` with every method stubbed (`todo!()` or `unimplemented!()`)
- `out/phase-01-crate-structure.md` exists and lists all created files and key type names
- Zero unresolved `{...}` variables outside code fences in any created file

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/Cargo.toml`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/lib.rs`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/module.rs`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/config.rs`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/lib.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/plugin_api.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `Cargo.toml`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/lib.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/module.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs`
- Output file: `Cargo.toml`

**Expected Deliverables:**
- `out/phase-01-crate-structure.md`

### Task 2: Schema Migrations

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-02-schema-migrations.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement `src/infra/migrations.rs` in the `timescaledb-usage-collector-storage-plugin` crate. The file must contain a `run_migrations(pool: &PgPool) -> Result<(), MigrationError>` function that executes the 9-step idempotent schema migration sequence (`inst-mig-1` through `inst-mig-9`): enable the TimescaleDB extension, create the `usage_records` table, convert it to a hypertable, and create 5 composite indexes including the partial unique idempotency index. Each CDSL instruction must be wrapped in `@cpt-begin`/`@cpt-end` block markers. After implementing, update `infra/mod.rs` to expose `pub mod migrations`, and mark all 9 `inst-mig-*` checkboxes as `[x]` in the FEATURE file.
- **Read FEATURE spec sections.** Read `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`. Locate `### Operator: Schema Migration` (flow `inst-flow-smig-*` steps) and `### Schema Migrations` (algo `inst-mig-*` steps). Confirm the current checkbox states for all 9 `inst-mig-*` steps — they should all be `[ ]`
- **Read Phase 1 output.** Read `.cypilot/.plans/implement-feature-production-storage-plugin/out/phase-01-crate-structure.md`. Confirm the module layout, `MigrationError` type name, and any existing stubs documented there
- **Read existing source files.** Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs` and `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`. Note the exact `MigrationError` variant names (e.g. for DB/extension errors) and the current stub contents of `infra/mod.rs`
- **Implement `src/infra/migrations.rs`.** Create the file at `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs` with the following requirements:
- **Update `src/infra/mod.rs`.** Edit `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs` to add `pub mod migrations;` so the new module is publicly accessible from the crate
- **Mark FEATURE checkboxes.** In the FEATURE file `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`, update all 9 `inst-mig-*` steps under `### Schema Migrations` from `[ ]` to `[x]`. Do NOT mark the parent algo ID `[x]` yet — that requires all algo steps (including continuous aggregate) to be complete in a later phase. Do NOT mark any `inst-flow-smig-*` steps yet
- **Write phase output and self-verify.** Create `.cypilot/.plans/implement-feature-production-storage-plugin/out/phase-02-migrations-done.md` containing: the function signature of `run_migrations`, a list of all 9 `@cpt-begin`/`@cpt-end` marker identifiers placed, and a confirmation that `infra/mod.rs` was updated. Then verify all acceptance criteria below are met and report results in the Output Format

**Success Checks:**
- File `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs` exists and is non-empty
- Function `pub async fn run_migrations(pool: &PgPool) -> Result<(), MigrationError>` is present in `migrations.rs`
- All 9 `inst-mig-1` through `inst-mig-9` steps are each wrapped in a distinct `@cpt-begin`/`@cpt-end` pair using the correct CDSL ID and step label
- The partial unique index step (`inst-mig-8`) is present and the SQL includes a `WHERE idempotency_key IS NOT NULL` predicate
- `infra/mod.rs` declares `pub mod migrations;`
- All 9 `inst-mig-*` checkboxes in the FEATURE file are marked `[x]`; the parent algo ID checkbox is NOT marked `[x]` (not yet complete)
- `out/phase-02-migrations-done.md` exists and lists the function signature and all 9 marker pairs
- No unresolved `{...}` variables outside code fences in any modified file

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1
- Required prior artifact: `out/phase-01-crate-structure.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs`

**Expected Deliverables:**
- `out/phase-02-migrations-done.md`

### Task 3: Continuous Aggregate

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-03-continuous-aggregate.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement `src/infra/continuous_aggregate.rs` inside the `timescaledb-usage-collector-storage-plugin` crate. The file must contain a single public async function `setup_continuous_aggregate(pool: &PgPool) -> Result<(), MigrationError>` that executes the 5-step idempotent setup defined by `inst-cagg-1` through `inst-cagg-5`: create the `usage_agg_1h` materialized view over `usage_records` grouping by 1-hour time buckets (with `WITH NO DATA`), register the automated 30-minute refresh policy, trigger an initial manual refresh only when the view was newly created, verify the view and policy exist, and return `Ok(())`. Each step must be wrapped with a matching `@cpt-begin`/`@cpt-end` marker pair. After creating the file, update `infra/mod.rs` to expose `pub mod continuous_aggregate` and mark `inst-cagg-1` through `inst-cagg-5` as `[x]` in the FEATURE spec.
- **Read FEATURE spec continuous-aggregate section** — Read
- **Read Phase 2 output** — Read
- **Read existing `infra/mod.rs`** — Read
- **Create `src/infra/continuous_aggregate.rs`** — Create the file at
- **Update `infra/mod.rs`** — Add `pub mod continuous_aggregate;` to
- **Update FEATURE checkboxes and write handoff output** — In

**Success Checks:**
- `src/infra/continuous_aggregate.rs` exists and contains a `pub async fn setup_continuous_aggregate` function.
- The view creation SQL uses `WITH NO DATA` (deferred initial population).
- The refresh policy registration step is present and uses `if_not_exists => true` or equivalent idempotent guard.
- All 5 steps (`inst-cagg-1` through `inst-cagg-5`) have individual `@cpt-begin`/`@cpt-end` marker pairs; no single pair wraps the entire function body.
- The algo scope marker `@cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate:p1` is present directly above the function signature.
- `infra/mod.rs` declares `pub mod continuous_aggregate;`.
- FEATURE checkboxes for `inst-cagg-1` through `inst-cagg-5` are marked `[x]`.
- `out/phase-03-cagg-done.md` exists and lists the files created/modified.
- No unresolved `{...}` variables outside code fences in any file written by this phase.

**Guidance:**
- **TDD**: Write failing test first, implement minimal code to pass, then refactor.
- **SOLID**:
- Single Responsibility: Each module/function focused on one reason to change.
- Open/Closed: Extend behavior via composition/configuration, not editing unrelated logic.
- Liskov Substitution: Implementations honor interface contract and invariants.
- Interface Segregation: Prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: Depend on abstractions; inject dependencies for testability.
- **DRY**: Remove duplication by extracting shared logic with clear ownership.
- **KISS**: Prefer simplest correct solution matching design and project conventions.
- **YAGNI**: No specs/abstractions not required by current design scope.
- **Refactoring discipline**: Refactor only after tests pass; keep behavior unchanged.
- **Testability**: Structure code so core logic is testable without heavy integration.
- **Error handling**: Fail explicitly with clear errors; never silently ignore failures.
- **Observability**: Log meaningful events at integration boundaries (no secrets).

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 2
- Required prior artifact: `out/phase-02-migrations-done.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/continuous_aggregate.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/mod.rs`

**Expected Deliverables:**
- `out/phase-03-cagg-done.md`

### Task 4: Scope Translator

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-04-scope-translator.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement `src/domain/scope.rs` in the `timescaledb-usage-collector-storage-plugin` crate. The deliverable is a `scope_to_sql(scope: &AccessScope) -> Result<(String, Vec<SqlValue>), ScopeTranslationError>` function that executes the five-step translation algorithm (`inst-s2s-1` through `inst-s2s-5`): reject empty scope, iterate groups and predicates, build an OR-of-AND SQL WHERE fragment with positional bind parameters, and hard-error on `InGroup`/`InGroupSubtree` predicates. Update `domain/mod.rs` to expose `pub mod scope`. Mark the five FEATURE spec checkboxes for `inst-s2s-1` through `inst-s2s-5` as done and write `out/phase-04-scope-done.md` with the function signature and `SqlValue` type.
- Read `out/phase-01-crate-structure.md` (at `out/phase-01-crate-structure.md`) — confirm exact `ScopeTranslationError` variant names and any `SqlValue` type already defined in the crate
- Read the FEATURE spec section `### AccessScope → SQL Translator` in `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` (~lines 246–274) to confirm all five `inst-s2s-*` step identifiers and the algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`
- Read `modules/system/usage-collector/usage-collector-sdk/src/models.rs` — identify the `AccessScope`, `ConstraintGroup`, `Predicate`, and `SqlValue` type definitions (or confirm `SqlValue` is absent and must be defined locally)
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs` and `src/domain/error.rs` — confirm existing module declarations and `ScopeTranslationError` definition before writing
- Implement `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`:
- Update `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs` to add `pub mod scope;`
- Mark `inst-s2s-1` through `inst-s2s-5` (and all sub-steps `inst-s2s-3a`, `inst-s2s-3b`, `inst-s2s-3b-i` through `inst-s2s-3b-iv`, `inst-s2s-3c`) as `[x]` in the FEATURE spec. Write `out/phase-04-scope-done.md` containing: the full `scope_to_sql` function signature, the `SqlValue` type definition (or a note that it is re-exported from the SDK), and the confirmed `ScopeTranslationError` variant names used. Self-verify against all acceptance criteria

**Success Checks:**
- `src/domain/scope.rs` exists and is non-empty.
- `pub fn scope_to_sql(scope: &AccessScope) -> Result<(String, Vec<SqlValue>), ScopeTranslationError>` is present in `scope.rs`.
- Empty `scope.groups` returns `Err(ScopeTranslationError::EmptyScope)` — not `Ok(...)` with an empty string.
- `InGroup` or `InGroupSubtree` predicate returns `Err(ScopeTranslationError::UnsupportedPredicate { .. })` — not silently omitted or skipped.
- Implementation produces OR-of-ANDs structure: each `ConstraintGroup` produces a separate AND fragment; fragments are joined with ` OR `; no group flattening is present.
- All five `inst-s2s-*` steps (including sub-steps) have `@cpt-begin`/`@cpt-end` marker pairs in `scope.rs`.
- `domain/mod.rs` contains `pub mod scope;`.
- FEATURE spec checkboxes for `inst-s2s-1` through `inst-s2s-5` (and all sub-steps) are marked `[x]`.
- `out/phase-04-scope-done.md` exists and contains the function signature and `SqlValue` type information.
- No unresolved `{...}` variables outside code fences in any written file.

**Guidance:**
- MUST follow TDD: write failing test first, implement minimal code to pass, then refactor.
- MUST follow SOLID:
- Single Responsibility: each module/function focused on one reason to change.
- Open/Closed: extend behavior via composition/configuration, not editing unrelated logic.
- Liskov Substitution: implementations honor interface contract and invariants.
- Interface Segregation: prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: depend on abstractions; inject dependencies for testability.
- MUST follow DRY: remove duplication by extracting shared logic with clear ownership.
- MUST follow KISS: prefer simplest correct solution matching design and project conventions.
- MUST follow YAGNI: no specs/abstractions not required by current design scope.
- MUST follow refactoring discipline: refactor only after tests pass; keep behavior unchanged.
- MUST ensure testability: structure code so core logic is testable without heavy integration.
- MUST handle errors explicitly with clear errors; MUST NOT silently ignore failures.
- MUST log meaningful events at integration boundaries (no secrets).

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1
- Required prior artifact: `out/phase-01-crate-structure.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs`

**Expected Deliverables:**
- `out/phase-04-scope-done.md`

### Task 5: Ingest Write Path

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-05-ingest-write-path.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement `create_usage_record` in `src/domain/client.rs`, replacing the existing stub with the full 7-step idempotent ingest algorithm (`inst-cur-1` through `inst-cur-7`) from the FEATURE spec: validate `value >= 0` for counters, validate `idempotency_key` present for counters, execute an idempotent `INSERT ... ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING`, map unexpected constraint violations to `StorageError`, map transient DB errors to `Transient`, set `ingested_at` via `NOW()` in the SQL (not from the caller), and return `Ok(())`. Wire the actor flow `flow-storage-backend-ingest` (`inst-flow-ing-1` through `inst-flow-ing-5`). Add `@cpt-begin`/`@cpt-end` markers for every CDSL instruction, increment four Prometheus metrics, and mark all covered FEATURE checkboxes.
- **Read FEATURE spec sections** — Read the FEATURE file at
- **Read Phase 4 output** — Read
- **Read existing domain files** — Read the following three files to understand current state:
- **Implement `create_usage_record`** — Replace the stub in `client.rs` with the full implementation following `inst-cur-1` through `inst-cur-7`:
- **Add Prometheus metric increments** — Inside `create_usage_record`, add counter/histogram increments:
- **Mark FEATURE checkboxes** — In the FEATURE spec file, mark the following as `[x]`:
- **Write intermediate output and self-verify** — Write

**Success Checks:**
- `create_usage_record` in `client.rs` is fully implemented — contains no stub (`todo!()`, `unimplemented!()`, or placeholder `Ok(())` without logic).
- The INSERT statement includes `ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING`.
- `ingested_at` is set via `NOW()` (or equivalent DB-side expression) inside the SQL query — it is NOT populated from a Rust-side `chrono::Utc::now()` or caller-supplied value.
- All seven CDSL instructions (`inst-cur-1` through `inst-cur-7`) each have a distinct `@cpt-begin`/`@cpt-end` marker pair wrapping only the lines implementing that instruction.
- The actor flow scope marker for `cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest` is present at the function entry.
- All four Prometheus metrics (`usage_ingestion_total`, `usage_ingestion_latency_ms`, `usage_dedup_total`, `usage_schema_validation_errors_total`) are incremented at the correct code paths.
- FEATURE checkboxes for `inst-cur-1` through `inst-cur-7` and `inst-flow-ing-1` through `inst-flow-ing-5` are all marked `[x]`.
- Parent IDs `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record` and `cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest` are marked `[x]` in the FEATURE spec.
- `out/phase-05-ingest-done.md` exists and documents the function signature, ON CONFLICT target, `ingested_at` approach, marker list, metrics list, and checkbox updates.
- No unresolved `{...}` variables outside code fences in any modified file.

**Guidance:**
- MUST apply TDD: write failing test first, implement minimal code to pass, then refactor.
- MUST follow SOLID:
- Single Responsibility: each module/function focused on one reason to change.
- Open/Closed: extend behavior via composition/configuration, not editing unrelated logic.
- Liskov Substitution: implementations honor interface contract and invariants.
- Interface Segregation: prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: depend on abstractions; inject dependencies for testability.
- MUST apply DRY: remove duplication by extracting shared logic with clear ownership.
- MUST apply KISS: prefer simplest correct solution matching design and project conventions.
- MUST apply YAGNI: no specs/abstractions not required by current design scope.
- MUST follow refactoring discipline: refactor only after tests pass; keep behavior unchanged.
- MUST structure code so core logic is testable without heavy integration (Testability).
- MUST fail explicitly with clear errors; MUST NOT silently ignore failures (Error handling).
- MUST log meaningful events at integration boundaries; MUST NOT log secrets (Observability).
- MUST pass the code quality checklist.
- MUST keep functions/methods appropriately sized.
- MUST handle errors consistently.
- MUST cover implemented requirements with tests.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 4
- Required prior artifact: `out/phase-04-scope-done.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`

**Expected Deliverables:**
- `out/phase-05-ingest-done.md`

### Task 6: Query Aggregated

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-06-query-aggregated.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement the `query_aggregated` method on the `TimescaleDbPluginClient` struct in `src/domain/client.rs`. The implementation executes the 7-step algorithm defined as `inst-qagg-1` through `inst-qagg-7` in the FEATURE spec: (1) translate the `AccessScope` to SQL via `scope_to_sql`, failing closed with `AccessDenied` on any translation error; (2) make a routing decision — route to the `usage_records` raw hypertable when `resource_id` or `subject_id` appears in user filters or `group_by`, otherwise route to the `usage_agg_1h` continuous aggregate; (3/4) build and execute the correct SQL query for the chosen path; (5) handle transient DB errors; (6) map result rows to `Vec<AggregationResult>` with absent GROUP BY dimensions set to `None`; (7) return the result. On the continuous-aggregate path, `Avg` MUST be computed as `SUM(sum_val) / NULLIF(SUM(cnt_val), 0)` — it is not stored in the view. Add `@cpt-begin`/`@cpt-end` markers for each instruction, emit `usage_query_latency_ms` metric, log the routing decision at DEBUG level, and mark `inst-qagg-1` through `inst-qagg-7` as `[x]` in the FEATURE file.
- Read the FEATURE spec at `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`, section `### query_aggregated — Aggregation Query with Routing` (approximately lines 276–300). Confirm the 7 algorithm steps `inst-qagg-1` through `inst-qagg-7`, the routing decision rules, and the AVG composability note. Report the confirmed step count
- Read `modules/system/usage-collector/usage-collector-sdk/src/models.rs`. Extract the definitions of `AggregationQuery`, `AggregationResult`, `AggregationFn`, `GroupByDimension`, and `BucketSize`. Report the field names and variants found
- Read `out/phase-05-ingest-done.md`. Confirm that `create_usage_record` is marked complete and note any structural information about `client.rs` relevant to adding `query_aggregated`. Report the confirmed status
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs` and `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`. Identify the `scope_to_sql` function signature, the `SqlParam` type, the pool field on the client struct, existing imports, and any metric helpers already present. Report the findings
- Implement `query_aggregated` in `src/domain/client.rs`:
- Add observability:
- Mark `inst-qagg-1` through `inst-qagg-7` as `[x]` in the FEATURE file at `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`. Verify each checkbox update is consistent with the cascade rules (do not mark the parent algo ID `[x]` yet unless all other algo steps are also complete — check and report). Then write `out/phase-06-query-agg-done.md` with the following content:

**Success Checks:**
- `query_aggregated` is implemented in `src/domain/client.rs` and is non-empty (no TODO/stub/unimplemented!).
- Both routing branches are present: a raw hypertable path querying `usage_records` and a continuous aggregate path querying `usage_agg_1h`.
- The `Avg` function on the continuous-aggregate path uses `SUM(sum_val) / NULLIF(SUM(cnt_val), 0)` — not a stored average column.
- `scope_to_sql` is called before any query is built or executed; `AccessDenied` is returned when translation fails.
- All 7 algorithm steps (`inst-qagg-1` through `inst-qagg-7`) have paired `@cpt-begin`/`@cpt-end` markers wrapping their implementing code.
- `usage_query_latency_ms` metric is recorded with `query_type = "aggregated"`.
- `inst-qagg-1` through `inst-qagg-7` are marked `[x]` in the FEATURE file.
- `out/phase-06-query-agg-done.md` exists and lists the implemented items.
- No unresolved `{...}` variables outside code fences in any modified file.

**Guidance:**
- MUST follow TDD: write failing test first, implement minimal code to pass, then refactor.
- MUST apply SOLID principles:
- Single Responsibility: each module/function focused on one reason to change.
- Open/Closed: extend behavior via composition/configuration, not editing unrelated logic.
- Liskov Substitution: implementations honor interface contract and invariants.
- Interface Segregation: prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: depend on abstractions; inject dependencies for testability.
- MUST apply DRY: remove duplication by extracting shared logic with clear ownership.
- MUST apply KISS: prefer simplest correct solution matching design and project conventions.
- MUST apply YAGNI: no specs/abstractions not required by current design scope.
- MUST apply refactoring discipline: refactor only after tests pass; keep behavior unchanged.
- MUST ensure testability: structure code so core logic is testable without heavy integration.
- MUST handle errors explicitly with clear errors; MUST NOT silently ignore failures.
- MUST ensure observability: log meaningful events at integration boundaries (no secrets).

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 4, 5
- Required prior artifact: `out/phase-05-ingest-done.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`

**Expected Deliverables:**
- `out/phase-06-query-agg-done.md`

### Task 7: Query Raw

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-07-query-raw.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement the `query_raw` method on `TimescaleDbPluginClient` in `src/domain/client.rs`, executing the 8-step algorithm `cpt-cf-usage-collector-algo-production-storage-plugin-query-raw` (inst-qraw-1 through inst-qraw-8). The implementation must: translate the `AccessScope` via `scope_to_sql` (fail closed on `ScopeTranslationError`); decode an optional base64 cursor into `(DateTime<Utc>, Uuid)` returning `InvalidCursor` on failure; build a SELECT against `usage_records` with scope fragment, time range, optional user filters, and keyset advancement condition when a cursor is present using the tuple comparison `(timestamp > $cursor_ts) OR (timestamp = $cursor_ts AND id > $cursor_id)`; append `ORDER BY timestamp ASC, id ASC LIMIT $page_size`; execute the query; encode the next cursor as base64 when result count equals `page_size`; return `PagedResult<UsageRecord>`. Each step must carry `@cpt-begin`/`@cpt-end` markers, the method must emit the `usage_query_latency_ms` metric labeled `query_type="raw"`, and the eight FEATURE checkboxes must be marked `[x]`.
- **Read FEATURE spec — query_raw section**: Read `/Users/binarycode/code/virtuozzo/cyberfabric-core/modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` lines 303–328 (section `### query_raw — Cursor-Based Raw Record Pagination`). Confirm inst-qraw-1 through inst-qraw-8 match the Input section above. Note the exact CDSL ID: `cpt-cf-usage-collector-algo-production-storage-plugin-query-raw`
- **Read SDK models.rs for type definitions**: Read `/Users/binarycode/code/virtuozzo/cyberfabric-core/modules/system/usage-collector/usage-collector-sdk/src/models.rs`. Identify the exact definitions for `RawQuery`, `PagedResult`, `Cursor`, and `UsageRecord` (field names, types, optional fields). Note the `Cursor` type — whether it is a newtype over `String` or `Vec<u8>`
- **Read phase-06 output**: Read `out/phase-06-query-agg-done.md`. Confirm `query_aggregated` is implemented and note any patterns (metric emission, error mapping, sqlx usage) to reuse for `query_raw`
- **Read existing client.rs, scope.rs, and error.rs**: Read:
- **Implement `query_raw` in `client.rs`**: Add the `query_raw` method following the 8 steps exactly. Requirements:
- **Mark FEATURE checkboxes**: In `/Users/binarycode/code/virtuozzo/cyberfabric-core/modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`, mark inst-qraw-1 through inst-qraw-8 as `[x]`. Then mark the parent `p1` ID `cpt-cf-usage-collector-algo-production-storage-plugin-query-raw` as `[x]` if all 8 steps are checked
- **Write handoff and self-verify**: Write `out/phase-07-query-raw-done.md` confirming: `query_raw` implemented; all 3 query operations (scope_to_sql, query_aggregated, query_raw) are now implemented in client.rs. Then verify each acceptance criterion below and report pass/fail

**Success Checks:**
- `query_raw` method is present and fully implemented in `src/domain/client.rs` — no stubs, no `todo!()`, no `unimplemented!()`.
- Keyset cursor condition is present using tuple comparison: `(timestamp > $cursor_ts) OR (timestamp = $cursor_ts AND id > $cursor_id)`.
- Base64 encoding is used when building the next cursor from the last row's `(timestamp, id)`.
- Base64 decoding is used when parsing an incoming cursor; `UsageCollectorPluginError::InvalidCursor` is returned on any decode or parse failure.
- `LIMIT $page_size` is applied in the SQL query.
- All 8 inst-qraw steps (inst-qraw-1 through inst-qraw-8) have paired `@cpt-begin`/`@cpt-end` markers in `client.rs`.
- FEATURE checkboxes for inst-qraw-1 through inst-qraw-8 and the parent algo ID are all marked `[x]`.
- `usage_query_latency_ms` metric is emitted labeled `query_type="raw"`.
- `out/phase-07-query-raw-done.md` exists and confirms all 3 query operations are implemented.
- Zero unresolved `{...}` variables outside code fences in this phase file.

**Guidance:**
- MUST follow TDD: write failing test first, implement minimal code to pass, then refactor.
- MUST follow SOLID:
- Single Responsibility: each module/function focused on one reason to change.
- Open/Closed: extend behavior via composition/configuration, not editing unrelated logic.
- Liskov Substitution: implementations honor interface contract and invariants.
- Interface Segregation: prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: depend on abstractions; inject dependencies for testability.
- MUST follow DRY: remove duplication by extracting shared logic with clear ownership.
- MUST follow KISS: prefer simplest correct solution matching design and project conventions.
- MUST follow YAGNI: no specs/abstractions not required by current design scope.
- MUST follow refactoring discipline: refactor only after tests pass; keep behavior unchanged.
- MUST ensure testability: structure code so core logic is testable without heavy integration.
- MUST handle errors explicitly with clear errors; never silently ignore failures.
- MUST ensure observability: log meaningful events at integration boundaries (no secrets).
- MUST ensure code passes quality checklist.
- MUST ensure functions/methods are appropriately sized.
- MUST ensure error handling is consistent.
- MUST ensure tests cover implemented requirements.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 4, 6
- Required prior artifact: `out/phase-06-query-agg-done.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`

**Expected Deliverables:**
- `out/phase-07-query-raw-done.md`

### Task 8: GTS Registration and Health

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-08-gts-registration.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Finalize `src/module.rs` and `src/config.rs` for the `timescaledb-usage-collector-storage-plugin` crate. Implement the full ModKit module registration including: `TimescaleDbPluginConfig` struct with all five configuration parameters and validation; `on_start` that builds a `SecureConn`-managed `PgPool` with TLS enforcement (`sslmode=require`; plaintext connections rejected as hard startup errors), runs migrations, sets up the continuous aggregate, registers `UsageCollectorStoragePluginSpecV1` with the GTS registry, and registers the plugin with `ClientHub`; a health check function emitting the `storage_health_status` gauge (1 = healthy, 0 = unreachable); log INFO on success and ERROR on failure (no credentials in log output); all startup failures propagated as hard errors. Add `@cpt-dod` markers for `dod-plugin-crate` and `dod-encryption-and-gts` and mark those DoD checkboxes `[x]` in the FEATURE spec.
- **Read FEATURE spec §5 dod-plugin-crate and dod-encryption-and-gts**: Read
- **Read SDK gts.rs for GTS spec type**: Read
- **Read Phase 1 output for existing module structure**: Read
- **Read noop module.rs as pattern reference**: Read
- **Read existing stub module.rs and config.rs**: Read
- **Implement `src/config.rs`**: Write the final `TimescaleDbPluginConfig` struct with all five
- **Implement `src/module.rs`**: Replace the stub with the full implementation:
- **Mark DoD checkboxes and write phase output**:

**Success Checks:**
- `src/module.rs` is fully implemented — no stub bodies, no `todo!()`, no `unimplemented!()`
- TLS enforcement is present: `sslmode=require` validated in `TimescaleDbPluginConfig::validate` and `SecureConn` used for pool creation
- Plaintext connection (missing or wrong sslmode) is rejected as a hard startup error and NOT silently ignored
- `UsageCollectorStoragePluginSpecV1` GTS registration call is present in `on_start` and failure propagates as a hard error
- `storage_health_status` gauge is emitted by the health check function (value 1.0 healthy, 0.0 unreachable)
- `database_url` does not appear in any log format string, error message, or `Debug` output in either file
- All startup errors (config validation, pool creation, migrations, GTS registration) are propagated — no silent swallowing
- `@cpt-dod` scope markers and `@cpt-begin`/`@cpt-end` block markers are present for both `dod-plugin-crate` and `dod-encryption-and-gts` CDSL instructions
- `out/phase-08-gts-done.md` exists and records marker IDs and DoD checkbox states
- No unresolved `{...}` variables outside code fences

**Guidance:**
- [x] **TDD**: Write failing test first, implement minimal code to pass, then refactor
- [x] **SOLID**:
- Single Responsibility: Each module/function focused on one reason to change
- Open/Closed: Extend behavior via composition/configuration, not editing unrelated logic
- Liskov Substitution: Implementations honor interface contract and invariants
- Interface Segregation: Prefer small, purpose-driven interfaces over broad ones
- Dependency Inversion: Depend on abstractions; inject dependencies for testability
- [x] **DRY**: Remove duplication by extracting shared logic with clear ownership
- [x] **KISS**: Prefer simplest correct solution matching design and project conventions
- [x] **YAGNI**: No specs/abstractions not required by current design scope
- [x] **Refactoring discipline**: Refactor only after tests pass; keep behavior unchanged
- [x] **Testability**: Structure code so core logic is testable without heavy integration
- [x] **Error handling**: Fail explicitly with clear errors; never silently ignore failures
- [x] **Observability**: Log meaningful events at integration boundaries (no secrets)
- [x] Code passes quality checklist
- [x] Functions/methods are appropriately sized
- [x] Error handling is consistent
- [x] Tests cover implemented requirements

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1, 2, 3
- Required prior artifact: `out/phase-01-crate-structure.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/module.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/module.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/gts.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/module.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/config.rs`

**Expected Deliverables:**
- `out/phase-08-gts-done.md`

### Task 9: Unit Tests

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-09-unit-tests.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement the Level 1 unit test suite in `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client_tests.rs`. The test module uses `#[cfg(test)]` with `#[cfg_attr(coverage_nightly, coverage(off))]` and requires no real database connection — all DB interactions are covered via mock pool trait or test doubles. All 8 `create_usage_record` test cases and all 4 `scope_to_sql` test cases defined in FEATURE §6 test coverage guidance must be present, with metrics mock-verified in every success and failure path. A `@cpt-dod` scope marker for `dod-testing-and-observability` must be added, and the corresponding FEATURE DoD checkbox must be marked in progress.
- **Read FEATURE §6 Acceptance Criteria and Testing & Observability DoD.**
- **Read existing `client.rs` and `scope.rs` for function signatures and mock boundaries.**
- **Read noop `client_tests.rs` for structural reference.**
- **Implement `src/domain/client_tests.rs`.**
- **Run unit tests.**
- **Write intermediate result and self-verify.**

**Success Checks:**
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client_tests.rs` exists and is non-empty.
- All 8 `create_usage_record` test cases are present: `test_create_usage_record_valid_counter`, `test_create_usage_record_valid_gauge`, `test_create_usage_record_negative_counter_value_rejected`, `test_create_usage_record_missing_idempotency_key_for_counter_rejected`, `test_create_usage_record_transient_db_error`, `test_create_usage_record_idempotent_insert`, `test_create_usage_record_counter_increments_ingestion_latency`, `test_create_usage_record_gauge_no_accumulation`.
- All 4 `scope_to_sql` test cases are present: `test_scope_to_sql_single_group`, `test_scope_to_sql_multiple_groups_or_of_ands_preserved`, `test_scope_to_sql_empty_scope_fail_closed`, `test_scope_to_sql_ingroup_predicate_rejection`.
- `test_scope_to_sql_ingroup_predicate_rejection` explicitly asserts `ScopeTranslationError::UnsupportedPredicate` (not a generic or opaque error).
- No test in the file opens a real database connection, performs a network call, or writes to the filesystem.
- `cargo test -p timescaledb-usage-collector-storage-plugin` passes with all 12 new tests green (no `--features integration` required).
- The `@cpt-dod:cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability` scope marker is present as the first comment line in the file.
- No unresolved `{...}` variables appear outside code fences in this phase file.

**Guidance:**
- MUST follow TDD: write the test structure first, implement any missing mock wiring to make tests compile, then confirm all tests pass before moving on.
- MUST follow SOLID principles:
- Single Responsibility: each test function verifies exactly one behavior or error path.
- Open/Closed: extend test coverage via new test functions; do not modify production logic in this phase.
- Liskov Substitution: mock implementations honor the same interface contract as real implementations.
- Interface Segregation: prefer small, purpose-driven mock traits over broad ones.
- Dependency Inversion: inject mock DB pool and mock metric registry into the client under test.
- MUST follow DRY: extract repeated mock setup into helper functions with clear ownership.
- MUST follow KISS: use the simplest correct mock structure matching the existing noop test pattern.
- MUST follow YAGNI: do not add test utilities or abstractions not required by the 12 test cases in scope.
- MUST maintain refactoring discipline: refactor only after all tests pass; keep production behavior unchanged.
- MUST ensure testability: no test may require an active database connection, network call, or filesystem write.
- MUST handle errors explicitly: every error-path test MUST assert the exact error variant returned.
- MUST provide observability: metric mock MUST verify that counters and histograms are incremented with the correct labels in every success and failure path.
- MUST pass code quality checklist: functions/methods appropriately sized, error handling consistent, tests cover implemented requirements.
- MUST NOT leave TODO/FIXME/XXX/HACK in the test module.
- MUST NOT use `unimplemented!()` or `todo!()` in any test or mock.
- MUST NOT use bare `unwrap()` or `panic!()` in production code paths (test helpers may use `unwrap()` when failure is impossible by construction).
- MUST ensure new/changed behavior is covered by tests (TDD rule).
- MUST keep responsibilities separated and dependencies injectable (SOLID).
- MUST avoid copy-paste duplication (DRY).
- MUST avoid unnecessary complexity (KISS).
- MUST avoid speculative abstractions (YAGNI).

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 5, 6, 7

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client_tests.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client_tests.rs`

**Expected Deliverables:**
- `out/phase-09-unit-tests-done.md`

### Task 10: Integration Tests

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-10-integration-tests.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Implement Level 2 integration tests for the TimescaleDB storage plugin in `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs`. All tests are gated with `#[cfg(feature = "integration")]` and `#[tokio::test]`. Add an `integration = []` feature flag and testcontainers dev-dependencies to the crate's `Cargo.toml`. Tests spin up a `timescale/timescaledb:latest-pg16` container via the `testcontainers` crate, create a connection pool using the same `SecureConn` pattern (non-TLS for local container), run migrations, execute the five required test scenarios, and drop the container handle at teardown. Tests run via `cargo test --features integration`. The phase also annotates the tests file with the `@cpt-dod` marker for `dod-testing-and-observability` and marks that DoD checkbox `[x]` in the FEATURE document.
- **Read FEATURE §6 integration test requirements and dod-testing-and-observability.**
- **Read the existing `Cargo.toml` to identify the dev-dependencies section.**
- **Read `migrations.rs` and `client.rs` for function signatures.**
- **Update `Cargo.toml`: add `[features] integration = []` and dev-dependencies.**
- **Implement `tests/integration.rs` with all 5 test functions.**
- **Mark `dod-testing-and-observability` `[x]` in the FEATURE document.**
- **Write `out/phase-10-integration-tests-done.md` and self-verify.**

**Success Checks:**
- `tests/integration.rs` exists at the crate root under `tests/`.
- All five integration test functions are present:
- Every test function is annotated with `#[cfg(feature = "integration")]`.
- Every test function is annotated with `#[tokio::test]`.
- A container drop handle is held in each test function (container stops
- `[features]` block with `integration = []` is present in `Cargo.toml`.
- `testcontainers` dev-dependency is present in `Cargo.toml`.
- `// @cpt-dod:dod-testing-and-observability:p10` scope annotation is
- `dod-testing-and-observability` checkbox is marked `[x]` in the FEATURE
- No unresolved `{...}` variables outside code fences in any modified file.

**Guidance:**
- MUST follow TDD discipline: write failing tests, implement/verify minimal
- MUST apply SOLID principles:
- Single Responsibility: each test function tests exactly one behavior.
- Open/Closed: test helpers extend behavior via composition, not editing
- Liskov Substitution: implementations honor interface contracts and
- Interface Segregation: prefer small, purpose-driven test helpers.
- Dependency Inversion: depend on abstractions; inject dependencies for
- MUST apply DRY: extract shared container/pool setup into a helper; no
- MUST apply KISS: prefer simplest correct solution matching design and
- MUST apply YAGNI: no specs or abstractions not required by current design
- MUST follow refactoring discipline: refactor only after tests pass; keep
- MUST structure code so core logic is testable without heavy integration
- MUST handle errors explicitly with clear errors; MUST NOT silently ignore
- MUST log meaningful events at integration boundaries (no secrets).
- MUST apply observability: use structured logging or metrics at connection

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 8, 9

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml`

**Expected Deliverables:**
- `out/phase-10-integration-tests-done.md`

### Task 11: Final Integration and Verification

**Original Phase File:**
- `.plans/implement-feature-production-storage-plugin/phase-11-final-integration.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Perform final integration and verification for the production storage plugin feature. This phase reads all ten prior phase outputs to confirm full CDSL coverage, audits the FEATURE file checkboxes and performs the final cascade (marking all flow/algo/dod IDs as `[x]` where all nested steps are complete), validates the FEATURE artifact structure with `cpt --json validate --artifact`, updates `DECOMPOSITION.md` to mark the feature as IMPLEMENTED, runs `cargo check` on the new crate, and closes with a full expert panel review covering correctness of OR-of-ANDs preservation, TLS enforcement, idempotency under concurrency, cursor stability, and AVG composability. All deliverable phases are complete before this phase runs; this phase is the terminal verification and documentation closure pass.
- **Read all 10 phase outputs** to build a completion checklist. Read each of the following files in order and record which CDSL IDs and steps each phase confirmed as implemented:
- **Read the FEATURE spec in full** for the checkbox audit
- **Verify and mark individual CDSL instruction steps `[x]`**. For each CDSL instruction step still showing `[ ]` in the FEATURE: cross-reference the phase-output completion checklist (from Step 1) to confirm the corresponding `@cpt-begin`/`@cpt-end` markers exist. If evidence is present in the phase outputs, mark the step `[x]` in the FEATURE file. Do not mark any step `[x]` without marker evidence. Report any gaps (steps that remain `[ ]` due to missing evidence)
- **Promote all flow/algo/dod parent IDs to `[x]`** in the FEATURE. For each flow ID, algo ID, and dod ID: verify that ALL nested task-tracked items (step definitions and task-checkbox references within the heading scope) are `[x]`. If they are, mark the parent ID `[x]`. Apply the full cascade consistency rules from the Rules section. Report each ID promoted and any that could not be promoted with reason
- **Run FEATURE validation**:
- **Update DECOMPOSITION.md**
- **Run cargo check on the new crate**:
- **Run full expert panel review** (architectural change > 200 LOC — Full panel required: Developer, QA Engineer, Security Expert, Performance Engineer, DevOps Engineer, Architect, Monitoring Engineer, Database Architect/Data Engineer)
- **Self-verify against all acceptance criteria**. Check each criterion listed in the Acceptance Criteria section and report PASS or FAIL with evidence

**Success Checks:**
- All FEATURE flow IDs are marked `[x]` in the FEATURE file
- All FEATURE algo IDs are marked `[x]` in the FEATURE file
- All FEATURE dod IDs are marked `[x]` in the FEATURE file
- `cpt --json validate --artifact` exits with `status: PASS` and `error_count: 0` on the FEATURE file
- DECOMPOSITION.md entry for `cpt-cf-usage-collector-feature-production-storage-plugin` is marked `[x]` with status `IMPLEMENTED`
- `cargo check -p cf-timescaledb-usage-collector-storage-plugin` exits with code 0
- Full expert panel review completed (all 8 experts) with findings documented; any CRITICAL findings resolved or escalated before marking PASS
- No unresolved `{...}` variables outside code fences in any modified file

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 8, 9, 10
- Required prior artifact: `out/phase-01-crate-structure.md`
- Required prior artifact: `out/phase-02-migrations-done.md`
- Required prior artifact: `out/phase-03-cagg-done.md`
- Required prior artifact: `out/phase-04-scope-done.md`
- Required prior artifact: `out/phase-05-ingest-done.md`
- Required prior artifact: `out/phase-06-query-agg-done.md`
- Required prior artifact: `out/phase-07-query-raw-done.md`
- Required prior artifact: `out/phase-08-gts-done.md`
- Required prior artifact: `out/phase-09-unit-tests-done.md`
- Required prior artifact: `out/phase-10-integration-tests-done.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/docs/DECOMPOSITION.md`
- Output file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Output file: `modules/system/usage-collector/docs/DECOMPOSITION.md`
