---
cpt:
  version: "1.4"
  changelog:
    - version: "1.4"
      date: "2026-04-27"
      changes:
        - "§3: removed inst-authz-1 (no-open-transaction assertion — not implementable due to platform limitations); renumbered authorize-for steps; marked authorize-for algo [x]"
        - "§3: added inst-enq-5b code marker; marked step [x]"
        - "§1: updated featstatus and DECOMP ref to [x] — all p1 DoD items and CDSL blocks complete"
    - version: "1.3"
      date: "2026-04-26"
      changes:
        - "§3: added queue overflow, message ordering, optional field serialization, and cache integration N/A notes (fixes REL-FDESIGN-004, INT-FDESIGN-004, DATA-FDESIGN-003)"
        - "§5.2: added data access patterns N/A, data archival N/A, connection management N/A notes (fixes DATA-FDESIGN-001, DATA-FDESIGN-004, INT-FDESIGN-002)"
        - "§5.3: added recovery, data archival N/A, configuration table, health & diagnostics, encryption constraint deferral (fixes REL-FDESIGN-005, DATA-FDESIGN-004, OPS-FDESIGN-002/003)"
        - "§2/§3/§5.4: added p2 deferral notes on deferred CDSL items (fix MAINT-FDESIGN-003)"
        - "§5: added Known Limitations / Technical Debt subsection (fix MAINT-FDESIGN-003)"
        - "§6: added test data requirements, test coverage guidance, success metrics (fixes TEST-FDESIGN-001/002, BIZ-FDESIGN-002)"
    - version: "1.2"
      date: "2026-04-26"
      changes:
        - "§3: added hot-path annotation for `authorize_for()` (fix PERF-FDESIGN-001)"
        - "§5.2: added resource management, concurrency, and observability notes (fixes PERF-FDESIGN-002, PERF-FDESIGN-003, OPS-FDESIGN-001)"
        - "§5.3: added observability and rollout/rollback notes (fixes OPS-FDESIGN-001, OPS-FDESIGN-004)"
        - "§1.2: added NFR target reference (fix PERF-FDESIGN-004)"
    - version: "1.1"
      date: "2026-04-26"
      changes:
        - "§1.3: replaced inline actor definition table with PRD reference list (fix BIZ-FDESIGN-NO-001)"
        - "§1.2: added `p2` priority annotation to `cpt-cf-usage-collector-fr-record-metadata` (fix SEM-FDESIGN-005)"
---

# Feature: Core SDK, Emitter & In-Process Ingest


