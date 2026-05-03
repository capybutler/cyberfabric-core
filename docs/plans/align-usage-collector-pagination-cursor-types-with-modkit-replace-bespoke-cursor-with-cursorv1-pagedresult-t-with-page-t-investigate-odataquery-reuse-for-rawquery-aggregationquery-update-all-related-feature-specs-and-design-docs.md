# Align usage-collector pagination/cursor types with ModKit: replace bespoke Cursor with CursorV1, PagedResult<T> with Page<T>, investigate ODataQuery reuse for RawQuery/AggregationQuery, update all related feature specs and design docs

**Type**: implement | **Phases**: 9

**Scope**: Technical Analysis & Design, SDK Core Update, Gateway Update, Plugin Implementations, Unit Test Updates, Integration Test Updates, F3 Feature Spec Update, F4, DESIGN, and DECOMPOSITION Spec Updates, Build Verification

## Validation Commands

No validation commands defined.

### Task 1: Technical Analysis & Design

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-01-technical-analysis.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Perform a targeted technical analysis of the bespoke usage-collector pagination types versus their ModKit equivalents, and produce a design decision document at `out/phase-01-analysis.md`. The deliverable answers four design questions: how `Cursor` maps to `CursorV1`, how `PagedResult<T>` maps to `Page<T>`, whether `ODataQuery` is a viable replacement for `RawQuery`, and whether `ODataQuery` fits `AggregationQuery`. The analysis document is the sole input for all subsequent implementation phases; no code is written here.
- Read `modules/system/usage-collector/usage-collector-sdk/src/models.rs` (full, 311 lines)
- Read `libs/modkit-odata/src/page.rs` (full, 148 lines)
- Read `libs/modkit-odata/src/lib.rs` lines 109–540
- Read `libs/modkit-sdk/src/odata.rs` (full, 166 lines)
- Read `libs/modkit-sdk/src/pager.rs` lines 1–150
- Analyze: **Cursor → CursorV1 mapping**
- Analyze: **PagedResult<T> → Page<T> mapping**
- Analyze: **ODataQuery fit for RawQuery**
- Analyze: **ODataQuery fit for AggregationQuery**
- Write `out/phase-01-analysis.md` with the following five sections:
- Self-verify: confirm `out/phase-01-analysis.md` exists, is under 200 lines, all five

**Success Checks:**
- `out/phase-01-analysis.md` exists and is fewer than 200 lines
- Section (a) specifies exact `k`, `o`, `s`, `d` field values for a timestamp+id keyset cursor
- Section (b) shows the complete field-level mapping from `PagedResult<T>` to `Page<T>` + `PageInfo`
- Section (c) states a clear recommendation (replace / augment / skip) for `RawQuery` with rationale
- Section (d) states a clear recommendation (replace / augment / skip) for `AggregationQuery` with rationale
- Section (e) lists the `modkit-odata` types required and states whether a new Cargo dependency is needed
- No unresolved `{...}` variables outside code fences

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `libs/modkit-odata/src/page.rs`
- Input file: `libs/modkit-odata/src/lib.rs`
- Input file: `libs/modkit-sdk/src/odata.rs`
- Input file: `libs/modkit-sdk/src/pager.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`

**Expected Deliverables:**
- `out/phase-01-analysis.md`

