# Analyze correctness of TimescaleDB integration-test fixes and align FEATURE doc with fixed code

**Type**: generate | **Phases**: 2

**Scope**: Technical Analysis & Misalignment Catalog, Apply FEATURE Doc Updates

## Validation Commands

No validation commands defined.

### Task 1: Technical Analysis & Misalignment Catalog

**Original Phase File:**
- `.plans/analyze-update-feature-production-storage-plugin-timescaledb-fixes/phase-01-technical-analysis.md`

**Execution Prompt:**
- [x] Load the original phase file and use it as the authoritative source for this task.
- [x] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [x] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Assess the technical correctness of three code fixes applied to the TimescaleDB storage plugin and produce a precise catalog of every section in the FEATURE doc that is now stale relative to the fixed code. The catalog file written at the end of this phase is the sole input consumed by Phase 2 to apply FEATURE doc updates and add missing `@cpt` traceability markers.
- **Read source files**
- **Assess Fix A (migrations.rs)**
- **Assess Fix B (pg_insert_port.rs)**
- **Assess Fix C (continuous_aggregate.rs)**
- **Compare FEATURE doc sections against the fixed code**
- **Check @cpt traceability markers in migrations.rs**
- **Write the catalog file**
- **Self-verify against acceptance criteria**

**Success Checks:**
- Fix A has a CORRECT or INCORRECT verdict with a reason in the catalog
- Fix B has a CORRECT or INCORRECT verdict with a reason in the catalog
- Fix C has a CORRECT or INCORRECT verdict with a reason in the catalog
- Race safety has a verdict with reasoning in the catalog
- Every section listed in Task step 5 is present in the FEATURE Doc Change Catalog
- `out/phase-01-catalog.md` exists on disk when this phase completes
- No unresolved `{...}` variables outside code fences in the catalog file

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Declared Scope:**
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/pg_insert_port.rs`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/continuous_aggregate.rs`
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`

**Expected Deliverables:**
- `out/phase-01-catalog.md`

### Task 2: Apply FEATURE Doc Updates

**Original Phase File:**
- `.plans/analyze-update-feature-production-storage-plugin-timescaledb-fixes/phase-02-apply-feature-updates.md`

**Execution Prompt:**
- [ ] Load the original phase file and use it as the authoritative source for this task.
- [ ] Prioritize the phase frontmatter plus `What`, `Rules`, `Input`, `Task`, `Acceptance Criteria`, and `Output Format`.
- [ ] Treat `Preamble` as boilerplate and use `Prior Context` only as supporting background, not as new requirements.

**Phase Focus:**
- Apply every change from the Phase 1 misalignment catalog to the FEATURE document `0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`. Add `@cpt` traceability markers to `migrations.rs` for the new `inst-mig-8` step. Bump the FEATURE document version to 1.8 with a changelog entry dated 2026-05-03. Catalog items with verdict "no change needed" are skipped without modification. Validate the updated FEATURE document with the `cpt validate` command and confirm zero errors.
- **Read the Phase 1 catalog.** Read `.cypilot/.plans/analyze-update-feature-production-storage-plugin-timescaledb-fixes/out/phase-01-catalog.md` in full. Extract and hold in working context: (a) the complete list of catalog entries, (b) each entry's section ID, verdict ("CHANGE NEEDED" or "NO CHANGE NEEDED"), and the exact prose change required
- **Read the FEATURE doc.** Read `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` in full. Note the current frontmatter `version` field and the structure of the `changelog` array
- **Apply catalog changes.** For each catalog entry with verdict "CHANGE NEEDED", apply EXACTLY the described change to the FEATURE doc using targeted Edit operations. Enforce the following constraints on every edit:
- **Bump FEATURE doc version and add changelog entry.** In the FEATURE doc frontmatter:
- **Read migrations.rs.** Read `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs` in full. Locate the `sqlx::query(` call that creates the `usage_idempotency_keys` table (the `CREATE TABLE IF NOT EXISTS usage_idempotency_keys` statement)
- **Add @cpt traceability markers to migrations.rs.** Using a targeted Edit operation:
- **Verify both output files.** Re-read both `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md` and `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`. Confirm:
- **Run cpt validate.** Execute:
- **Self-verify against acceptance criteria.** Check every criterion in the Acceptance Criteria section and report pass/fail for each before generating the Output Format report

**Success Checks:**
- All catalog entries with verdict "CHANGE NEEDED" are applied to the FEATURE doc; none skipped.
- All catalog entries with verdict "NO CHANGE NEEDED" are left untouched in the FEATURE doc.
- FEATURE doc frontmatter `version` field equals `"1.8"`.
- A v1.8 changelog entry dated `2026-05-03` is present in the FEATURE doc frontmatter `changelog` array.
- No SQL DDL or code snippets are present in the FEATURE doc body (MAINT-FDESIGN-NO-001 respected).
- `@cpt-begin` and `@cpt-end` markers for `inst-mig-8` are present in `migrations.rs`, using the exact format `@cpt-{kind}:{cpt-id}:p{N}` (e.g., `@cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8`).
- `cpt validate --artifact ... --skip-code` exits with status PASS or WARN with zero errors.
- No unresolved `{...}` variables appear in either output file outside code fences.

**Ignore:**
- Other phases unless they are required by `depends_on` or explicitly referenced by the original phase file.
- Files outside this phase's declared scope unless the original phase explicitly tells you to read them at runtime.
- Any compiled-plan summary text if it conflicts with the original phase file.

**Dependencies:**
- Depends on phase(s): 1
- Required prior artifact: `out/phase-01-catalog.md`

**Declared Scope:**
- Input file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Input file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`
- Input file: `.cypilot/config/kits/sdlc/artifacts/FEATURE/rules.md`
- Output file: `modules/system/usage-collector/docs/features/0004-cpt-cf-usage-collector-feature-production-storage-plugin.md`
- Output file: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`
