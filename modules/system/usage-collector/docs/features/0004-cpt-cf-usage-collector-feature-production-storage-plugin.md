---
cpt:
  version: "1.8"
  changelog:
    - version: "1.8"
      date: "2026-05-03"
      changes:
        - "Align FEATURE doc with TimescaleDB integration fixes: replace inst-mig-8 partial-unique-index description with usage_idempotency_keys plain-table description; update inst-mig-2 id column to describe composite PRIMARY KEY (id, timestamp); update inst-cur-3, inst-flow-ing-3, dod-ingest-ops to describe two-step transaction for idempotent ingest; change continuous aggregate start offset from 2 hours to 3 hours in inst-cagg-2, §3 input block, and dod-schema-migrations; add @cpt-begin/@cpt-end markers for inst-mig-8 in migrations.rs."
    - version: "1.7"
      date: "2026-05-03"
      changes:
        - "Issue #5: Replace inline SQL DDL/DML in 12 algorithm steps with prose descriptions (MAINT-FDESIGN-NO-001): inst-mig-1 through inst-mig-8, inst-cagg-1 through inst-cagg-3, inst-qraw-4"
    - version: "1.6"
      date: "2026-05-03"
      changes:
        - "Remove stale F7/F8 column references: status='active' from inst-cagg-1, inst-qagg-3, inst-qraw-3; status='active'/version=1 from inst-cur-6 (columns absent from inst-mig-2 schema since v1.4)"
    - version: "1.5"
      date: "2026-05-03"
      changes:
        - "Issue #2: Remove stale F7/F8 references from §1.2 Purpose (operator backfill/amendment/deactivation, retention enforcement, watermark retrieval), §1.2 Requirements (nfr-retention), dod-plugin-crate Touches (RetentionPolicy entity), dod-schema-migrations body (retention_policies table) and Touches (retention_policies DB, RetentionPolicy entity), dod-encryption-and-gts Encryption paragraph (enforce_retention sentence); add explicit Out-of-Scope paragraph in §7 listing deferred F7/F8 capabilities"
        - "Issue #4: Add formal AC for InGroup/InGroupSubtree predicate rejection to §6 Acceptance Criteria"
    - version: "1.4"
      date: "2026-05-01"
      changes:
        - "Remove remaining F7/F8 leakage: retention_policies table step (inst-mig-9) from schema-migrations; status/inactive_at/version columns from usage_records (inst-mig-2); Operator Operations & Retention DoD section; F7/F8 Testing & Observability DoD references; 4 F7/F8 Acceptance Criteria (deactivate, enforce_retention, amend version conflict, tenant retention)"
    - version: "1.3"
      date: "2026-05-01"
      changes:
        - "Remove F7/F8 out-of-scope content: Retention Policy Configuration flow (F7), Backfill Ingest flow (F8), backfill_ingest algo (F8), amend_record algo (F8), deactivate_record algo (F8), get_watermarks algo (F8), enforce_retention algo (F7), Usage Record Lifecycle state machine (F8)"
    - version: "1.2"
      date: "2026-05-01"
      changes:
        - "Add §2 Actor Flows: operator-schema-migration, operator-retention-configure, operator-backfill, storage-backend-ingest (resolves ARCH-FDESIGN-003)"
    - version: "1.1"
      date: "2026-05-01"
      changes:
        - "Add §5 Definitions of Done, §6 Acceptance Criteria, §7 Non-Applicability Notes"
    - version: "1.0"
      date: "2026-05-01"
      changes:
        - "Initial feature specification"
---