### Task 2: SDK Core Update

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-02-sdk-core-update.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Replace the bespoke `Cursor`, `CursorDecodeError`, `PagedResult<T>`, and the `cursor_field` helper in `models.rs` with the modkit-odata equivalents (`CursorV1` and `Page<T>`). Update `RawQuery.cursor` to use `Option<modkit_odata::CursorV1>` (or apply the ODataQuery strategy if Phase 1 recommended replacing `RawQuery` entirely). Update `plugin_api.rs` to change `query_raw`'s return type from `Result<PagedResult<UsageRecord>, UsageCollectorError>` to `Result<modkit_odata::Page<UsageRecord>, UsageCollectorError>`. Update `lib.rs` to replace the re-exports of bespoke cursor/page types with `Page` and `PageInfo` from `modkit_odata`. Add the `modkit-odata` workspace dependency to `Cargo.toml`. Write `out/phase-02-sdk-interface.md` listing all changed public type signatures for downstream phases.
- Read `out/phase-01-analysis.md` and extract the following design decisions:
- Read all four SDK files:
- Update `models.rs` — remove bespoke cursor/page types:
- Update `models.rs` — apply cursor strategy from Phase 1 analysis:
- Update `plugin_api.rs`:
- Update `lib.rs`:
- Update `Cargo.toml`:
- Write `out/phase-02-sdk-interface.md` with the following sections:
- Self-verify against acceptance criteria:

**Success Checks:**
- `Cursor` struct, `CursorDecodeError` enum, and `cursor_field` helper are removed from `models.rs`.
- `PagedResult<T>` struct is removed from `models.rs`.
- `RawQuery.cursor` field type matches the CursorV1/ODataQuery strategy decided in Phase 1 (`out/phase-01-analysis.md`).
- `plugin_api.rs` `query_raw` return type is `Result<modkit_odata::Page<UsageRecord>, UsageCollectorError>`.
- `modkit-odata = { workspace = true }` is present in `Cargo.toml` `[dependencies]`.
- `lib.rs` no longer re-exports `Cursor`, `CursorDecodeError`, or `PagedResult`; re-exports `Page` and `PageInfo` from `modkit_odata`.
- `out/phase-02-sdk-interface.md` exists and lists all removed types, changed signatures, and new dependency.
- No unresolved `{...}` variables outside code fences in any modified file.

**Guidance:**
- MUST write failing test first (TDD), implement minimal code to pass, then refactor.
- MUST apply SOLID principles:
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
- MUST log meaningful events at integration boundaries with no secrets (Observability).
- MUST pass code quality checklist.
- MUST keep functions/methods appropriately sized.
- MUST maintain consistent error handling.
- MUST ensure tests cover implemented requirements.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1
- Required prior artifact: `out/phase-01-analysis.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/plugin_api.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/lib.rs`
- Input file: `modules/system/usage-collector/usage-collector-sdk/Cargo.toml`
- Output file: `modules/system/usage-collector/usage-collector-sdk/src/models.rs`
- Output file: `modules/system/usage-collector/usage-collector-sdk/src/plugin_api.rs`
- Output file: `modules/system/usage-collector/usage-collector-sdk/src/lib.rs`
- Output file: `modules/system/usage-collector/usage-collector-sdk/Cargo.toml`

**Expected Deliverables:**
- `out/phase-02-sdk-interface.md`

