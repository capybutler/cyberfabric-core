# Phase 1 Catalog — Technical Analysis & Misalignment

## Correctness Verdicts

### Fix A — migrations.rs: PRIMARY KEY + idempotency table

Verdict: CORRECT

Reason: `PRIMARY KEY (id, timestamp)` includes the partition column `timestamp` as required by
TimescaleDB; `usage_idempotency_keys (tenant_id, idempotency_key, PRIMARY KEY (tenant_id,
idempotency_key))` exists with the correct columns; `idx_usage_records_tenant_idempotency`
partial unique index is absent — its removal is correct because a partial index omitting the
partition column is rejected by TimescaleDB.

### Fix B — pg_insert_port.rs: two-step transaction for idempotent inserts

Verdict: CORRECT

Reason: The idempotency-key branch follows the required sequence — begin transaction, INSERT into
`usage_idempotency_keys` ON CONFLICT DO NOTHING, check `rows_affected`, rollback and return
`Ok(0)` if 0 rows claimed, INSERT into `usage_records`, commit; the gauge / no-key branch is an
unconditional direct INSERT without a transaction, unchanged from the prior design.

### Fix C — continuous_aggregate.rs: start_offset adjusted

Verdict: CORRECT

Reason: `start_offset` = 3 hours, `end_offset` = 1 hour; refresh window = 3h − 1h = 2h, which is
strictly greater than the 1-hour bucket size, satisfying the TimescaleDB refresh policy rule.

---

## Insertion Logic Race Safety

Verdict: RACE-SAFE

The `usage_idempotency_keys` table carries `PRIMARY KEY (tenant_id, idempotency_key)`. Under
PostgreSQL's READ COMMITTED isolation, concurrent `INSERT … ON CONFLICT DO NOTHING` statements
on the same primary key are serialised at the database level: the first writer acquires the tuple
slot; all concurrent writers receive `rows_affected = 0` without error and immediately roll back,
returning `Ok(0)` to the caller. The primary key constraint prevents phantom duplicates regardless
of concurrency level.

Transaction atomicity (cited rule: Transaction atomicity rule): the idempotency-key INSERT and the
`usage_records` INSERT share one transaction. If the second INSERT fails (e.g., transient DB
error), the entire transaction rolls back, including the idempotency-key row. A subsequent retry
will not find the key pre-claimed, so the operation is safe to retry without leaking a phantom key.

Edge case — panic between commit of idempotency-key INSERT and start of usage_records INSERT: this
scenario cannot occur in the current code because both operations execute inside a single `sqlx`
transaction (`tx`). There is no point between the two inserts where the transaction could be
partially committed; a process crash before `tx.commit()` causes an automatic rollback by the
server when the connection closes.

No race condition was found.

---

## FEATURE Doc Change Catalog

### inst-mig-2
Current text: "Create the `usage_records` table with the following columns: `id` (UUID primary
key, auto-generated), `tenant_id` (UUID, required)..."
Required change: Replace "`id` (UUID primary key, auto-generated)" with "`id` (UUID, required,
auto-generated via `gen_random_uuid()`; part of composite PRIMARY KEY `(id, timestamp)` required
by TimescaleDB hypertable partitioning)". The composite key constraint must be visible in the
column list so it is not confused with a simple single-column primary key.

### inst-mig-8
Current text: "Create a partial unique index on `(tenant_id, idempotency_key)` named
`idx_usage_records_tenant_idempotency`, covering only rows where `idempotency_key` is non-null;
this is the upsert target for idempotent record creation; idempotent"
Required change: Replace the entire step with a description of the `usage_idempotency_keys` table
creation: "Create a plain table `usage_idempotency_keys` with columns `tenant_id` (UUID, required)
and `idempotency_key` (text, required), with `PRIMARY KEY (tenant_id, idempotency_key)`; this
table is the cross-partition deduplication store for idempotent counter records; a separate plain
table is required because TimescaleDB rejects unique indexes that omit the partition column on a
hypertable; idempotent (`CREATE TABLE IF NOT EXISTS`)".

### §3 Continuous Aggregate algo — Input block
Current text: "`continuous_aggregate_refresh_interval` operational parameter (default: 30-minute
schedule, 2-hour start offset, 1-hour end offset)"
Required change: Replace "2-hour start offset" with "3-hour start offset". The refresh window
is 3h − 1h = 2h, which must exceed the 1-hour bucket size; the 2-hour value was rejected by
TimescaleDB.

### inst-cagg-2
Current text: "Register an automated refresh policy for `usage_agg_1h`: schedule interval 30
minutes, start offset 2 hours, end offset 1 hour; if a policy already exists, skip"
Required change: Replace "start offset 2 hours" with "start offset 3 hours".