# Feature: Production Storage Plugin (TimescaleDB)

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Operator: Schema Migration](#operator-schema-migration)
  - [Storage Backend: Ingest Record](#storage-backend-ingest-record)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Schema Migrations](#schema-migrations)
  - [Continuous Aggregate — 1h Bucket Pre-Aggregation](#continuous-aggregate--1h-bucket-pre-aggregation)
  - [`create_usage_record` — Idempotent Ingest](#create_usage_record--idempotent-ingest)
  - [`AccessScope` → SQL Translator](#accessscope--sql-translator)
  - [`query_aggregated` — Aggregation Query with Routing](#query_aggregated--aggregation-query-with-routing)
  - [`query_raw` — Cursor-Based Raw Record Pagination](#query_raw--cursor-based-raw-record-pagination)
- [4. Definitions of Done](#4-definitions-of-done)
  - [Plugin Crate (`timescaledb-usage-collector-storage-plugin`)](#plugin-crate-timescaledb-usage-collector-storage-plugin)
  - [Schema & Migrations](#schema--migrations)
  - [Ingest Operations](#ingest-operations)
  - [Query Operations](#query-operations)
  - [Encryption & GTS Registration](#encryption--gts-registration)
  - [Testing & Observability](#testing--observability)
- [5. Acceptance Criteria](#5-acceptance-criteria)
- [6. Non-Applicability Notes](#6-non-applicability-notes)

<!-- /toc -->

- [x] `p1` - **ID**: `cpt-cf-usage-collector-featstatus-production-storage-plugin`

<!-- reference to DECOMPOSITION entry -->
- [x] `p1` - `cpt-cf-usage-collector-feature-production-storage-plugin`

## 1. Feature Context

### 1.1 Overview

Implements the production TimescaleDB storage plugin for the usage-collector
gateway: a full `UsageCollectorPluginClientV1` implementation providing
durable, high-throughput record persistence, aggregation query pushdown via
continuous aggregates, cursor-based raw pagination, operator write operations,
and configurable retention enforcement backed by TimescaleDB hypertable
partitioning.

### 1.2 Purpose

Delivers the first production storage backend enabling the usage-collector
gateway to meet its throughput, latency, and recovery objectives. Covers
the TimescaleDB plugin crate (`timescaledb-usage-collector-storage-plugin`):
GTS schema registration, database schema migrations, idempotent ingest with
counter delta accumulation, aggregation and raw query delegation.

**Requirements**: `cpt-cf-usage-collector-fr-pluggable-storage`,
`cpt-cf-usage-collector-nfr-query-latency` (≤ 500ms p95 for 30-day
single-tenant aggregation), `cpt-cf-usage-collector-nfr-throughput`
(≥ 10,000 records/sec sustained), `cpt-cf-usage-collector-nfr-rpo`
(RPO = 0 for committed records), `cpt-cf-usage-collector-nfr-recovery`
(RTO ≤ 15 min from backend recovery)

**Principles**: `cpt-cf-usage-collector-principle-pluggable-storage`

**Constraints**: `cpt-cf-usage-collector-constraint-single-plugin`,
`cpt-cf-usage-collector-constraint-types-registry`,
`cpt-cf-usage-collector-constraint-encryption`

### 1.3 Actors

| Actor | Role in Feature |
|-------|-----------------|
| `cpt-cf-usage-collector-actor-platform-operator` | Selects TimescaleDB as the active plugin via operator configuration; initiates schema migrations; configures retention policies per usage type and per tenant |
| `cpt-cf-usage-collector-actor-storage-backend` | TimescaleDB receives usage records for durable persistence; responds to aggregation pushdown, cursor-based raw pagination, watermark queries, and retention enforcement operations |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md)
- **Design**: [DESIGN.md](../DESIGN.md)
- **DECOMPOSITION**: [DECOMPOSITION.md](../DECOMPOSITION.md)
- **Dependencies**: `cpt-cf-usage-collector-feature-sdk-and-ingest-core`,
  `cpt-cf-usage-collector-feature-query-api`

## 2. Actor Flows (CDSL)

### Operator: Schema Migration

- [x] `p1` - **ID**: `cpt-cf-usage-collector-flow-production-storage-plugin-operator-schema-migration`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Success Scenarios**:
- Migration completes and all schema objects are present; plugin confirms readiness

**Error Scenarios**:
- TimescaleDB extension unavailable; operator receives `MigrationError` with diagnostic
- Continuous aggregate setup fails; operator receives `MigrationError::ContinuousAggregateSetupFailed`

**Steps**:
1. [x] - `p1` - Platform operator invokes the plugin migration command (CLI or gateway startup flag) - `inst-flow-smig-1`
2. [x] - `p1` - Plugin executes the idempotent migration sequence: `cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations` - `inst-flow-smig-2`
3. [x] - `p1` - **IF** migration succeeds — plugin logs `INFO` confirming all schema objects are present - `inst-flow-smig-3`
   1. [x] - `p1` - Plugin executes continuous aggregate setup: `cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate` - `inst-flow-smig-3a`
   2. [x] - `p1` - **IF** continuous aggregate setup succeeds — **RETURN** success feedback to operator - `inst-flow-smig-3b`
   3. [x] - `p1` - **IF** continuous aggregate setup fails — log `ERROR` with `MigrationError::ContinuousAggregateSetupFailed`; **RETURN** failure feedback to operator - `inst-flow-smig-3c`
4. [x] - `p1` - **IF** migration fails (e.g., extension unavailable) — log `ERROR` with `MigrationError` details; **RETURN** failure feedback to operator - `inst-flow-smig-4`

---

### Storage Backend: Ingest Record

- [x] `p1` - **ID**: `cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest`

**Actor**: `cpt-cf-usage-collector-actor-storage-backend`

**Success Scenarios**:
- Record inserted or confirmed as duplicate; `Ok(())` returned to gateway

**Error Scenarios**:
- Counter record missing `idempotency_key` or has negative `value`; `UsageCollectorPluginError::InvalidRecord` returned
- Transient DB failure; `UsageCollectorPluginError::Transient` returned; gateway circuit breaker handles retry

**Steps**:
1. [x] - `p1` - Gateway calls `create_usage_record(UsageRecord)` on the plugin - `inst-flow-ing-1`
2. [x] - `p1` - Plugin validates: `value >= 0` for counter records; `idempotency_key` present for counter records; **IF** either fails — **RETURN** `UsageCollectorPluginError::InvalidRecord` - `inst-flow-ing-2`
3. [x] - `p1` - Plugin executes idempotent INSERT: `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record` — deduplication via a two-step transaction against the `usage_idempotency_keys` table (ON CONFLICT DO NOTHING on the key row, then INSERT into `usage_records` if the key was newly claimed) - `inst-flow-ing-3`
4. [x] - `p1` - **IF** DB returns a transient error — **RETURN** `UsageCollectorPluginError::Transient`; gateway circuit breaker records the failure; outbox retries delivery - `inst-flow-ing-4`
5. [x] - `p1` - **RETURN** `Ok(())` — record inserted or confirmed duplicate; no distinction exposed to caller - `inst-flow-ing-5`

## 3. Processes / Business Logic (CDSL)

Internal system functions and procedures that do not interact with actors directly. These are the core plugin algorithms: database schema management, pre-aggregation maintenance, idempotent record ingest, and authorization scope translation.

### Schema Migrations

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations`

**Input**: `sqlx::PgPool` — connection pool to the TimescaleDB instance; migration runner invoked by the platform operator at plugin startup or on explicit migration command

**Output**: `Result<(), MigrationError>` — all schema objects created or already present; idempotent (safe to re-run)

**Steps**:
1. [x] - `p1` - Ensure the TimescaleDB extension is installed in the database; the operation is idempotent and safe to re-run on an already-configured instance — `inst-mig-1`
2. [x] - `p1` - Create the `usage_records` table with the following columns: `id` (UUID, required, auto-generated via `gen_random_uuid()`; part of composite PRIMARY KEY `(id, timestamp)` required by TimescaleDB hypertable partitioning), `tenant_id` (UUID, required), `module` (text, required), `kind` (text, `'counter'` or `'gauge'`), `metric` (text, required), `value` (numeric, required), `timestamp` (timestamptz, required), `idempotency_key` (nullable text; non-null enforced at upsert level for counter records), `resource_id` (UUID, required), `resource_type` (text, required), `subject_id` (nullable UUID), `subject_type` (nullable text), `ingested_at` (timestamptz, defaults to current time), `metadata` (nullable JSONB); if the table already exists, skip — `inst-mig-2`
3. [x] - `p1` - Convert `usage_records` into a TimescaleDB hypertable partitioned on `timestamp`; use the idempotent form (`if_not_exists`) to allow safe re-runs on an already-converted table — `inst-mig-3`
4. [x] - `p1` - Create a composite index on `(tenant_id, timestamp DESC)` named `idx_usage_records_tenant_time`; this is the mandatory primary filter index driving all time-range scans; idempotent — `inst-mig-4`
5. [x] - `p1` - Create a composite index on `(tenant_id, metric, timestamp DESC)` named `idx_usage_records_tenant_metric_time`; supports `usage_type` filter in aggregation and raw queries; idempotent — `inst-mig-5`
6. [x] - `p1` - Create a composite index on `(tenant_id, subject_id, timestamp DESC)` named `idx_usage_records_tenant_subject_time`; supports `subject` filter in aggregation and raw queries; idempotent — `inst-mig-6`
7. [x] - `p1` - Create a composite index on `(tenant_id, resource_id, timestamp DESC)` named `idx_usage_records_tenant_resource_time`; supports `resource` filter in aggregation and raw queries; idempotent — `inst-mig-7`
8. [x] - `p1` - Create a plain table `usage_idempotency_keys` with columns `tenant_id` (UUID, required) and `idempotency_key` (text, required), with `PRIMARY KEY (tenant_id, idempotency_key)`; this table is the cross-partition deduplication store for idempotent counter records; a separate plain table is required because TimescaleDB rejects unique indexes that omit the partition column on a hypertable; idempotent (`CREATE TABLE IF NOT EXISTS`) — `inst-mig-8`
9. [x] - `p1` - **RETURN** `Ok(())` — all schema objects are present and indexes are consistent - `inst-mig-9`

**Implements**: `cpt-cf-usage-collector-fr-pluggable-storage` (plugin owns its schema lifecycle), `cpt-cf-usage-collector-nfr-query-latency` (four composite indexes satisfy §3.7 mandate)

**Constraints**: `cpt-cf-usage-collector-constraint-encryption` (TLS-only `PgPool` connection enforced by plugin configuration; encryption at rest governed by platform infrastructure policy)

---

### Continuous Aggregate — 1h Bucket Pre-Aggregation

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate`

**Input**: `sqlx::PgPool` — invoked as part of schema migrations after the hypertable exists; `continuous_aggregate_refresh_interval` operational parameter (default: 30-minute schedule, 3-hour start offset, 1-hour end offset)

**Output**: `Result<(), MigrationError>` — `usage_agg_1h` continuous aggregate view and refresh policy created or already present; idempotent

**Operational parameter**: `continuous_aggregate_refresh_interval` — maximum refresh lag between latest ingested records and the pre-aggregated view; default schedule produces at most ~30 min lag; MUST be documented in plugin operator documentation and surfaced as a queryable parameter per DESIGN §3.7

**Steps**:
1. [x] - `p1` - Create the `usage_agg_1h` continuous aggregate materialized view over `usage_records`, grouping by 1-hour time buckets (`timestamp`), `tenant_id`, `metric`, `module`, `resource_type`, and `subject_type`; aggregate columns: sum of `value`, count of records, min of `value`, max of `value`; exclude `resource_id` and `subject_id` from GROUP BY to prevent cardinality explosion; `AVG` is not stored — computed at query time as `sum / NULLIF(count, 0)` for correctness across bucket merges; defer initial data population (`WITH NO DATA`); if the view already exists, skip — `inst-cagg-1`
2. [x] - `p1` - Register an automated refresh policy for `usage_agg_1h`: schedule interval 30 minutes, start offset 3 hours, end offset 1 hour; if a policy already exists, skip — `inst-cagg-2`
3. [x] - `p1` - **IF** the view was newly created (not already present) — trigger an initial manual refresh to populate historical data up to 1 hour before the current time — `inst-cagg-3`
4. [x] - `p1` - Verify the view exists and the refresh policy is registered; **IF** verification fails — **RETURN** `MigrationError::ContinuousAggregateSetupFailed` - `inst-cagg-4`
5. [x] - `p1` - **RETURN** `Ok(())` - `inst-cagg-5`

**Implements**: `cpt-cf-usage-collector-fr-pluggable-storage` (plugin-owned acceleration structure), `cpt-cf-usage-collector-nfr-query-latency` (pre-aggregation meets ≤ 500ms p95 for 30-day single-tenant aggregation without `resource_id`/`subject_id` GROUP BY)

**Constraints**: `cpt-cf-usage-collector-nfr-rpo` (continuous aggregate is an acceleration structure only; the `usage_records` hypertable remains the authoritative source of truth with RPO = 0 for committed records; aggregate eventual consistency lag bounded by `continuous_aggregate_refresh_interval`)

---

### `create_usage_record` — Idempotent Ingest

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record`

**Input**: `UsageRecord` — deserialized from gateway ingest payload; fields: `tenant_id`, `module`, `kind` (`'counter'` or `'gauge'`), `metric`, `value`, `timestamp`, `idempotency_key` (non-null for counter records; nullable for gauge records), `resource_id`, `resource_type`, `subject_id` (nullable), `subject_type` (nullable), `metadata` (nullable JSONB), `ingested_at` (gateway sets to `now()`)

**Output**: `Result<(), UsageCollectorPluginError>` — `Ok(())` on successful insert or confirmed duplicate; error on constraint violation or transient DB failure

**Counter-delta semantics**: for `kind = 'counter'`, each record's `value` is a non-negative delta contribution. The record is stored as-is alongside other deltas. The persistent total for any `(tenant_id, metric)` pair is `SUM(value)` over all active records — no separate accumulation table or running total is maintained. This matches DESIGN §3.7 Counter Accumulation.

**Steps**:
1. [x] - `p1` - Validate `value >= 0` when `kind = 'counter'`; **IF** negative — **RETURN** `UsageCollectorPluginError::InvalidRecord` (gateway enforces this before calling the plugin, but the plugin re-validates as a defensive check) - `inst-cur-1`
2. [x] - `p1` - Validate `idempotency_key` is present (non-null, non-empty) when `kind = 'counter'`; **IF** absent — **RETURN** `UsageCollectorPluginError::InvalidRecord` - `inst-cur-2`
3. [x] - `p1` - Open a transaction; INSERT into `usage_idempotency_keys (tenant_id, idempotency_key)` ON CONFLICT DO NOTHING; if 0 rows were claimed (duplicate key), rollback and return immediately (record already stored); otherwise INSERT the record into `usage_records` with all fields and commit the transaction. This two-step approach is required because TimescaleDB rejects unique indexes on `usage_records` that omit the partition column - `inst-cur-3`
4. [x] - `p1` - **IF** DB operation returns a unique constraint violation on a column other than the idempotency index (unexpected schema conflict) — **RETURN** `UsageCollectorPluginError::StorageError(err)` - `inst-cur-4`
5. [x] - `p1` - **IF** DB operation returns a transient error (connection lost, pool timeout, serialization failure) — **RETURN** `UsageCollectorPluginError::Transient(err)`; the gateway circuit breaker records this failure and the outbox library retries delivery - `inst-cur-5`
6. [x] - `p1` - Set `ingested_at = now()` at DB INSERT time (not passed from caller) - `inst-cur-6`
7. [x] - `p1` - **RETURN** `Ok(())` — record inserted or confirmed as duplicate via idempotency key; no distinction exposed to caller - `inst-cur-7`

**Implements**: `cpt-cf-usage-collector-fr-pluggable-storage` (implements `UsageCollectorPluginClientV1::create_usage_record`), `cpt-cf-usage-collector-fr-counter-semantics` (stores each delta record as-is; persistent total computed as SUM of active deltas; upsert target `(tenant_id, idempotency_key)` enforces at-most-once delivery per key)

**Constraints**: `cpt-cf-usage-collector-nfr-throughput` (≥ 10,000 records/sec sustained; single-row INSERT is the hot path for gauge records; counter records use a two-step transaction (idempotency-key INSERT + usage_records INSERT) which adds one extra round-trip per counter record; connection pool size and hypertable chunk cache govern throughput ceiling), `cpt-cf-usage-collector-nfr-rpo` (INSERT inside a DB transaction; committed record has RPO = 0)

---

### `AccessScope` → SQL Translator

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`

**Input**: `AccessScope` — compiled from PDP constraints by the gateway before delegating to the plugin; structure: `Vec<ConstraintGroup>` where each `ConstraintGroup` contains `Vec<Predicate>`; predicates include `TenantId(Vec<Uuid>)`, `ResourceId(Vec<Uuid>)`, `ResourceType(Vec<String>)`, `InGroup { group_id }`, `InGroupSubtree { group_id }`

**Output**: `Result<(String, Vec<SqlValue>), ScopeTranslationError>` — SQL WHERE fragment string (ready for embedding in a `WHERE (...)` clause) and a positional bind-parameter list; the fragment combines constraint groups with OR and predicates within each group with AND

**OR-of-ANDs preservation**: the translator MUST compile each `ConstraintGroup` into a separate AND branch and combine branches with OR. Flattening multiple groups into a single AND set is a security violation (`cpt-cf-usage-collector-constraint-or-of-ands-preservation`) — it widens access beyond the PDP-authorized scope.

**Steps**:
1. [x] - `p1` - **IF** `scope.groups` is empty — **RETURN** `ScopeTranslationError::EmptyScope`; callers must fail closed on empty scope (no constraints = deny all) - `inst-s2s-1`
2. [x] - `p1` - Initialize `group_fragments: Vec<String>` and `bind_params: Vec<SqlValue>` - `inst-s2s-2`
3. [x] - `p1` - **FOR EACH** `group` in `scope.groups` - `inst-s2s-3`
   1. [x] - `p1` - Initialize `predicate_fragments: Vec<String>` for this group - `inst-s2s-3a`
   2. [x] - `p1` - **FOR EACH** `predicate` in `group.predicates` - `inst-s2s-3b`
      1. [x] - `p1` - **IF** predicate is `InGroup` or `InGroupSubtree` — **RETURN** `ScopeTranslationError::UnsupportedPredicate { kind: "InGroup/InGroupSubtree" }`; these predicates require gateway pre-flattening before plugin invocation and are not supported at the plugin SQL translation layer - `inst-s2s-3b-i`
      2. [x] - `p1` - **IF** predicate is `TenantId(ids)` — append `tenant_id = ANY($N)` (or `tenant_id = $N` for single value) and bind `ids` - `inst-s2s-3b-ii`
      3. [x] - `p1` - **IF** predicate is `ResourceId(ids)` — append `resource_id = ANY($N)` and bind `ids` - `inst-s2s-3b-iii`
      4. [x] - `p1` - **IF** predicate is `ResourceType(types)` — append `resource_type = ANY($N)` and bind `types` - `inst-s2s-3b-iv`
   3. [x] - `p1` - Join `predicate_fragments` with ` AND `; wrap in parentheses: `(pred1 AND pred2 ...)`; append to `group_fragments` - `inst-s2s-3c`
4. [x] - `p1` - Join `group_fragments` with ` OR `; wrap entire expression in outer parentheses: `(group1 OR group2 ...)`; this is the final SQL WHERE fragment - `inst-s2s-4`
5. [x] - `p1` - **RETURN** `Ok((sql_fragment, bind_params))` - `inst-s2s-5`

**Implements**: `cpt-cf-usage-collector-constraint-or-of-ands-preservation` (each PDP constraint group compiles to a separate AND branch; branches combined with OR; flattening is explicitly prohibited)

**Constraints**: `cpt-cf-usage-collector-principle-pluggable-storage` (translator is plugin-internal using raw `sqlx`; gateway does not inspect or manipulate the SQL fragment); `InGroup`/`InGroupSubtree` predicate rejection MUST be a hard error, not a silent omission, to preserve fail-closed security posture

---

### `query_aggregated` — Aggregation Query with Routing

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated`

**Input**: `AggregationQuery` — fields: `scope: AccessScope` (compiled from PDP constraints), `time_range: (DateTime<Utc>, DateTime<Utc>)`, `function: AggregationFn` (`Sum | Count | Min | Max | Avg`), `group_by: Vec<GroupByDimension>` (`TimeBucket(BucketSize) | UsageType | Subject | Resource | Source`), `bucket_size: Option<BucketSize>` (required when `group_by` includes `TimeBucket`), optional user filters: `metric`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `source`

**Output**: `Result<Vec<AggregationResult>, UsageCollectorPluginError>` — aggregated result rows; empty vec for time ranges with no matching records; error on scope translation failure or DB failure

**Routing decision**: queries are routed to the `usage_agg_1h` continuous aggregate when all active user filters are on dimensions present in that view (`metric`, `resource_type`, `subject_type`, `source` / `module`); queries are routed to the raw `usage_records` hypertable when `resource_id` or `subject_id` is present as a user filter or GROUP BY dimension — these high-cardinality dimensions are intentionally excluded from `usage_agg_1h` (see `cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate`, `inst-cagg-1`)

**AVG composability**: `Avg` is not stored in the continuous aggregate; it is computed at query time as `SUM(sum_val) / NULLIF(SUM(cnt_val), 0)` over the pre-aggregated rows; this formula is mathematically correct across bucket merges (unlike averaging per-bucket averages)

**Steps**:
1. [x] - `p1` - Translate `scope` via `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`; **IF** translation returns `ScopeTranslationError` — **RETURN** `UsageCollectorPluginError::AccessDenied`; the call site must fail closed on empty or untranslatable scope - `inst-qagg-1`
2. [x] - `p1` - **IF** `resource_id` or `subject_id` is present in user filters OR `group_by` includes `Resource` (meaning `resource_id`) OR `group_by` includes `Subject` (meaning `subject_id`) — **ROUTE TO** raw hypertable path (step 3); **ELSE** — **ROUTE TO** continuous aggregate path (step 4) - `inst-qagg-2`
3. [x] - `p1` - **[Raw hypertable path]** Build SELECT query against `usage_records`; apply WHERE clause: scope SQL fragment AND `timestamp >= $start AND timestamp <= $end` AND optional filters (`metric`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `source`); build GROUP BY from `query.group_by` dimensions; build SELECT expressions using `query.function` (`SUM(value)`, `COUNT(*)`, `MIN(value)`, `MAX(value)`, or `AVG(value)` directly on raw records); apply `ORDER BY` on bucket start when `TimeBucket` is in `group_by`; **GO TO** step 5 - `inst-qagg-3`
4. [x] - `p1` - **[Continuous aggregate path]** Build SELECT query against `usage_agg_1h`; apply WHERE clause: scope SQL fragment AND `bucket >= $start AND bucket <= $end` AND optional filters (`metric`, `resource_type`, `subject_type`, `source`); build GROUP BY from `query.group_by` dimensions (mapped to aggregate columns: `bucket` for `TimeBucket`, `metric` for `UsageType`, `subject_type` for `Subject`, `resource_type` for `Resource`, `module` for `Source`); build SELECT expressions from pre-aggregated columns: `SUM(sum_val)` for `Sum`, `SUM(cnt_val)` for `Count`, `MIN(min_val)` for `Min`, `MAX(max_val)` for `Max`, `SUM(sum_val) / NULLIF(SUM(cnt_val), 0)` for `Avg`; apply `ORDER BY bucket_start` when `TimeBucket` is in `group_by`; **GO TO** step 5 - `inst-qagg-4`
5. [x] - `p1` - Execute the constructed query with all bind parameters using `sqlx::PgPool`; **IF** DB returns transient error — **RETURN** `UsageCollectorPluginError::Transient(err)` - `inst-qagg-5`
6. [x] - `p1` - Map result rows to `Vec<AggregationResult>`; set `function` field to `query.function`; populate optional dimension fields (`bucket_start`, `usage_type`, `subject_id`, `subject_type`, `resource_id`, `resource_type`, `source`) only when the corresponding `GroupByDimension` was present in `query.group_by`; absent dimensions MUST be `None` - `inst-qagg-6`
7. [x] - `p1` - **RETURN** `Ok(results)` — empty vec is a valid result for time ranges with no matching data - `inst-qagg-7`

**Implements**: `cpt-cf-usage-collector-fr-pluggable-storage` (implements `UsageCollectorPluginClientV1::query_aggregated`), `cpt-cf-usage-collector-nfr-query-latency` (continuous aggregate path meets ≤ 500ms p95 for 30-day single-tenant aggregation on low-cardinality dimensions; raw hypertable path used only when high-cardinality filters are present and the acceleration structure is inapplicable)

**Constraints**: `cpt-cf-usage-collector-constraint-or-of-ands-preservation` (scope SQL fragment preserves OR-of-ANDs structure via `scope-to-sql`; never flattened)

---

### `query_raw` — Cursor-Based Raw Record Pagination

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-production-storage-plugin-query-raw`

**Input**: `RawQuery` — fields: `scope: AccessScope` (compiled from PDP constraints), `time_range: (DateTime<Utc>, DateTime<Utc>)`, optional user filters: `metric`, `resource_id`, `resource_type`, `subject_id`, `cursor: Option<Cursor>` (opaque to caller; encodes `(timestamp, id)` composite position), `page_size: usize`

**Output**: `Result<PagedResult<UsageRecord>, UsageCollectorPluginError>` — page of active records plus optional next cursor; empty page when all records exhausted; error on scope translation failure, cursor decode failure, or DB failure

**Cursor encoding**: the cursor is a base64-encoded tuple `(timestamp_iso8601, id_uuid)` — opaque to API callers; the plugin owns encoding and decoding; a `None` cursor means first page

**Cursor stability**: `id` is the stable tiebreaker within records sharing the same timestamp, ensuring no skipped or duplicated rows under concurrent inserts mid-pagination; the condition `(timestamp > $cursor_ts) OR (timestamp = $cursor_ts AND id > $cursor_id)` is a tuple comparison that never double-counts ties

**Steps**:
1. [x] - `p1` - Translate `scope` via `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`; **IF** translation returns `ScopeTranslationError` — **RETURN** `UsageCollectorPluginError::AccessDenied` - `inst-qraw-1`
2. [x] - `p1` - **IF** `query.cursor` is `Some(cursor)` — base64-decode the cursor bytes; parse `(timestamp: DateTime<Utc>, id: Uuid)` from the decoded bytes; **IF** decode fails — **RETURN** `UsageCollectorPluginError::InvalidCursor` - `inst-qraw-2`
3. [x] - `p1` - Build SELECT query against `usage_records`; always include in WHERE: scope SQL fragment AND `timestamp >= $start AND timestamp <= $end`; append optional user filters when present: `AND metric = $metric`, `AND resource_id = $resource_id`, `AND resource_type = $resource_type`, `AND subject_id = $subject_id` - `inst-qraw-3`
4. [x] - `p1` - **IF** cursor is present — append a keyset advancement condition that selects records strictly after the cursor position using tuple comparison on `(timestamp, id)`: records with a later timestamp, or records with the same timestamp and a later `id`; this condition never skips or double-counts rows under concurrent inserts; **IF** cursor is absent — no cursor condition is appended (first page) — `inst-qraw-4`
5. [x] - `p1` - Append `ORDER BY timestamp ASC, id ASC LIMIT $page_size`; `page_size` is bounded at the gateway layer (e.g., max 1000); `id` is the tiebreaker ensuring deterministic ordering for records sharing the same timestamp - `inst-qraw-5`
6. [x] - `p1` - Execute the query with all bind parameters; **IF** DB returns transient error — **RETURN** `UsageCollectorPluginError::Transient(err)` - `inst-qraw-6`
7. [x] - `p1` - Map result rows to `Vec<UsageRecord>`; **IF** result count equals `page_size` — take the last row's `(timestamp, id)`, base64-encode as the next cursor; **IF** result count is less than `page_size` — set next cursor to `None` (page exhausted) - `inst-qraw-7`
8. [x] - `p1` - **RETURN** `Ok(PagedResult { records, next_cursor })` — empty `records` with `next_cursor = None` is a valid exhausted-page response - `inst-qraw-8`

**Implements**: `cpt-cf-usage-collector-fr-pluggable-storage` (implements `UsageCollectorPluginClientV1::query_raw`), `cpt-cf-usage-collector-nfr-query-latency` (keyset cursor with TimescaleDB chunk pruning on time range avoids full-table scans; the `(tenant_id, timestamp)` index drives the seek)

**Constraints**: `cpt-cf-usage-collector-constraint-or-of-ands-preservation` (scope SQL fragment preserves OR-of-ANDs structure via `scope-to-sql`; cursor is opaque to callers and MUST NOT be inspected or modified by the gateway)

---

## 4. Definitions of Done

Specific implementation tasks derived from the algorithms above.

### Plugin Crate (`timescaledb-usage-collector-storage-plugin`)

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-production-storage-plugin-plugin-crate`

The system **MUST** implement the `timescaledb-usage-collector-storage-plugin` crate providing: a full `UsageCollectorPluginClientV1` implementation registered via the `UsageCollectorStoragePluginSpecV1` GTS schema; a `SecureConn`-managed `sqlx::PgPool` connection pool with configurable size and timeout; startup validation that rejects missing `database_url` or failed TLS negotiation as hard errors; and registration with the gateway `ClientHub` via GTS discovery at startup.

**Implements**:
- `cpt-cf-usage-collector-component-storage-plugin`

**Constraints**: `cpt-cf-usage-collector-constraint-single-plugin`, `cpt-cf-usage-collector-constraint-types-registry`

**Touches**:
- Crate: `timescaledb-usage-collector-storage-plugin`
- Entities: `UsageRecord`

**Configuration**:

| Parameter | Type | Valid range | Default | Validation | Runtime-changeable |
|-----------|------|-------------|---------|------------|--------------------|
| `database_url` | String | valid PostgreSQL URL | — | MUST be present; TLS required (`sslmode=require` or equivalent) | No — requires restart |
| `pool_size_min` | u32 | 1–64 | 2 | must be ≥ 1 | No |
| `pool_size_max` | u32 | 1–128 | 16 | must be ≥ `pool_size_min` | No |
| `retention_default` | duration | 7 days – 7 years | 365 days | must be in `[7d, 7y]` | No |
| `connection_timeout` | duration | 1s–60s | 10s | must be > 0 | No |

All parameters are static and require a plugin restart to change. No feature flags are used; the plugin is selected exclusively by operator configuration (GTS resolution at gateway startup).

**Health & diagnostics**: The plugin MUST report `storage_health_status = 1` (healthy) when the connection pool can execute a liveness probe against TimescaleDB and `storage_health_status = 0` (unreachable) otherwise. This metric feeds the platform-level readiness check (`/health/ready`). First-level troubleshooting: (1) verify `database_url` is reachable from the gateway host; (2) check `storage_health_status` metric; (3) inspect structured logs for pool exhaustion or TLS handshake errors; (4) confirm TimescaleDB has the `timescaledb` extension installed.

**Audit logging**: Plugin registration and successful GTS schema resolution are logged at `INFO`. Startup failures (connection pool creation, TLS negotiation, GTS registration) are logged at `ERROR` with the error type only — `database_url` and credentials MUST NOT appear in log output. No per-operation audit is produced at the plugin layer; audit of individual record operations is delegated to the gateway audit layer per `cpt-cf-usage-collector-constraint-no-business-logic`.

---

### Schema & Migrations

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-production-storage-plugin-schema-migrations`

The system **MUST** implement idempotent schema migrations that: enable the `timescaledb` extension; create the `usage_records` hypertable with all required columns and four composite indexes and a separate `usage_idempotency_keys` plain table for cross-partition idempotency deduplication; and create the `usage_agg_1h` continuous aggregate with a 30-minute scheduled refresh policy and a 3-hour start / 1-hour end offset. All migration steps MUST be idempotent and safe to re-run on an already-migrated schema without error.

**Implements**:
- `cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations`
- `cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate`

**Constraints**: `cpt-cf-usage-collector-constraint-single-plugin`, `cpt-cf-usage-collector-constraint-types-registry`

**Touches**:
- DB: `usage_records` (hypertable), `usage_agg_1h` (continuous aggregate), `usage_idempotency_keys` (plain table)
- Entities: `UsageRecord`

**Resource management**: The migration runner holds a single pool connection during the migration sequence and releases it immediately on completion or error. Continuous aggregate creation uses `WITH NO DATA` to defer the initial population to the scheduled refresh policy; the migration-time manual refresh is bounded by the volume of existing data. All DDL steps use idempotency guards (`IF NOT EXISTS` / `if_not_exists`) to prevent duplicate-object errors.

**Recovery**: Schema migrations are forward-only — rollback of a partially applied migration requires manual DBA intervention. If continuous aggregate setup fails, the error surfaces at gateway startup with a `MigrationError::ContinuousAggregateSetupFailed` diagnostic; the operator must resolve the TimescaleDB configuration issue and restart. Historical data present before the first migration run is included in the migration-time manual refresh.

**Data access patterns**: All schema objects are created via DDL operations only; no data queries are issued during migration. TimescaleDB automatically propagates new indexes to existing hypertable chunks via its chunk inheritance mechanism.

---

### Ingest Operations

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-production-storage-plugin-ingest-ops`

The system **MUST** implement the idempotent ingest write path: `create_usage_record` persisting a single record using a two-step transaction: INSERT into `usage_idempotency_keys` ON CONFLICT DO NOTHING, early return on 0 rows claimed, then INSERT into `usage_records`; and `scope_to_sql` translating the PDP `AccessScope` to a SQL WHERE fragment that preserves the OR-of-ANDs structure of the original PDP constraint groups. Counter records require a non-null `idempotency_key` and a non-negative `value`; these are enforced defensively at the plugin layer even if the gateway pre-validates.

**Implements**:
- `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record`
- `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`

**Constraints**: `cpt-cf-usage-collector-constraint-or-of-ands-preservation`, `cpt-cf-usage-collector-constraint-no-business-logic`

**Touches**:
- DB: `usage_records`
- Entities: `UsageRecord`

**Concurrency**: The idempotent upsert is safe under concurrent inserts — duplicate records sharing the same `(tenant_id, idempotency_key)` are silently dropped without error, matching the at-most-once delivery semantic of the outbox pipeline. No row-level locks are held beyond the implicit single-row INSERT lock; TimescaleDB hypertable chunk routing is lock-free for non-overlapping time ranges.

**Audit logging**: Individual ingest operations are not audited at the plugin layer — audit of ingest API calls is delegated to the platform-wide API audit layer. Plugin-level structured log events: `INFO` on successful insert, `DEBUG` on idempotency deduplication, `WARN` on `InvalidRecord` validation failure, `ERROR` on unexpected constraint violations. The `usage_dedup_total` metric MUST be incremented for each deduplicated record.

**Observability**: `usage_ingestion_total` MUST be incremented with `status = "success"` for each inserted record and `status = "dedup"` for each deduplicated record. `usage_ingestion_latency_ms` MUST be recorded as a histogram observation per `create_usage_record` invocation. `usage_schema_validation_errors_total` MUST be incremented for each `InvalidRecord` return.

---

### Query Operations

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-production-storage-plugin-query-ops`

The system **MUST** implement the two read operations: `query_aggregated` routing to the `usage_agg_1h` continuous aggregate when all active filters are on low-cardinality dimensions (`metric`, `resource_type`, `subject_type`, `source`) and falling back to the raw `usage_records` hypertable when `resource_id` or `subject_id` is present in filters or GROUP BY dimensions; and `query_raw` delivering cursor-based pagination with stable keyset ordering on `(timestamp, id)`. Both operations apply the PDP `AccessScope` via `scope_to_sql` before query construction and fail closed on scope translation errors.

**Implements**:
- `cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated`
- `cpt-cf-usage-collector-algo-production-storage-plugin-query-raw`
- `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`

**Constraints**: `cpt-cf-usage-collector-constraint-or-of-ands-preservation`, `cpt-cf-usage-collector-constraint-types-registry`

**Touches**:
- DB: `usage_records`, `usage_agg_1h`
- Entities: `UsageRecord`, `AggregationResult`

**Concurrency**: The `query_raw` keyset cursor on `(timestamp, id)` is stable under concurrent inserts — new records inserted mid-pagination are picked up in later pages without skipping or duplicating earlier rows. No shared lock is held between pages. `query_aggregated` operates on snapshot-consistent data within the query transaction; concurrent inserts may not appear in in-progress aggregate results but do not corrupt them.

**Observability**: `usage_query_latency_ms` MUST be recorded as a histogram observation per invocation, labeled with `query_type = "aggregated"` or `query_type = "raw"`. The routing decision (continuous aggregate vs raw hypertable) SHOULD be logged at `DEBUG` level with the triggering dimension.

**Data access patterns**: The continuous aggregate path is the primary hot path for billing and reporting queries — it reads from `usage_agg_1h` (pre-aggregated, indexed on `bucket`, `tenant_id`, `metric`) to meet the 500ms p95 latency target. The raw hypertable fallback uses the `(tenant_id, timestamp)` composite index for time-range seeks. The `query_raw` keyset pagination uses the same `(tenant_id, timestamp)` index plus the `id` tiebreaker for cursor stability.

---

### Encryption & GTS Registration

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-production-storage-plugin-encryption-and-gts`

The system **MUST** enforce TLS for all connections to TimescaleDB by requiring `sslmode=require` (or equivalent) in the connection parameters; plaintext connections MUST be rejected at pool initialization as a hard startup error. The plugin MUST register its GTS schema (`UsageCollectorStoragePluginSpecV1`) at startup; registration failure is also a hard startup error. Encryption at rest is governed by platform infrastructure policy (encrypted tablespace or OS-level encryption); no per-record key management is required for non-PII usage data.

**Implements**:
- `cpt-cf-usage-collector-component-storage-plugin`

**Constraints**: `cpt-cf-usage-collector-constraint-encryption`, `cpt-cf-usage-collector-constraint-types-registry`

**Touches**:
- GTS registry: `UsageCollectorStoragePluginSpecV1`
- Channel: TLS-only connection to TimescaleDB

**Encryption**: All plugin connections to TimescaleDB MUST use TLS; the `database_url` is managed via `SecureConn` and MUST NOT appear in logs or error messages. Encryption at rest is mandatory and governed by platform infrastructure policy (encrypted tablespace or OS-level encryption for the TimescaleDB data directory). Encryption key management is the responsibility of the platform secret management system.

**Audit logging**: GTS schema registration is logged at `INFO` on success. Startup failures (TLS rejection, GTS registration failure) are logged at `ERROR` with the error type only — connection string and credentials MUST NOT be included. No per-record encryption audit is required for non-PII usage data.

---

### Testing & Observability

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-production-storage-plugin-testing-and-observability`

The system **MUST** implement a two-level test suite: Level 1 inline unit tests (no DB) in `src/domain/client_tests.rs` covering all validation branches, error paths, and mock boundaries; Level 2 integration tests in `tests/integration.rs` gated with `#[cfg(feature = "integration")]` and run via `cargo test --features integration`, using a `timescale/timescaledb:latest-pg16` container via `testcontainers::GenericImage`. All DESIGN §3.8 metrics owned by the storage plugin MUST be emitted correctly and verified in both test levels (via mock registry in unit tests; via real registry in integration tests).

**Implements**:
- `cpt-cf-usage-collector-algo-production-storage-plugin-schema-migrations`
- `cpt-cf-usage-collector-algo-production-storage-plugin-continuous-aggregate`
- `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record`
- `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql`
- `cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated`
- `cpt-cf-usage-collector-algo-production-storage-plugin-query-raw`
- `cpt-cf-usage-collector-component-storage-plugin`

**Constraints**: `cpt-cf-usage-collector-constraint-single-plugin`, `cpt-cf-usage-collector-constraint-types-registry`, `cpt-cf-usage-collector-constraint-encryption`, `cpt-cf-usage-collector-constraint-or-of-ands-preservation`

**Touches**:
- Crates: `timescaledb-usage-collector-storage-plugin` (unit: `src/domain/client_tests.rs`; integration: `tests/integration.rs`)
- Metrics: `usage_ingestion_total`, `usage_ingestion_latency_ms`, `usage_dedup_total`, `usage_schema_validation_errors_total`, `usage_query_latency_ms`, `storage_health_status` (DESIGN §3.8)

**Observability**: All DESIGN §3.8 metrics owned by the storage plugin MUST be emitted: `usage_ingestion_total` (per-tenant, per-status counter), `usage_ingestion_latency_ms` (histogram per invocation), `usage_dedup_total` (per-tenant deduplication counter), `usage_query_latency_ms` (per-query-type histogram), `storage_health_status` (gauge; 1 = healthy, 0 = unreachable). Structured log events MUST carry `correlation_id`, `tenant_id`, operation type, and handler latency. PII exclusion: record `value`, `metadata`, `subject_id`, and `resource_id` MUST NOT appear in structured log fields.

**Health & diagnostics**: The integration test suite MUST include a health-check test verifying `storage_health_status = 1` when the pool is connected and `storage_health_status = 0` after the connection is interrupted.

**Resource management**: Integration tests MUST use the `testcontainers` container drop handle to clean up the TimescaleDB container at test teardown; no persistent containers between test runs. Unit tests require no external resources.

## 5. Acceptance Criteria

- [ ] Given N concurrent `create_usage_record` calls with the same `(tenant_id, idempotency_key)`, exactly one row persists in `usage_records`; all calls return `Ok(())`
- [ ] Counter total for a `(tenant_id, metric)` pair returned by `query_aggregated` equals the sum of all active record values; no separate running total is maintained
- [ ] A gauge record is stored with its exact submitted value; no accumulation is applied at ingest time; `query_aggregated(Avg)` on a single gauge record returns that stored value
- [ ] `query_aggregated` routes to `usage_agg_1h` when all active filters are on low-cardinality dimensions (`metric`, `resource_type`, `subject_type`, `source`); it routes to the raw `usage_records` hypertable when `resource_id` or `subject_id` is present in filters or GROUP BY
- [ ] `query_raw` with a valid cursor returns records in stable ascending `(timestamp, id)` order; no rows are skipped or duplicated when concurrent inserts occur between pages
- [ ] `query_aggregated` via the continuous aggregate path on a 30-day single-tenant time range completes within 500ms p95, meeting `cpt-cf-usage-collector-nfr-query-latency`
- [ ] `create_usage_record` sustains ≥ 10,000 records/sec under representative load, meeting `cpt-cf-usage-collector-nfr-throughput`
- [ ] `scope_to_sql` with an `AccessScope` containing multiple `ConstraintGroup` entries generates a WHERE fragment where each group is a distinct AND branch and branches are combined with OR; no group flattening occurs
- [ ] Plugin startup fails with a descriptive error (no credentials in message) when `database_url` is missing, TLS negotiation is rejected, or GTS schema registration fails; the gateway process does not start
- [ ] Level 2 integration tests pass when run with `cargo test --features integration` against a freshly migrated `timescale/timescaledb:latest-pg16` container
- [ ] `scope_to_sql` returns `ScopeTranslationError::UnsupportedPredicate` when the `AccessScope` contains any `InGroup` or `InGroupSubtree` predicate; the calling operation (`query_aggregated`, `query_raw`) translates this to `UsageCollectorPluginError::AccessDenied` and returns the error without executing any query.

**Test data requirements**:
(1) At least one tenant with counter records spanning three or more 1-hour time buckets across a > 30-day window and one tenant with gauge records.
(2) Idempotency collision test: submit ≥ 5 concurrent `create_usage_record` calls with the same `(tenant_id, idempotency_key)`; verify exactly one row in `usage_records`.
(3) Cursor stability test: establish a baseline page of records, insert additional records with timestamps in the already-paged range, verify that the next page does not contain duplicates or skip expected records.

**Test coverage guidance**:
Unit (`src/domain/client_tests.rs`): `create_usage_record` — valid counter insert, valid gauge insert, negative counter value rejected, missing `idempotency_key` for counter rejected, transient DB error path; `scope_to_sql` — single group, multiple groups (OR-of-ANDs preserved), empty scope (fail-closed), `InGroup` predicate rejection.
Integration (`tests/integration.rs`, `--features integration`): schema migration idempotency (run migration twice, verify no error); idempotent upsert under concurrent inserts; `query_aggregated` routing (continuous aggregate vs raw hypertable); `query_raw` cursor stability under concurrent inserts; health-check metric (`storage_health_status`).
E2E: deferred — the full emitter → gateway → plugin → TimescaleDB path is covered by integration tests; a compose-based e2e suite is out of scope for this feature.
Performance: measure `create_usage_record` throughput against `cpt-cf-usage-collector-nfr-throughput` (≥ 10,000 records/sec) and `query_aggregated` p95 latency against `cpt-cf-usage-collector-nfr-query-latency` (≤ 500ms) using representative data volumes in the integration environment.

**Success metrics**:
(1) All unit and Level 2 integration tests pass on CI via `cargo test --features integration`.
(2) `create_usage_record` throughput ≥ 10,000 records/sec under sustained load on representative hardware.
(3) `query_aggregated` (continuous aggregate path, 30-day window, single tenant) p95 latency ≤ 500ms on representative data volume.
(4) Zero unexpected constraint violations in production; deduplication rate observable via `usage_dedup_total` metric.

## 6. Non-Applicability Notes

**Out of Scope (Deferred to F7/F8)**. The following capabilities are explicitly out of scope for this feature and are deferred to their respective feature specifications: operator backfill/amendment/deactivation (`backfill_ingest`, `amend_record`, `deactivate_record`) — deferred to F8; retention enforcement (`enforce_retention`) — deferred to F7; watermark retrieval (`get_watermarks`) — deferred to F8. None of these operations are implemented, referenced, or tested by this feature.

**UX (User Experience & Accessibility)**. Not applicable. This feature implements a Rust plugin crate exposing a machine-to-machine `UsageCollectorPluginClientV1` trait. There is no user-facing UI, no end-user interaction model, no user-visible error messages, and no accessibility requirements.

**COMPL (Regulatory & Privacy Compliance)**. Not applicable at the feature level. Tenant identifiers (`tenant_id`, `resource_id`, `subject_id`) are opaque billing UUIDs — not regulated personal data under the project's data classification policy. Encryption in transit is enforced by `cpt-cf-usage-collector-constraint-encryption` (TLS to TimescaleDB); encryption at rest is governed by platform infrastructure policy. Data retention compliance is implemented by `enforce_retention` per `cpt-cf-usage-collector-nfr-retention` (7 days – 7 years). No consent handling, data subject rights, or cross-border transfer considerations apply to opaque billing metrics.

**BIZ (Business Logic)**. Not applicable. The storage plugin contains no business logic per `cpt-cf-usage-collector-constraint-no-business-logic`. Record metadata is stored as opaque JSONB without indexing or interpretation. Pricing, rating, billing rules, invoice generation, and quota enforcement are the responsibility of downstream consumers; the plugin stores and retrieves records as-is.

**INT (External Integrations beyond TimescaleDB)**. Not applicable. The plugin's sole external integration is TimescaleDB. There are no message brokers, external APIs, webhooks, or secondary datastores. Audit event publishing (`WriteAuditEvent`) is delegated to the gateway layer, not the plugin.