### Task 3: Gateway Update

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-03-gateway-update.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Update the gateway crate's DTO layer and HTTP handler code to use the modkit types `Page<T>`, `PageInfo`, and `CursorV1` instead of the bespoke `PagedResult<T>` and `Cursor` types. This includes replacing response DTO structs in `dto.rs`, updating cursor decode/encode logic in the raw query handler in `handlers.rs`, and removing any remaining bespoke cursor/paged-result references in `local_client.rs`. No behavior changes are introduced: the wire format evolves to match what the SDK now publishes, but query logic, filtering, and error handling remain unchanged.
- **Read Phase 1 analysis**: Read `out/phase-01-analysis.md` in full. Note the CursorV1 field mapping (`k`/`o`/`s`/`d`), the decision to use `modkit_odata::Page<T>` and `modkit_odata::PageInfo`, and any wire-format notes
- **Read Phase 2 SDK interface summary**: Read `out/phase-02-sdk-interface.md` in full. Confirm the exact new type signatures: `query_raw` returns `Page<UsageRecord>`, `CursorV1` is used for encode/decode, and `PagedResult`/`Cursor` are removed from the SDK
- **Read gateway DTOs**: Read `modules/system/usage-collector/usage-collector/src/api/rest/dto.rs` in full (243 lines). Identify any response structs that use `PagedResult<T>` or the bespoke `Cursor` type. Plan replacements: use `modkit_odata::Page<T>` directly or define local thin wrappers that delegate to `Page<T>` and `PageInfo`. Update `use` declarations and serde annotations as needed
- **Read gateway handlers**: Read `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs` in full (585 lines). Find the raw query handler function. Identify: (a) where it decodes an inbound cursor string into a `Cursor` struct (replace with `CursorV1::decode()`), (b) where it constructs the response `PagedResult { items, next_cursor }` (replace with `Page::new(items, PageInfo { next_cursor, prev_cursor: None, limit })`), and (c) where it encodes an outbound `next_cursor` string (replace with `CursorV1::encode()`). Update `use` imports accordingly
- **Read local client**: Read `modules/system/usage-collector/usage-collector/src/domain/local_client.rs` in full (440 lines). Identify any usages of `PagedResult<T>` or the bespoke `Cursor` type. Update those references to the modkit equivalents sourced from `modkit_odata` (or re-exported via the SDK)
- **Apply all changes**: Edit `dto.rs`, `handlers.rs`, and `local_client.rs` to implement the replacements identified in steps 3–5. Ensure:
- **Verify no stale references**: Search (grep) within the three modified files for the strings `PagedResult` and `Cursor` (bespoke, non-modkit). Confirm zero occurrences remain outside code comments. Report the grep results
- **Self-verify**: Check every acceptance criterion below. Report PASS or FAIL with reason for each

**Success Checks:**
- No `PagedResult` import or usage remains in `dto.rs`, `handlers.rs`, or `local_client.rs`.
- No bespoke `Cursor` struct import or usage remains in `dto.rs`, `handlers.rs`, or `local_client.rs`.
- `CursorV1::decode()` is used for inbound cursor string parsing in `handlers.rs`.
- `CursorV1::encode()` is used for outbound cursor string construction in `handlers.rs`.
- The raw query handler in `handlers.rs` returns `Page<UsageRecord>` (with `PageInfo`).
- All `use` declarations in modified files reference `modkit_odata` (or the SDK re-export) types, not removed local types.
- No behavior changes introduced: query logic, error handling, and filtering are unchanged.
- No unresolved `{...}` variables outside code fences.

**Guidance:**
- **TDD**: MUST write failing test first, implement minimal code to pass, then refactor.
- **SOLID**:
- Single Responsibility: Each module/function MUST be focused on one reason to change.
- Open/Closed: MUST extend behavior via composition/configuration, not editing unrelated logic.
- Liskov Substitution: Implementations MUST honor interface contract and invariants.
- Interface Segregation: MUST prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: MUST depend on abstractions; inject dependencies for testability.
- **DRY**: MUST remove duplication by extracting shared logic with clear ownership.
- **KISS**: MUST prefer simplest correct solution matching design and project conventions.
- **YAGNI**: MUST NOT add specs/abstractions not required by current design scope.
- **Refactoring discipline**: MUST refactor only after tests pass; keep behavior unchanged.
- **Testability**: MUST structure code so core logic is testable without heavy integration.
- **Error handling**: MUST fail explicitly with clear errors; MUST NOT silently ignore failures.
- **Observability**: MUST log meaningful events at integration boundaries (no secrets).
- Code MUST pass quality checklist.
- Functions/methods MUST be appropriately sized.
- Error handling MUST be consistent.
- Tests MUST cover implemented requirements.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 2
- Required prior artifact: `out/phase-01-analysis.md`
- Required prior artifact: `out/phase-02-sdk-interface.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/dto.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/domain/local_client.rs`
- Output file: `modules/system/usage-collector/usage-collector/src/api/rest/dto.rs`
- Output file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers.rs`
- Output file: `modules/system/usage-collector/usage-collector/src/domain/local_client.rs`

### Task 4: Plugin Implementations

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-04-plugin-implementations.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Update both storage plugin client implementations to use `Page<UsageRecord>` as the return type for `query_raw`, replacing the bespoke `PagedResult<T>` and `Cursor` types. In the noop plugin the change is a trivial stub update. In the timescaledb plugin, replace bespoke `Cursor` struct decode/encode with `CursorV1::decode()`/`CursorV1::encode()` from `modkit_odata`, and construct `Page::new(items, PageInfo { next_cursor, prev_cursor: None, limit })` as the return value. No changes to aggregation logic, query construction, row-mapping, or schema are required.
- Read `out/phase-01-analysis.md` in full. Extract the CursorV1 field mapping for usage records (timestamp → field, id → field) and any other design decisions relevant to the timescaledb `query_raw` update
- Read `out/phase-02-sdk-interface.md` in full. Confirm the exact import paths for `Page`, `PageInfo`, and `CursorV1` as established by Phase 2
- Read `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client.rs` in full
- Update the noop plugin `client.rs`:
- Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs` in full
- Update the timescaledb plugin `client.rs` imports:
- Update the `query_raw` function in the timescaledb plugin:
- EXECUTE: verify no bespoke `PagedResult` or `Cursor` (the old struct, not `CursorV1`) remain in either plugin file:
- Self-verify all acceptance criteria are met and report results in the required Output Format