<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Usage Emission Flow](#usage-emission-flow)
  - [Module Config Retrieval Flow](#module-config-retrieval-flow)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Phase 1: `authorize_for()` Authorization](#phase-1-authorize_for-authorization)
  - [Phase 2: `build_usage_record().enqueue()` — In-Transaction Enqueue](#phase-2-build_usage_recordenqueue--in-transaction-enqueue)
  - [Outbox Delivery `MessageHandler`](#outbox-delivery-messagehandler)
  - [Gateway Ingest Handler](#gateway-ingest-handler)
  - [Static Module Config Resolution](#static-module-config-resolution)
- [4. States (CDSL)](#4-states-cdsl)
- [5. Definitions of Done](#5-definitions-of-done)
  - [SDK Crate (`usage-collector-sdk`)](#sdk-crate-usage-collector-sdk)
  - [Emitter Crate (`usage-emitter`)](#emitter-crate-usage-emitter)
  - [Gateway Crate (`usage-collector`) — Ingest & Config](#gateway-crate-usage-collector--ingest--config)
  - [No-Op Storage Plugin (`noop-usage-collector-storage-plugin`)](#no-op-storage-plugin-noop-usage-collector-storage-plugin)
  - [Known Limitations / Technical Debt](#known-limitations--technical-debt)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Non-Applicability Notes](#7-non-applicability-notes)

<!-- /toc -->

- [x] `p2` - **ID**: `cpt-cf-usage-collector-featstatus-sdk-and-ingest-core`
<!-- STATUS: IMPLEMENTED — all p1 DoD items and all CDSL blocks are [x]. -->

<!-- reference to DECOMPOSITION entry -->
- [x] `p1` - `cpt-cf-usage-collector-feature-sdk-and-ingest-core`
## 1. Feature Context

### 1.1 Overview

Establishes the core Usage Collector data model, SDK trait boundaries, two-phase authorization emitter, transactional outbox pipeline, gateway ingest handler, static metric configuration, and no-op storage plugin — delivering the complete in-process emission path from `authorize_for()` through the outbox background pipeline to the gateway and plugin.

### 1.2 Purpose

Implements the foundation for all usage collection capabilities. Covers the SDK crate (`usage-collector-sdk`), the emitter crate (`usage-emitter`), the gateway crate (`usage-collector`) ingest and config endpoints, and the no-op storage plugin (`noop-usage-collector-storage-plugin`). This feature is the prerequisite for all other Usage Collector features.

**Requirements**: `cpt-cf-usage-collector-fr-ingestion`, `cpt-cf-usage-collector-fr-idempotency`, `cpt-cf-usage-collector-fr-delivery-guarantee`, `cpt-cf-usage-collector-fr-counter-semantics`, `cpt-cf-usage-collector-fr-gauge-semantics`, `cpt-cf-usage-collector-fr-tenant-attribution`, `cpt-cf-usage-collector-fr-resource-attribution`, `cpt-cf-usage-collector-fr-subject-attribution`, `cpt-cf-usage-collector-fr-tenant-isolation`, `cpt-cf-usage-collector-fr-ingestion-authorization`, `cpt-cf-usage-collector-fr-pluggable-storage`, `cpt-cf-usage-collector-fr-record-metadata` (`p2`), `cpt-cf-usage-collector-nfr-availability`, `cpt-cf-usage-collector-nfr-ingestion-latency`, `cpt-cf-usage-collector-nfr-authentication`, `cpt-cf-usage-collector-nfr-authorization`, `cpt-cf-usage-collector-nfr-scalability`, `cpt-cf-usage-collector-nfr-fault-tolerance`, `cpt-cf-usage-collector-nfr-recovery`, `cpt-cf-usage-collector-nfr-graceful-degradation`, `cpt-cf-usage-collector-nfr-rpo`

**NFR targets (from PRD)**: `cpt-cf-usage-collector-nfr-ingestion-latency`
and `cpt-cf-usage-collector-nfr-availability` define numeric targets; values
are defined in PRD §NFRs and are not reproduced here. See PRD for
response-time and throughput targets.

**Principles**: `cpt-cf-usage-collector-principle-source-side-persistence`, `cpt-cf-usage-collector-principle-pluggable-storage`, `cpt-cf-usage-collector-principle-tenant-from-ctx`, `cpt-cf-usage-collector-principle-fail-closed`, `cpt-cf-usage-collector-principle-scoped-source-attribution`, `cpt-cf-usage-collector-principle-two-phase-authz`

### 1.3 Actors

**Actors** (defined in PRD.md):
- `cpt-cf-usage-collector-actor-usage-source` — initiates emission flows
- `cpt-cf-usage-collector-actor-platform-developer` — SDK integrator
- `cpt-cf-usage-collector-actor-storage-backend` — storage delegation target

### 1.4 References

- **PRD**: [PRD.md](../PRD.md)
- **Design**: [DESIGN.md](../DESIGN.md)
- **DECOMPOSITION**: [DECOMPOSITION.md](../DECOMPOSITION.md)
- **Dependencies**: None

## 2. Actor Flows (CDSL)

### Usage Emission Flow

- [x] `p1` - **ID**: `cpt-cf-usage-collector-flow-sdk-and-ingest-core-emit`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Success Scenarios**:
- Record is durably enqueued in the source's local outbox within the caller's DB transaction
- Outbox background pipeline delivers the record to the gateway; plugin confirms storage

**Error Scenarios**:
- PDP denies `USAGE_RECORD`/`CREATE` → `UsageEmitterError::AuthorizationDenied`; no outbox INSERT
- Module not configured → `UsageEmitterError::ModuleNotConfigured`; no outbox INSERT
- Metric not in allowed list → `UsageEmitterError::MetricNotAllowed`; no outbox INSERT
- Counter record with negative value or missing idempotency key → `UsageEmitterError::InvalidRecord`; no outbox INSERT
- `AuthorizedUsageEmitter` token exceeded max age → `UsageEmitterError::TokenExpired`; no outbox INSERT
- Metadata exceeds 8 KB → `UsageEmitterError::MetadataTooLarge`; no outbox INSERT
- Outbox delivery fails after retry budget exhausted → message moved to dead-letter store; surfaced via monitoring

**Steps**:
1. [x] - `p1` - Source retrieves `UsageEmitterV1` from `ClientHub` at module initialization - `inst-emit-1`
2. [x] - `p1` - Source calls `UsageEmitterV1::for_module(MODULE_NAME)` to obtain a `ScopedUsageEmitter` bound to the source module's identity - `inst-emit-2`
3. [x] - `p1` - Before opening a DB transaction, source calls `ScopedUsageEmitter::authorize_for(ctx, tenant_id, resource_id, resource_type)` — triggers phase 1 authorization - `inst-emit-3`
4. [x] - `p1` - **IF** PDP denies or module is not configured - `inst-emit-4`
   1. [x] - `p1` - **RETURN** `UsageEmitterError`; no record is persisted - `inst-emit-4a`
5. [x] - `p1` - **RETURN** `AuthorizedUsageEmitter` token carrying PDP permit, allowed-metrics list, and bound `tenant_id`/`resource_id`/`resource_type` - `inst-emit-5`
6. [x] - `p1` - Inside the source's DB transaction, source calls `AuthorizedUsageEmitter::build_usage_record(metric, value).enqueue()` — triggers phase 2 enqueue - `inst-emit-6`
7. [x] - `p1` - **IF** any in-memory validation fails (token expired, metric disallowed, counter invalid, metadata oversized) - `inst-emit-7`
   1. [x] - `p1` - **RETURN** `UsageEmitterError`; outbox INSERT is not executed - `inst-emit-7a`
8. [x] - `p1` - Outbox row is inserted into the source's local DB within the caller's transaction, serialized as `payload_type = "usage-collector.record.v1"` - `inst-emit-8`
9. [x] - `p1` - Outbox background pipeline picks up the row and calls `UsageCollectorClientV1::create_usage_record()` on delivery - `inst-emit-9`
10. [x] - `p1` - **IF** delivery fails transiently (network error, 5xx, 429) - `inst-emit-10`
    1. [x] - `p1` - Retry with exponential backoff; `backoff_max` MUST be configured below 15 minutes - `inst-emit-10a`
11. [x] - `p1` - **IF** delivery fails permanently (4xx excluding 429) - `inst-emit-11`
    1. [x] - `p1` - Move message to dead-letter store and surface via monitoring - `inst-emit-11a`
12. [x] - `p1` - **RETURN** delivery confirmed; record is available at the gateway - `inst-emit-12`

### Module Config Retrieval Flow

- [x] `p2` - **ID**: `cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config`

> _(p2: deferred — static module config reload requires gateway restart; implementing dynamic config reload is out of scope for Feature 1)_

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Success Scenarios**:
- Gateway returns `ModuleConfig` with the static `allowed_metrics` list for the requesting module

**Error Scenarios**:
- Module not registered in static config → gateway returns 404; `authorize_for()` surfaces `UsageEmitterError::ModuleNotConfigured`

**Steps**:
1. [x] - `p2` - During `authorize_for()` phase 1, emitter calls `UsageCollectorClientV1::get_module_config(module_name)` - `inst-cfg-1`
2. [x] - `p2` - Gateway receives `GET /usage-collector/v1/modules/{module_name}/config` authenticated via SecurityContext - `inst-cfg-2`
3. [x] - `p2` - Gateway looks up static metric configuration for the module - `inst-cfg-3`
4. [x] - `p2` - **IF** module not in static config - `inst-cfg-4`
   1. [x] - `p2` - **RETURN** 404; emitter surfaces `UsageEmitterError::ModuleNotConfigured` - `inst-cfg-4a`
5. [x] - `p2` - **RETURN** `ModuleConfig { module_name, allowed_metrics: [AllowedMetric { name, kind }] }` - `inst-cfg-5`

## 3. Processes / Business Logic (CDSL)

### Phase 1: `authorize_for()` Authorization

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for`

**Input**: `SecurityContext`, `tenant_id: Uuid`, `resource_id: Uuid`, `resource_type: String`

**Output**: `Result<AuthorizedUsageEmitter, UsageEmitterError>`

**Hot path**: `authorize_for()` (PDP call + config fetch) is the
latency-critical path for SDK callers. `get_module_config()` is called on
every `authorize_for()` invocation; it reads from static gateway
configuration loaded at startup — no per-call I/O, no TTL-based cache
needed. Batch delivery and N+1 query optimisation are not applicable —
records are enqueued individually by design.

**Steps**:
1. [x] - `p1` - Call platform PDP: `USAGE_RECORD`/`CREATE` for the given `tenant_id` and `resource_id`/`resource_type` - `inst-authz-2`
2. [x] - `p1` - **IF** PDP denies - `inst-authz-3`
   1. [x] - `p1` - **RETURN** `UsageEmitterError::AuthorizationDenied` - `inst-authz-3a`
3. [x] - `p1` - Call `get_module_config(module_name)` to fetch `AllowedMetric` list from gateway - `inst-authz-4`
4. [x] - `p1` - **IF** module not in static config - `inst-authz-5`
   1. [x] - `p1` - **RETURN** `UsageEmitterError::ModuleNotConfigured` - `inst-authz-5a`
5. [x] - `p1` - Bind PDP permit result, allowed-metrics list, `tenant_id`, `resource_id`, `resource_type`, and issuance timestamp into `AuthorizedUsageEmitter` token - `inst-authz-6`
6. [x] - `p1` - **RETURN** `AuthorizedUsageEmitter` token - `inst-authz-7`

### Phase 2: `build_usage_record().enqueue()` — In-Transaction Enqueue

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue`

**Input**: `AuthorizedUsageEmitter`, metric name, value, optional idempotency key, optional metadata JSON

**Output**: `Result<(), UsageEmitterError>`

**Optional field serialization**: `metadata` is optional. When absent it serializes as an absent JSON field (not `null`). Deserialization treats absent as `None` with no default substitution. `idempotency_key` is optional from the caller's perspective but is **always present in the serialized record** — when the caller omits it, a UUID v4 is auto-generated before enqueue so the wire format always carries a non-null key.

**Steps**:
1. [x] - `p1` - Verify `AuthorizedUsageEmitter` token has not exceeded its maximum age - `inst-enq-1`
2. [x] - `p1` - **IF** token is expired - `inst-enq-2`
   1. [x] - `p1` - **RETURN** `UsageEmitterError::TokenExpired` - `inst-enq-2a`
3. [x] - `p1` - Verify metric name is present in the token's allowed-metrics list - `inst-enq-3`
4. [x] - `p1` - **IF** metric not in allowed list - `inst-enq-4`
   1. [x] - `p1` - **RETURN** `UsageEmitterError::MetricNotAllowed` - `inst-enq-4a`
5. [x] - `p1` - **IF** metric kind is `counter` AND (value < 0 OR idempotency_key is None) - `inst-enq-5`
   1. [x] - `p1` - **RETURN** `UsageEmitterError::InvalidRecord` - `inst-enq-5a`
5a. [x] - `p1` - **IF** idempotency_key is None (gauge record without caller-supplied key) — generate a UUID v4 and assign it as the idempotency_key - `inst-enq-5b`
6. [x] - `p1` - Capture `subject_id` and `subject_type` from `SecurityContext` - `inst-enq-6`
7. [x] - `p1` - **IF** metadata is present AND byte length > 8192 - `inst-enq-7`
   1. [x] - `p1` - **RETURN** `UsageEmitterError::MetadataTooLarge` - `inst-enq-7a`
8. [x] - `p1` - Serialize `UsageRecord` (tenant_id, module, kind, metric, value, idempotency_key, resource_id, resource_type, subject_id, subject_type, metadata, timestamp) with `payload_type = "usage-collector.record.v1"` - `inst-enq-8`
9. [x] - `p1` - Call `Outbox::enqueue(payload, payload_type)` within the caller's active DB transaction - `inst-enq-9`
10. [x] - `p1` - **RETURN** `Ok(())`; record is durably enqueued and delivery proceeds asynchronously - `inst-enq-10`

### Outbox Delivery `MessageHandler`

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery`

**Input**: Serialized outbox message with `payload_type = "usage-collector.record.v1"`

**Output**: `HandlerResult`

**Queue overflow**: The outbox grows as long as the storage plugin is unavailable. No max-rows limit is enforced by this feature — unbounded growth is an operational concern delegated to DB capacity management. `enqueue()` does not apply backpressure to the caller; callers experience DB write latency only. Operators should monitor outbox queue depth (see Observability) and provision DB capacity accordingly.

**Message ordering**: Ordering across the 4 outbox partitions is not guaranteed. Per-partition ordering may be preserved by the `modkit-db` outbox library but is not relied upon by this feature. Idempotency keys on counter records provide at-least-once deduplication at the storage layer.

**Steps**:
1. [x] - `p1` - Deserialize outbox payload bytes into `UsageRecord` - `inst-dlv-1`
2. [x] - `p1` - **IF** deserialization fails - `inst-dlv-2`
   1. [x] - `p1` - **RETURN** `HandlerResult::Reject`; unrecoverable format error — message moved to dead-letter store - `inst-dlv-2a`
3. [x] - `p1` - Assemble gateway ingest request from `UsageRecord` fields - `inst-dlv-3`
4. [x] - `p1` - Call `UsageCollectorClientV1::create_usage_record(record)` - `inst-dlv-4`
5. [x] - `p1` - **IF** call succeeds (200 OK) - `inst-dlv-5`
   1. [x] - `p1` - **RETURN** `HandlerResult::Success`; outbox row is deleted - `inst-dlv-5a`
6. [x] - `p1` - **IF** transient failure (network error, 5xx, 429) - `inst-dlv-6`
   1. [x] - `p1` - **RETURN** `HandlerResult::Retry`; outbox library applies exponential backoff; `backoff_max` MUST be configured below 15 minutes to satisfy `cpt-cf-usage-collector-nfr-recovery` - `inst-dlv-6a`
7. [x] - `p1` - **IF** permanent failure (4xx excluding 429) - `inst-dlv-7`
   1. [x] - `p1` - **RETURN** `HandlerResult::Reject`; message moved to dead-letter store and surfaced via monitoring - `inst-dlv-7a`

### Gateway Ingest Handler

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler`

**Input**: `UsageRecord` delivered by the outbox pipeline, `SecurityContext`

**Output**: 200 OK or error response

**Steps**:
1. [x] - `p1` - Enforce metadata size limit: reject if `record.metadata` byte length > 8192 - `inst-gw-1`
2. [x] - `p1` - Check circuit breaker state for the active plugin instance - `inst-gw-2`
3. [x] - `p1` - **IF** circuit is open - `inst-gw-3`
   1. [x] - `p1` - **RETURN** `503 Service Unavailable` - `inst-gw-3a`
4. [x] - `p1` - Resolve the active storage plugin via GTS - `inst-gw-4`
5. [x] - `p1` - Call `plugin.create_usage_record(record)` with configurable timeout (default 5 s) - `inst-gw-5`
6. [x] - `p1` - **IF** plugin call times out or fails transiently - `inst-gw-6`
   1. [x] - `p1` - Record failure against circuit breaker; open circuit after 5 consecutive failures within a 10 s window; half-open probe after configurable interval (default 30 s) - `inst-gw-6a`
   2. [x] - `p1` - **RETURN** transient error; retry is handled by the outbox library on the SDK side - `inst-gw-6b`
7. [x] - `p1` - **RETURN** 200 OK on successful plugin confirmation - `inst-gw-7`

### Static Module Config Resolution

- [x] `p2` - **ID**: `cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config`

> _(p2: deferred — static module config reload requires gateway restart; implementing dynamic config reload is out of scope for Feature 1)_

**Input**: `module_name: String`, `SecurityContext`

**Output**: `Result<ModuleConfig, ModuleConfigError>`

**Cache integration**: Not applicable — `ModuleConfig` is loaded from static gateway
configuration at startup. No runtime caching layer is introduced in this feature.

**Steps**:
1. [x] - `p2` - Authenticate request via SecurityContext; ModKit pipeline rejects unauthenticated requests before the handler - `inst-cfg-p-1`
2. [x] - `p2` - Look up module name in the gateway's static `metrics` configuration - `inst-cfg-p-2`
3. [x] - `p2` - **IF** module not found in static config - `inst-cfg-p-3`
   1. [x] - `p2` - **RETURN** 404 Not Found - `inst-cfg-p-3a`
4. [x] - `p2` - **RETURN** `ModuleConfig { module_name, allowed_metrics }` - `inst-cfg-p-4`

## 4. States (CDSL)

Not applicable for this feature. `UsageRecord.status` transitions (`active` → `inactive`) are owned by Feature 8 (operator amendment and deactivation). Outbox message lifecycle is managed by the `modkit-db` outbox library and is not a domain state machine defined here.

## 5. Definitions of Done

### SDK Crate (`usage-collector-sdk`)

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-sdk-and-ingest-core-sdk-crate`

The system **MUST** implement the `usage-collector-sdk` crate providing: `UsageCollectorClientV1` delivery trait (`create_usage_record()`, `get_module_config()`), `UsageCollectorPluginClientV1` plugin trait (write and read operations), shared model types (`UsageRecord`, `ModuleConfig`, `AllowedMetric`, `UsageKind`, error types), and the GTS schema `UsageCollectorStoragePluginSpecV1` for storage plugin registration.

**Implements**:
- `cpt-cf-usage-collector-component-sdk`

**Constraints**: `cpt-cf-usage-collector-constraint-modkit`

**Touches**:
- Entities: `UsageRecord`, `ModuleConfig`, `AllowedMetric`, `UsageKind`

**Data Protection**: `UsageRecord` fields (`tenant_id`, `resource_id`, `resource_type`,
`subject_id`, `subject_type`) are classified as internal billing identifiers —
opaque UUIDs and numeric values — not PII under the project's data
classification policy. Data minimization: only fields required for billing
attribution are collected. Data subject deletion rights: not applicable at the
feature level; delegated to the storage plugin (Feature 4). Encryption at rest
and in transit: not enforced by this feature; delegated to the storage plugin
and its infrastructure (Feature 4 — Production Storage Plugin).

### Emitter Crate (`usage-emitter`)

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-sdk-and-ingest-core-emitter-crate`

The system **MUST** implement the `usage-emitter` crate providing: `UsageEmitterV1::for_module(name) -> ScopedUsageEmitter`, `ScopedUsageEmitter::authorize_for()` and `authorize()` (PDP call + module config fetch), `AuthorizedUsageEmitter::build_usage_record().enqueue()` (all in-memory validations + `Outbox::enqueue()` within caller's transaction), the outbox `MessageHandler` with `backoff_max` configured below 15 minutes, and registration of `UsageEmitterV1` in `ClientHub` during gateway `init()`.

**Implements**:
- `cpt-cf-usage-collector-flow-sdk-and-ingest-core-emit`
- `cpt-cf-usage-collector-algo-sdk-and-ingest-core-authorize-for`
- `cpt-cf-usage-collector-algo-sdk-and-ingest-core-enqueue`
- `cpt-cf-usage-collector-algo-sdk-and-ingest-core-outbox-delivery`
- `cpt-cf-usage-collector-component-emitter`

**Constraints**: `cpt-cf-usage-collector-constraint-outbox-infra`, `cpt-cf-usage-collector-constraint-security-context`, `cpt-cf-usage-collector-constraint-modkit`

**Touches**:
- DB: `outbox` (source's local DB, `cpt-cf-usage-collector-dbtable-outbox`)
- Entities: `UsageRecord`, `AuthorizedUsageEmitter`

**Audit logging**: Calls to `authorize_for()` are not individually audited at
the SDK boundary — audit of usage-collector ingestion calls is delegated to
the platform-wide API audit layer. Calls to `POST /usage-collector/v1/records`
are similarly delegated. No feature-level audit log is produced.

**Resource management**: `AuthorizedUsageEmitter` tokens are single-use and
dropped after `enqueue()` — no additional cleanup required. DB connection
pooling for the outbox is fully managed by the `modkit-db` outbox library;
this feature does not hold connections. `UsageCollectorClientV1` connection
pool is managed by the ModKit HTTP client; no feature-owned connection
lifecycle.

**Concurrency**: `authorize_for()` and `enqueue()` are safe for concurrent
calls — all state is per-call with no shared mutable state in the emitter.
Rate limiting on `POST /usage-collector/v1/records` is not in scope for
this feature; it is delegated to the platform API gateway.

**Observability**: Structured log events MUST be emitted for: authorization
denial (`WARN`), validation failure (`WARN`), delivery retry (`INFO`),
dead-letter routing (`ERROR`), circuit breaker state transitions
(`WARN`/`INFO`). Metrics: outbox queue depth, delivery attempt count,
plugin call latency (histogram), circuit breaker open/closed state (gauge).
OpenTelemetry trace propagation across the outbox pipeline boundary is
deferred to a future observability feature; correlation IDs from inbound
requests are not propagated in this feature.

**Data access patterns**: Not applicable — DB access is fully mediated by the `modkit-db` outbox library. This feature constructs no raw queries; index usage, join patterns, and aggregation patterns are encapsulated by the library.

**Data archival and retention**: Not applicable to this feature. Archival and retention compliance are delegated to the storage plugin implementation and its backing infrastructure. The outbox schema migration is forward-only via `DatabaseCapability::migrations()`; schema rollback is not supported.

**Connection management**: Not applicable — connection management, query parameterization, and
result handling are fully encapsulated by the `modkit-db` outbox library. This feature constructs
no raw queries.

### Gateway Crate (`usage-collector`) — Ingest & Config

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-sdk-and-ingest-core-gateway-crate`

The system **MUST** implement in the `usage-collector` gateway crate: outbox queue registration (`"usage-records"`, 4 partitions, configurable) and schema migrations via `DatabaseCapability::migrations()`, `POST /usage-collector/v1/records` ingest handler (metadata size enforcement, GTS plugin resolution with timeout, circuit breaker — 5 failures / 10 s open, 30 s half-open probe), `GET /usage-collector/v1/modules/{module_name}/config` handler (static metric config lookup), and construction + registration of `UsageEmitterV1` (backed by `UsageCollectorLocalClient`) in `ClientHub` during `init()`.

**Implements**:
- `cpt-cf-usage-collector-flow-sdk-and-ingest-core-fetch-module-config`
- `cpt-cf-usage-collector-algo-sdk-and-ingest-core-gateway-ingest-handler`
- `cpt-cf-usage-collector-algo-sdk-and-ingest-core-get-module-config`
- `cpt-cf-usage-collector-component-gateway`

**Constraints**: `cpt-cf-usage-collector-constraint-outbox-infra`, `cpt-cf-usage-collector-constraint-single-plugin`, `cpt-cf-usage-collector-constraint-modkit`, `cpt-cf-usage-collector-constraint-security-context`, `cpt-cf-usage-collector-constraint-no-business-logic`

**Touches**:
- API: `POST /usage-collector/v1/records`, `GET /usage-collector/v1/modules/{module_name}/config`
- DB: `outbox` (`"usage-records"` queue, `cpt-cf-usage-collector-dbtable-outbox`)
- Entities: `UsageRecord`, `ModuleConfig`

**Audit logging**: Calls to `authorize_for()` are not individually audited at
the SDK boundary — audit of usage-collector ingestion calls is delegated to
the platform-wide API audit layer. Calls to `POST /usage-collector/v1/records`
are similarly delegated. No feature-level audit log is produced.

**Security error handling**: The gateway strips internal stack traces before
returning 4xx/5xx responses to callers. `authorize_for()` timing: constant-time
response patterns are not applied at the SDK layer; tenant-existence enumeration
via PDP call timing is mitigated by the PDP's own response-time guarantees.
Rate limiting on `authorize_for()` calls is out of scope for this feature —
delegated to the platform gateway rate-limiting layer.

**Observability**: Structured log events MUST be emitted for: authorization
denial (`WARN`), validation failure (`WARN`), delivery retry (`INFO`),
dead-letter routing (`ERROR`), circuit breaker state transitions
(`WARN`/`INFO`). Metrics: outbox queue depth, delivery attempt count,
plugin call latency (histogram), circuit breaker open/closed state (gauge).
OpenTelemetry trace propagation across the outbox pipeline boundary is
deferred to a future observability feature; correlation IDs from inbound
requests are not propagated in this feature.

**Rollout/rollback**: The outbox schema migration
(`DatabaseCapability::migrations()`) is forward-only — rollback of the
schema is not supported. Rollback of the gateway binary to a pre-feature
version is safe only if no messages have been enqueued; enqueued rows will
remain in the DB until the migrated gateway is redeployed. No feature flag
guards the new endpoints in this feature — rollout strategy is managed at
the platform level via standard deployment controls.

**Recovery**: In-flight outbox messages survive a gateway upgrade — rows are durable in the DB and will be picked up by the restarted process. Circuit breaker state and plugin registration are recovered automatically on gateway restart (stateless configuration). Dead-lettered records: operators inspect via direct DB query on the dead-letter partition; reprocessing requires manual row deletion and re-insertion into the live queue, or a future admin API (out of scope). No compensating transaction is required for the delivery pipeline.

**Data access patterns**: Not applicable — all storage I/O is delegated to the active storage plugin via `UsageCollectorPluginClientV1`. The gateway constructs no raw DB queries; plugin selection, connection pooling, and query execution are fully encapsulated by the plugin implementation.

**Data archival and retention**: Not applicable to this feature. Archival and retention compliance are delegated to the storage plugin implementation and its backing infrastructure. The outbox schema migration is forward-only via `DatabaseCapability::migrations()`; schema rollback is not supported.

**Configuration**:

| Parameter | Type | Valid range | Default | Validation | Runtime-changeable |
|-----------|------|-------------|---------|------------|--------------------|
| `backoff_max` | duration | 1s–15m | 600s (10 min) | must be > 0 and < 900s | No — requires restart |
| Plugin timeout | duration | 100ms–30s | 5s | must be > 0 | No |
| Circuit breaker failure threshold | integer | 1–100 | 5 | must be ≥ 1 | No |
| Circuit breaker recovery timeout | duration | 1s–5m | 30s | must be > 0 | No |
| Queue partitions | integer | 1–64 | 4 | must be ≥ 1 | No |

No feature flags are used; all configuration is static and requires gateway restart to change.

**Health & diagnostics**: Circuit breaker state is not directly exposed via `GET /health` in this feature — it contributes to the platform-level health aggregate. Outbox queue depth is a recommended monitoring metric (see Observability). First-level troubleshooting: (1) check circuit breaker state via structured logs; (2) inspect outbox queue depth; (3) verify storage plugin connectivity; (4) check dead-letter partition for accumulated records.

`cpt-cf-usage-collector-constraint-encryption` is not enforced by this feature — encryption at
rest and in transit is deferred to Feature 4 (Production Storage Plugin) which owns the
production storage backend.

### No-Op Storage Plugin (`noop-usage-collector-storage-plugin`)

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-sdk-and-ingest-core-noop-plugin`

> _(p2: deferred — noop plugin validation is a test-time concern; plugin interface is validated by integration tests)_

The system **MUST** implement the `noop-usage-collector-storage-plugin` crate providing a no-op implementation of `UsageCollectorPluginClientV1` where all write operations succeed without persisting data and all read operations return empty results. Must register via `UsageCollectorStoragePluginSpecV1` GTS schema for selection by operator configuration in test and local-dev deployments.

**Implements**:
- `cpt-cf-usage-collector-component-storage-plugin` (no-op only)

**Constraints**: `cpt-cf-usage-collector-constraint-single-plugin`, `cpt-cf-usage-collector-constraint-modkit`

**Touches**:
- Entities: `UsageRecord`

### Known Limitations / Technical Debt

- **Static `ModuleConfig`**: Has no dynamic update mechanism — changes require a gateway restart.
- **Outbox payload versioning**: Payload type `usage-collector.record.v1` uses an opaque string
  version with no schema registry or backward-compatibility contract defined in this feature.
  Payload versioning strategy MUST be documented before Feature 2+ extends the record schema.

## 6. Acceptance Criteria

- [ ] A usage source can call `authorize_for()` and receive an `AuthorizedUsageEmitter` token when the PDP permits `USAGE_RECORD`/`CREATE` for the given tenant and resource
- [ ] `enqueue()` persists a usage record to the source's local outbox within the caller's DB transaction; the transaction commit is the durability boundary for the record
- [ ] Counter records without an idempotency key or with a negative value are rejected before the outbox INSERT; no outbox row is created
- [ ] Gauge records without a caller-supplied idempotency key are accepted; the emitter auto-generates a UUID v4 and the stored record always carries a non-null key
- [ ] Records with a metric name not in the module's allowed-metrics list are rejected by `enqueue()` in-memory before the outbox INSERT
- [ ] PDP denial in `authorize_for()` surfaces as `UsageEmitterError::AuthorizationDenied` with no record persisted
- [ ] The outbox delivery pipeline delivers records to the gateway with at-least-once semantics; transient failures trigger exponential backoff retry with `backoff_max` configured below 15 minutes
- [ ] The gateway ingest endpoint enforces the 8 KB metadata limit, resolves the active plugin via GTS, and delegates record persistence with a 5 s default timeout
- [ ] The circuit breaker opens after 5 consecutive plugin call failures within a 10 s window; the gateway returns `503 Service Unavailable` while open and probes after 30 s
- [ ] `GET /usage-collector/v1/modules/{name}/config` returns the static allowed-metrics list for a configured module and 404 for an unknown module
- [ ] The no-op plugin accepts all write calls with no side effects and returns empty results for reads; integration tests pass with the no-op backend selected
- [ ] `UsageEmitterV1` is available in `ClientHub` after gateway `init()` completes; sources can call `for_module()` without additional setup

**Test data requirements**:
(1) Static gateway config must include at least one module with a `counter` metric and one with a
    `gauge` metric.
(2) PDP stub must support permit/deny configuration for `USAGE_RECORD`/`CREATE` actions by
    `tenant_id`/`resource_id` pair.
(3) Integration tests use `noop-usage-collector-storage-plugin`.
(4) Idempotency collision test: submit two records with the same `idempotency_key` for the same
    metric and verify deduplication.

**Test coverage guidance**:
Unit: `authorize_for()` — PDP permit, PDP deny, token expiry, metric type mismatch;
      `enqueue()` — each validation branch.
Integration: full emission flow with noop plugin; circuit breaker open/close cycle;
             dead-letter routing after max retries.
E2E: single record visible in noop plugin store after transaction commit.
Performance baseline: measure `authorize_for()` + `enqueue()` round-trip latency against
`nfr-ingestion-latency` target using noop backend.

**Success metrics**:
(1) At-least-once delivery rate ≥ 99.9% under normal conditions within `backoff_max` window.
(2) Circuit breaker recovers within 30 s of storage plugin recovery.
(3) Noop plugin integration test pass rate: 100% on CI.

## 7. Non-Applicability Notes

**COMPL (Regulatory & Privacy Compliance)**: Not applicable. This feature
processes opaque UUIDs (`tenant_id`, `resource_id`, `subject_id`) and numeric
counters/gauges for internal billing metrics. No regulated personal data is
defined at the feature level. No audit trail, consent, data retention, or data
subject rights apply to this in-process SDK. If future features extend this to
personal data, COMPL must be revisited.

**UX (User Experience & Accessibility)**: Not applicable. This feature provides
an in-process SDK library (`usage-collector-sdk`, `usage-emitter`) and a gateway
service with machine-to-machine REST endpoints. There is no user-facing UI, no
end-user interaction model, no user-visible error messages, and no accessibility
requirements.