### inst-cur-3
Current text: "Execute INSERT INTO `usage_records` with all record fields; ON CONFLICT on
`(tenant_id, idempotency_key)` WHERE `idempotency_key IS NOT NULL` DO NOTHING — idempotent
upsert: duplicate records (same `tenant_id` + `idempotency_key`) are silently ignored, not
double-counted"
Required change: Replace the single-INSERT-with-ON-CONFLICT description with a two-step
transaction description: "Open a transaction; INSERT into `usage_idempotency_keys (tenant_id,
idempotency_key)` ON CONFLICT DO NOTHING; if 0 rows were claimed (duplicate key), rollback and
return immediately (record already stored); otherwise INSERT the record into `usage_records`
with all fields and commit the transaction. This two-step approach is required because
TimescaleDB rejects unique indexes on `usage_records` that omit the partition column."

### §3 create_usage_record algo — Constraints block
Current text: "`cpt-cf-usage-collector-nfr-throughput` (≥ 10,000 records/sec sustained;
single-row INSERT with partial unique index upsert is the hot path; connection pool size and
hypertable chunk cache govern throughput ceiling)"
Required change: Replace "single-row INSERT with partial unique index upsert is the hot path"
with "single-row INSERT is the hot path for gauge records; counter records use a two-step
transaction (idempotency-key INSERT + usage_records INSERT) which adds one extra round-trip per
counter record; connection pool size and hypertable chunk cache govern throughput ceiling".

### inst-flow-ing-3
Current text: "Plugin executes idempotent INSERT: `cpt-cf-usage-collector-algo-production-
storage-plugin-create-usage-record` — ON CONFLICT `(tenant_id, idempotency_key)` DO NOTHING"
Required change: Remove the trailing "— ON CONFLICT `(tenant_id, idempotency_key)` DO NOTHING"
clause and replace with "— deduplication via a two-step transaction against the
`usage_idempotency_keys` table (ON CONFLICT DO NOTHING on the key row, then INSERT into
`usage_records` if the key was newly claimed)".

### dod-schema-migrations
Current text: "...create the `usage_records` hypertable with all required columns and five
composite indexes including the partial unique idempotency index on `(tenant_id,
idempotency_key)`; and create the `usage_agg_1h` continuous aggregate with a 30-minute scheduled
refresh policy and a 2-hour start / 1-hour end offset."
Required change: Replace "five composite indexes including the partial unique idempotency index
on `(tenant_id, idempotency_key)`" with "four composite indexes and a separate
`usage_idempotency_keys` plain table for cross-partition idempotency deduplication"; replace "a
2-hour start / 1-hour end offset" with "a 3-hour start / 1-hour end offset".

### dod-ingest-ops
Current text: "`create_usage_record` persisting a single record using idempotency-index conflict
resolution (DO NOTHING on duplicate `(tenant_id, idempotency_key)`);"
Required change: Replace "idempotency-index conflict resolution (DO NOTHING on duplicate
`(tenant_id, idempotency_key)`)" with "a two-step transaction: INSERT into
`usage_idempotency_keys` ON CONFLICT DO NOTHING, early return on 0 rows claimed, then INSERT
into `usage_records`".

---

## Code Change: @cpt Markers

File: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/infra/migrations.rs`

The `usage_idempotency_keys` DDL block (currently lines 41–53) has no `@cpt-begin` / `@cpt-end`
markers. Phase 2 must wrap this block with:

```
// @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8
```
...insert before line 41 (the `// Separate plain table for idempotency deduplication.` comment)

```
// @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8
```
...insert after line 53 (the `.map_err(...)` result unwrap `?;` line that ends the `usage_idempotency_keys` sqlx block)

Exact marker IDs:
- begin: `@cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8`
- end:   `@cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations:p1:inst-mig-8`

Line range in current file: lines 41–53 (comment + sqlx::query block through the terminal `?;`).

---

## Self-Verification Against Acceptance Criteria

- [x] Fix A has a CORRECT or INCORRECT verdict with a reason in the catalog — PASS
- [x] Fix B has a CORRECT or INCORRECT verdict with a reason in the catalog — PASS
- [x] Fix C has a CORRECT or INCORRECT verdict with a reason in the catalog — PASS
- [x] Race safety has a verdict with reasoning in the catalog — PASS
- [x] Every section listed in Task step 5 is present in the FEATURE Doc Change Catalog (9 sections: inst-mig-2, inst-mig-8, §3 Continuous Aggregate Input block, inst-cagg-2, inst-cur-3, §3 create_usage_record Constraints block, inst-flow-ing-3, dod-schema-migrations, dod-ingest-ops) — PASS
- [x] out/phase-01-catalog.md exists on disk when this phase completes — PASS
- [x] No unresolved {...} variables outside code fences in the catalog file — PASS