**Success Checks:**
- `noop-usage-collector-storage-plugin/src/domain/client.rs`: `query_raw` return type is `Result<Page<UsageRecord>, UsageCollectorError>`.
- `noop-usage-collector-storage-plugin/src/domain/client.rs`: `query_raw` body returns `Page::new(vec![], PageInfo { next_cursor: None, prev_cursor: None, limit: query.page_size as u64 })`.
- `timescaledb-usage-collector-storage-plugin/src/domain/client.rs`: `query_raw` return type is `Result<Page<UsageRecord>, UsageCollectorError>`.
- `timescaledb-usage-collector-storage-plugin/src/domain/client.rs`: cursor is decoded via `CursorV1::decode()` and encoded via `CursorV1::encode()`.
- `timescaledb-usage-collector-storage-plugin/src/domain/client.rs`: `query_raw` returns `Page::new(records, PageInfo { next_cursor: ..., prev_cursor: None, limit: query.page_size as u64 })`.
- No `PagedResult` or bespoke `Cursor` (non-`CursorV1`) references remain in either plugin file (grep confirms zero matches).
- All existing `@cpt-begin`/`@cpt-end` markers on `query_raw` in both files are preserved.
- No unresolved `{...}` variables outside code fences in either modified file.

**Guidance:**
- **TDD**: Write failing test first, implement minimal code to pass, then refactor.
- **SOLID**:
- Single Responsibility: Each module/function MUST be focused on one reason to change.
- Open/Closed: Extend behavior via composition/configuration, NOT by editing unrelated logic.
- Liskov Substitution: Implementations MUST honor interface contract and invariants.
- Interface Segregation: Prefer small, purpose-driven interfaces over broad ones.
- Dependency Inversion: Depend on abstractions; inject dependencies for testability.
- **DRY**: Remove duplication by extracting shared logic with clear ownership.
- **KISS**: Prefer the simplest correct solution matching design and project conventions.
- **YAGNI**: No specs/abstractions NOT required by current design scope.
- **Refactoring discipline**: Refactor only after tests pass; keep behavior unchanged.
- **Testability**: Structure code so core logic is testable without heavy integration.
- **Error handling**: Fail explicitly with clear errors; MUST NOT silently ignore failures.
- **Observability**: Log meaningful events at integration boundaries (no secrets).
- Code MUST pass the quality checklist.
- Functions/methods MUST be appropriately sized.
- Error handling MUST be consistent.
- Tests MUST cover implemented requirements.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 3
- Required prior artifact: `out/phase-01-analysis.md`
- Required prior artifact: `out/phase-02-sdk-interface.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`
- Output file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`

### Task 5: Unit Test Updates

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-05-unit-test-updates.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Update the unit test files in three packages so they compile and pass against the new `CursorV1` and `Page<T>` types introduced by Phases 2-4. In `models_tests.rs`, replace all bespoke `Cursor` encode/decode tests with `CursorV1` round-trip equivalents and remove `CursorDecodeError` tests. In `noop-usage-collector-storage-plugin/src/domain/client_tests.rs`, update `PagedResult { items, next_cursor }` assertions to `Page { items, page_info }` shape. In `handlers_tests.rs`, search for all test cases that reference `PagedResult`, `Cursor`, `next_cursor`, or `page_size` and update only those cases to use the new response body shape; the remainder of the 1498-line file must be left unchanged.
- Read `out/phase-01-analysis.md` (full) to confirm the authoritative type mapping: old `Cursor` → `CursorV1`, old `PagedResult<T>` → `Page<T>`, `next_cursor` → `page_info.next_cursor` (or equivalent field as decided in analysis)
- Read `out/phase-02-sdk-interface.md` (full) to confirm the exact public API signatures and struct field names produced in Phase 2 (e.g., the `PageInfo` struct layout and `CursorV1` encode/decode contract)
- Read `modules/system/usage-collector/usage-collector-sdk/src/models_tests.rs` in full
- Edit `modules/system/usage-collector/usage-collector-sdk/src/models_tests.rs`:
- Read `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client_tests.rs` in full
- Edit `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client_tests.rs`:
- Read `modules/system/usage-collector/usage-collector/src/api/rest/handlers_tests.rs` in full
- Edit `modules/system/usage-collector/usage-collector/src/api/rest/handlers_tests.rs`:
- Self-verify against all acceptance criteria below. Confirm no `{variable}` placeholders remain outside code fences in the edited files

**Success Checks:**
- `models_tests.rs` contains no references to the bespoke `Cursor` type or `CursorDecodeError`.
- `models_tests.rs` contains at least one `CursorV1` encode/decode round-trip test that passes.
- `noop/client_tests.rs` assertions use `Page` / `PageInfo` shape; no `PagedResult` references remain.
- `handlers_tests.rs` raw query response assertions use `Page` / `PageInfo` shape; no `PagedResult` references remain in those test cases.
- `handlers_tests.rs` test cases outside the raw query scope are unmodified.
- All three files compile without errors against the updated SDK types from Phases 2-4.
- No TODO / FIXME / unimplemented! introduced in the edited test files.
- No unresolved `{variables}` outside code fences.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 4
- Required prior artifact: `out/phase-01-analysis.md`
- Required prior artifact: `out/phase-02-sdk-interface.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/usage-collector-sdk/src/models_tests.rs`
- Input file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers_tests.rs`
- Input file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client_tests.rs`
- Output file: `modules/system/usage-collector/usage-collector-sdk/src/models_tests.rs`
- Output file: `modules/system/usage-collector/usage-collector/src/api/rest/handlers_tests.rs`
- Output file: `modules/system/usage-collector/plugins/noop-usage-collector-storage-plugin/src/domain/client_tests.rs`

### Task 6: Integration Test Updates

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-06-integration-test-updates.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Update the TimescaleDB integration test file to replace all uses of the bespoke `PagedResult`/`Cursor` types with the ModKit equivalents (`Page<T>`/`CursorV1`) that were introduced by Phase 4. Test assertions that check `.next_cursor` as `Option<Cursor>` must be updated to check `.page_info.next_cursor` as `Option<String>`. Multi-page traversal tests that construct a `Cursor` for subsequent page requests must instead pass the opaque `String` cursor directly from `page_info.next_cursor`. After this phase, integration.rs must contain zero references to `PagedResult` or the bespoke `Cursor` struct.
- **Read design decisions**: Read `out/phase-01-analysis.md` (full). Note the CursorV1 mapping design and Page<T> field mapping decisions. These govern how assertions are written
- **Read SDK interface summary**: Read `out/phase-02-sdk-interface.md` (full). Confirm the new public signatures: `query_raw` returns `Page<UsageRecord>`, `RawQuery.cursor` is `Option<modkit_odata::CursorV1>`
- **Grep for affected lines**: Run the following command to collect all affected line numbers before touching the file:
- **Read targeted ranges**: For each matched line, read integration.rs in the range `[match_line - 20, match_line + 20]`. Do NOT read the full file. The known match clusters from planning are approximately:
- **Update multi-page traversal test (lines ~285-352)**:
- **Update single-page filter tests (lines ~1435-1620+)**:
- **Update imports**: Check the `use` block at the top of integration.rs (lines 1-29). If `Cursor` or `PagedResult` appear in the import list, remove them. If `CursorV1` is needed for the decode call in step 5, add `use modkit_odata::CursorV1;` (or the correct path per phase-02 output)
- **Verify no remaining bespoke types**: Run:
- **Self-verify against acceptance criteria**: Check each criterion below and confirm PASS or identify the failing location

**Success Checks:**
- No `PagedResult` identifier remains anywhere in integration.rs.
- No bespoke `Cursor` struct usage remains in integration.rs (neither import nor direct construction).
- Multi-page traversal test uses `page_info.next_cursor` on both pages: `first_page.page_info.next_cursor` for extraction and `second_page.page_info.next_cursor` for the is_none assertion.
- The cursor String from page 1 is correctly decoded into `CursorV1` before being passed as `RawQuery.cursor` in the page-2 request.
- Single-page filter tests compile unchanged (`.items` access is unaffected; no next_cursor assertions existed there).
- No unresolved `{...}` variables outside code fences.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 5
- Required prior artifact: `out/phase-01-analysis.md`
- Required prior artifact: `out/phase-02-sdk-interface.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/tests/integration.rs`

### Task 7: F3 Feature Spec Update

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-07-f3-spec-update.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Update the F3 feature spec (`0003-cpt-cf-usage-collector-feature-query-api.md`) to replace all references to the bespoke `Cursor` type with `CursorV1` from `modkit-odata`, and `PagedResult<T>` with `Page<T>` from `modkit-odata`. This includes updating CDSL instructions `inst-sdk-6` and `inst-sdk-7` (Cursor/RawQuery definitions), `inst-noop-2` (noop stub return value), `inst-raw-8` and `inst-raw-9` (plugin call signature and return type), the DoD entry for SDK types, and all acceptance criteria that reference bespoke types. The spec version is incremented (minor bump) with a changelog entry recording the type replacements.
- Read `out/phase-01-analysis.md` (intermediate output from Phase 1). Extract and note:
- Read `modules/system/usage-collector/docs/features/0003-cpt-cf-usage-collector-feature-query-api.md` (full file, ~530 lines). Identify every occurrence of the following strings and note the instruction slug and surrounding context for each:
- Prepare and present a summary of ALL proposed changes to the user before writing. Present each change as a before/after pair. At minimum, the following sections require changes:
- Upon user approval, apply all approved changes to `modules/system/usage-collector/docs/features/0003-cpt-cf-usage-collector-feature-query-api.md`. Edits MUST be minimal and surgical — change only the identified type names, struct field types, return type annotations, and directly dependent prose. Do not rewrite unchanged sections
- Bump the spec version. Increment the minor version number (e.g., 2.9 → 3.0) in the TOML frontmatter. Add a new changelog entry at the top of the `changelog` list:
- EXECUTE: `cpt --json toc /Users/binarycode/code/virtuozzo/cyberfabric-core/modules/system/usage-collector/docs/features/0003-cpt-cf-usage-collector-feature-query-api.md`
- Self-verify against the acceptance criteria:

**Success Checks:**
- No remaining `PagedResult` references in the F3 spec outside of code fences and historical changelog entries
- No remaining `CursorDecodeError` references in the F3 spec
- No remaining bespoke `Cursor struct` references (occurrences of `Cursor` as a standalone type name, not `CursorV1`) in the F3 spec body outside of historical changelog entries
- `inst-sdk-6` describes `CursorV1` from `modkit-odata` as the cursor type
- `inst-sdk-7` `RawQuery.cursor` field type is `Option<CursorV1>`
- `inst-sdk-9` `query_raw` return type is `Result<Page<UsageRecord>, UsageCollectorError>`
- `inst-noop-2` return expression reflects `Page::empty()` or equivalent modkit-correct constructor
- DoD entry `cpt-cf-usage-collector-dod-query-api-sdk-types` "Touches" list references `CursorV1` and `Page<T>` instead of `Cursor` and `PagedResult`
- Spec version incremented (minor bump) with changelog entry documenting type replacements
- TOC regenerated via `cpt toc` command
- User approval received before any changes were written to disk
- No unresolved `{...}` variables outside code fences in the updated spec

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 6
- Required prior artifact: `out/phase-01-analysis.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0003-cpt-cf-usage-collector-feature-query-api.md`
- Output file: `modules/system/usage-collector/docs/features/0003-cpt-cf-usage-collector-feature-query-api.md`

### Task 8: F4, DESIGN, and DECOMPOSITION Spec Updates

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-08-f4-design-spec-updates.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Update the F4 production storage plugin FEATURE spec, DESIGN.md, and DECOMPOSITION.md to remove all bespoke Cursor/PagedResult references and replace them with the canonical modkit-odata types: CursorV1 and Page<T>. In the F4 spec, update CDSL steps that reference PagedResult return types and cursor-related instructions. In DESIGN.md, update the domain model section to reflect CursorV1 and Page<T> with modkit-odata attribution. In DECOMPOSITION.md, update domain type entries only if they explicitly list bespoke Cursor/PagedResult as new domain types introduced by F3. Bump versions and add changelog entries for every modified spec file.
- Read `out/phase-01-analysis.md` — extract the list of misaligned references identified for F4 spec, DESIGN.md, and DECOMPOSITION.md. Note the exact file paths, affected section headings, and the canonical replacements recorded there
- EXECUTE: `grep -n "PagedResult\|CursorDecodeError\|Cursor::\|bespoke.*[Cc]ursor" modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Read the targeted sections of `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` identified in step 2
- Present a diff-style summary of the F4 spec changes to the user. Wait for explicit approval before writing
- EXECUTE: `grep -n "Cursor\|PagedResult\|domain.*type\|[Dd]ata [Mm]odel" modules/system/usage-collector/docs/DESIGN.md`
- Read the targeted sections of `modules/system/usage-collector/docs/DESIGN.md` identified in step 5
- Present a diff-style summary of the DESIGN.md changes to the user. Wait for explicit approval before writing
- EXECUTE: `grep -n "Cursor\|PagedResult\|Page<\|domain type" modules/system/usage-collector/docs/DECOMPOSITION.md`
- In DECOMPOSITION.md: update only if step 8 confirms that entries explicitly list bespoke `Cursor` or `PagedResult` as domain types added by F3 — replace those entries with `CursorV1` / `Page<T>` and add modkit-odata attribution. If no such entries exist, make no changes and record "no changes required" in the report
- Present a summary of proposed DECOMPOSITION.md changes (or confirm "no changes needed") to the user. Wait for explicit approval before writing
- EXECUTE: `cpt --json toc` for each file that was actually modified — regenerate the table of contents
- Write all approved files to disk
- Self-verify against acceptance criteria: confirm no remaining `PagedResult` or bespoke `Cursor` references in the modified files, confirm versions were bumped, confirm TOC was regenerated, and confirm no unresolved `{...}` variables appear outside code fences in the written files

**Success Checks:**
- F4 spec contains no remaining `PagedResult` or bespoke `Cursor` references; every such reference has been replaced with `Page<T>` or `CursorV1`
- F4 spec version was incremented and a changelog entry was added describing the type alignment
- DESIGN.md domain model section lists `CursorV1` and `Page<T>` with "(from modkit-odata)" attribution; no bespoke definitions remain
- DECOMPOSITION.md was updated if and only if it explicitly listed bespoke `Cursor` or `PagedResult` as domain types introduced by F3; no changes made otherwise
- TOC was regenerated in every file that was modified
- User approved each file change before it was written to disk
- No unresolved `{...}` variables appear outside code fences in this phase file or in the written output files
- Rule ARCH-FDESIGN-NO-001 is not violated: no new system-level type definitions were added to the F4 FEATURE spec

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 7
- Required prior artifact: `out/phase-01-analysis.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/docs/DESIGN.md`
- Input file: `modules/system/usage-collector/docs/DECOMPOSITION.md`
- Output file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Output file: `modules/system/usage-collector/docs/DESIGN.md`
- Output file: `modules/system/usage-collector/docs/DECOMPOSITION.md`

### Task 9: Build Verification

**Original Phase File:**
- `.plans/implement-feature-query-api-modkit-alignment/phase-09-build-verification.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Verify that the full usage-collector workspace compiles cleanly with all bespoke types replaced by their modkit equivalents. Run unit tests for each crate to confirm no regressions were introduced. Fix any remaining compile errors discovered during the build. This phase confirms the end-to-end build is green after the type-migration work completed in Phases 2–8.
- EXECUTE: `cargo build -p usage-collector-sdk 2>&1` — report any compilation errors; if errors exist, read the specific error locations and apply targeted fixes before continuing
- EXECUTE: `cargo build -p usage-collector 2>&1` — report any compilation errors; if errors exist, read the specific error locations and apply targeted fixes before continuing
- EXECUTE: `cargo build -p noop-usage-collector-storage-plugin 2>&1` — report any compilation errors; if errors exist, read the specific error locations and apply targeted fixes
- EXECUTE: `cargo build -p timescaledb-usage-collector-storage-plugin 2>&1` — report any compilation errors; if errors exist, read the specific error locations and apply targeted fixes
- EXECUTE: `cargo clippy -p usage-collector-sdk -p usage-collector -p noop-usage-collector-storage-plugin 2>&1` — report all warnings; flag any warnings that represent code quality issues introduced by the migration
- EXECUTE: `cargo test -p usage-collector-sdk 2>&1` — report test results (pass/fail counts, any failures)
- EXECUTE: `cargo test -p usage-collector 2>&1` — report test results (pass/fail counts, any failures)
- EXECUTE: `cargo test -p noop-usage-collector-storage-plugin 2>&1` — report test results (pass/fail counts, any failures)
- If any build errors or test failures were found and fixed in steps 1–8, re-run the affected build/test step to confirm the fix resolves the issue
- Produce the Code Quality Report (see Rules — Phase 4 format) summarising: build status for all four crates, lint status, unit test counts, and a note that integration tests for `timescaledb-usage-collector-storage-plugin` require a live TimescaleDB instance and were not executed in this phase. Then self-verify against all acceptance criteria

**Success Checks:**
- `cargo build -p usage-collector-sdk` exits with code 0 (no compilation errors).
- `cargo build -p usage-collector` exits with code 0 (no compilation errors).
- `cargo build -p noop-usage-collector-storage-plugin` exits with code 0 (no compilation errors).
- `cargo build -p timescaledb-usage-collector-storage-plugin` exits with code 0 (no compilation errors).
- `cargo test` passes for `usage-collector-sdk`, `usage-collector`, and `noop-usage-collector-storage-plugin`.
- No compile errors reference `PagedResult`, bespoke `Cursor`, or `CursorDecodeError`.
- No unresolved `{...}` variables outside code fences.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 8
