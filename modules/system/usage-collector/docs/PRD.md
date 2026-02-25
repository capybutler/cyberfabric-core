# PRD — Usage Collector

## 1. Overview

### 1.1 Purpose

A centralized usage metering system for reliably collecting, persisting, and managing usage data from all platform sources, with exactly-once semantics, tenant isolation, and type-safe usage definitions. The Usage Collector acts as the authoritative record of resource consumption. Business logic (pricing, rating, billing rules, invoice generation) remains the responsibility of downstream systems. Querying and server-side aggregation are handled by the usage-query module.

### 1.2 Background / Problem Statement

The Usage Collector (UC) serves as the single source of truth for all platform usage data. UC handles usage capture, storage, and data integrity; querying, aggregation, and data exposure are handled by the usage-query module. UC supports diverse collection patterns optimized for different throughput needs, pluggable storage backends, and provides type-safe usage definitions through a schema-based type system.

The service addresses the fragmentation problem where different consumers (billing, monitoring, quota enforcement) implement their own collection logic, leading to inconsistent data and duplicated effort. By centralizing usage collection, the platform ensures that all consumers operate on the same accurate, deduplicated data.

Key problems:

- **Fragmented tracking**: Each consumer implements own collection leading to inconsistent data
- **High-volume ingestion**: Per-event synchronous calls are inefficient at high throughput due to protocol overhead and blocking behavior
- **No custom units**: Cannot meter new resource types (AI tokens) without code changes
- **Storage lock-in**: No flexibility for different retention and performance needs

### 1.3 Goals (Business Outcomes)

- All billable platform services emit usage through UC
- Single source of truth: all downstream consumers (billing, monitoring, quota enforcement) operate on the same usage data from UC
- Custom unit registration in less than 5 minutes without code changes
- High-volume services can emit 10,000+ events per second without blocking
- Storage backends are pluggable — no lock-in to a specific database technology
- 99.95%+ monthly availability

### 1.4 Glossary

| Term | Definition |
|------|------------|
| Usage Record | A single data point representing resource consumption by a tenant |
| Counter | A delta metric representing a non-negative increment since the last report (e.g., API calls in this batch). The UC accumulates deltas into a monotonically increasing persistent total. |
| Gauge | A point-in-time metric that can go up or down (e.g., current memory usage) |
| Measuring Unit | A registered schema defining how a usage type is measured (e.g., "ai-credits", "vCPU-hours") |
| Usage Collector Plugin | A plugin that provides backend-specific data persistence for a specific storage backend (e.g., ClickHouse, TimescaleDB). Each plugin implements the full write-path interface for its backend: record persistence, deduplication storage, retention enforcement, and failure buffering. |
| Idempotency Key | A client-provided identifier ensuring exactly-once processing of a usage record |
| Backfill | The process of retroactively submitting historical usage data to fill gaps caused by outages, pipeline failures, or corrections |
| Grace Period | A configurable time window during which late-arriving events are accepted via normal ingestion without requiring explicit backfill |
| Reconciliation | The process of comparing usage data across pipeline stages or external sources to detect gaps and inconsistencies (performed by external systems; UC exposes metadata to support this) |
| Amendment | A correction to previously recorded usage data, either by replacing events in a time range or deprecating individual events |
| Rate Limit | A constraint on the volume of requests a source can submit within a time window |
| Load Shedding | The deliberate dropping or deferral of low-priority work to preserve system stability under overload |
| Record Metadata | An optional, extensible JSON object attached to a usage record, allowing usage sources to include context-specific properties (e.g., LLM model name, token type, geographic region) that are opaque to UC and interpreted by downstream consumers |

## 2. Actors

### 2.1 Human Actors

#### Platform Operator

**ID**: `cpt-cf-usage-collector-actor-platform-operator`

**Role**: Configures storage plugins, retention policies, custom measuring units, and monitors system health.
**Needs**: Ability to manage storage backend plugins, define retention policies, register custom units, and monitor system health without code changes.

#### Platform Developer

**ID**: `cpt-cf-usage-collector-actor-platform-developer`

**Role**: Integrates services with UC using SDKs or APIs to emit usage data.
**Needs**: Well-documented SDKs and APIs for emitting usage data with minimal integration effort.

### 2.2 System Actors

#### Usage Source

**ID**: `cpt-cf-usage-collector-actor-usage-source`

**Role**: Any platform service, infrastructure adapter, or gateway that emits usage records (e.g., LLM Gateway, Compute Service, API Gateway).

#### Monitoring System

**ID**: `cpt-cf-usage-collector-actor-monitoring-system`

**Role**: Consumes usage metrics for dashboards, alerting, and operational visibility.

#### Types Registry

**ID**: `cpt-cf-usage-collector-actor-types-registry`

**Role**: Provides schema validation for usage types and custom measuring units.

#### Storage Backend

**ID**: `cpt-cf-usage-collector-actor-storage-backend`

**Role**: The underlying data store (ClickHouse, TimescaleDB, or external system) that persists usage records. Accessed by the usage collector plugin for write operations.

## 3. Operational Concept & Environment

No module-specific environment constraints beyond project defaults.

## 4. Scope

### 4.1 In Scope

- Client-side SDK with batching (primary ingestion path)
- API for usage ingestion (Rust API, gRPC, HTTP)
- Counter and gauge metric semantics
- Per-tenant, per-subject (user, service account, etc.), and per-resource usage attribution
- Pluggable storage plugin framework (ClickHouse, TimescaleDB, custom)
- Custom measuring unit registration via API
- Configurable retention policies
- Idempotency and deduplication for exactly-once semantics
- Backfill API for retroactive submission of historical usage data
- Late-arriving event handling with configurable grace period
- Per-tenant and per-source ingestion rate limiting with configurable overrides
- Priority-based load shedding under sustained overload

### 4.2 Out of Scope

- **Querying & Aggregation**: Raw record querying, server-side aggregation, cursor-based pagination, rollups, and query engine management are handled by the usage-query module.
- **Business Aggregation & Interpretation**: Business-specific data interpretation — pricing, rating, billing rules, invoice generation, chargeback allocation, quota policy decisions — is the responsibility of downstream consumers.
- **Reconciliation & Gap Detection**: Monitoring for data gaps, heartbeat tracking, watermark analysis, and cross-stage count reconciliation are handled by external observability/reconciliation systems. UC exposes metadata (event counts, timestamps per source) that external systems can consume for this purpose.
- **Report Generation**: Usage reports, dashboards, and visualizations are handled by Monitoring/Analytics systems.
- **Rules & Exceptions**: Business rules, usage policies, threshold-based actions, and exception handling are the responsibility of downstream consumers.
- **Billing/Rating Logic**: Pricing calculation handled by downstream Billing System.
- **Invoice Generation**: Handled by Billing System.
- **Quota Enforcement Decisions**: Handled by Quota Enforcement System; UC provides data only.
- **Usage Prediction/Forecasting**: Deferred to future phase.
- **Multi-Region Replication**: Deferred to future phase.

## 5. Functional Requirements

### 5.1 Usage Ingestion

#### Usage Record Ingestion

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-usage-ingestion`

The system **MUST** accept usage records via multiple transport mechanisms (Rust API, gRPC, HTTP), with SDK providing automatic batching for high-throughput scenarios (10,000+ events per second).

**Rationale**: Different usage sources have different integration and throughput needs; providing multiple transport options ensures all sources can efficiently emit data.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`

#### Idempotency and Deduplication

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-idempotency`

The system **MUST** support idempotency keys to ensure exactly-once processing, preventing duplicate records and incorrect aggregations.

**Rationale**: Network retries and batching can produce duplicate submissions; deduplication ensures billing accuracy.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Per-Record Extensible Metadata

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-record-metadata`

The system **MUST** support an optional, extensible metadata field on each usage record, allowing usage sources to attach arbitrary key-value properties as a JSON object. The system **MUST** persist metadata as-is and return it in query results without interpretation. The system **MUST** enforce a configurable maximum size limit on the metadata field (default 8 KB per record) and **MUST** reject records exceeding the limit with an actionable error.

The system **MUST NOT** index, aggregate, or interpret metadata contents — metadata is opaque to the Usage Collector. Downstream consumers (billing, reporting, analytics) are responsible for extracting and processing metadata fields according to their own domain logic.

**Rationale**: Different usage sources need to attach context-specific properties to usage records (e.g., LLM model name, token type, request category, geographic region) that enable downstream reporting and analytics. Storing metadata per-record at ingestion time avoids the need to correlate usage records with external context stores and supports use cases like detailed LLM usage reporting and cost attribution by model. This follows the industry-standard pattern used by metering platforms (OpenMeter, Lago, Orb, Amberflo) where per-event properties are stored alongside the core measurement.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`

### 5.2 Metric Semantics

#### Counter Metric Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-counter-semantics`

The system **MUST** enforce counter semantics: sources submit non-negative delta values representing consumption since their last report. The system **MUST** reject counter records with negative values. The system **MUST** accumulate submitted deltas into a persistent monotonically increasing total per (tenant, resource, usage_type) tuple.

**Rationale**: Delta-based reporting decouples the source's internal state from the UC's persistent totals. Sources never report cumulative values, so process restarts and counter resets in the source are transparent to the UC — a restart simply results in the next batch starting from zero again, which is valid. This avoids the need for counter reset signaling.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Gauge Metric Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-gauge-semantics`

The system **MUST** support gauge metrics (point-in-time values) without monotonicity validation, storing values as-is.

**Rationale**: Gauges represent instantaneous measurements (e.g., current memory usage) that naturally fluctuate.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`

### 5.3 Attribution & Isolation

#### Tenant Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-tenant-attribution`

The system **MUST** attribute all usage records to a tenant derived from security context, ensuring attribution is immutable and used for isolation.

**Rationale**: Accurate tenant attribution is the foundation for billing, quota enforcement, and data isolation.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Resource Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-resource-attribution`

The system **MUST** support attributing usage to specific resource instances within a tenant, including resource ID, type, and lineage.

**Rationale**: Granular resource attribution enables per-resource billing and usage analysis.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Subject Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-subject-attribution`

The system **MUST** support attributing usage to a subject within a tenant, identified by a `subject_id` and `subject_type` pair. Subject types include but are not limited to users and service accounts. Subject attribution **MUST** always be derived from the authenticated SecurityContext — the system **MUST NOT** accept subject identity from request payloads.

Subject attribution is optional on a per-usage-record basis to accommodate system-level resource consumption that is not attributable to a specific subject (e.g., background jobs authenticating as a service account where no per-user attribution is meaningful).

**Rationale**: Per-subject attribution enables chargeback, detailed usage analytics, per-subject quota enforcement, and helps organizations understand which subjects (users, service accounts) are driving consumption. This is essential for multi-user tenants who need to allocate costs or enforce limits at the subject level, and for tracking consumption by automated service accounts alongside human users.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Tenant Isolation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-tenant-isolation`

The system **MUST** enforce strict tenant isolation ensuring usage data is never accessible across tenants, failing closed on authorization failures.

**Rationale**: Tenant data isolation is a security and compliance requirement.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-platform-developer`, `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-types-registry`, `cpt-cf-usage-collector-actor-storage-backend`

#### Ingestion Authorization

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-ingestion-authorization`

The system **MUST** identify the source of each usage record from the caller's SecurityContext using `subject_id` and `subject_type`. Source identity **MUST** be derived server-side from the authenticated SecurityContext and **MUST NOT** be accepted from the request payload. The system **MUST** authorize each ingestion request by verifying that the authenticated caller is permitted to report the specific usage type being submitted and that the referenced resource and subject are within the caller's SecurityContext scope. The system **MUST** reject usage records that fail either check, failing closed on authorization failures.

Both dimensions — which usage types a source may report, and on whose behalf it may attribute usage — **MUST** be enforced through the platform's authorization mechanisms rather than a separate registry-based validation.

**Rationale**: Without ingestion authorization, any module or integration could report usage for resource types it does not own (e.g., a File Parser reporting LLM token usage) or attribute usage to subjects and resources beyond its organizational reach, leading to inaccurate metering, incorrect billing, quota manipulation, and cross-boundary data pollution.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-operator`

### 5.4 Storage & Retention

#### Data Persistence Guarantees and Boundaries

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-persistence-guarantees`

The system does **NOT** guarantee absolute persistence of all usage events under all conditions. The system provides limited persistence guarantees with explicit boundaries between guaranteed durability (post-ingestion) and acceptable data loss (pre-ingestion, overload scenarios).

**Persistence guarantee applies only AFTER successful ingestion acknowledgment** (`cpt-cf-usage-collector-nfr-exactly-once`, `cpt-cf-usage-collector-nfr-fault-tolerance`). **Before acknowledgment**, data loss is possible and acceptable under SDK buffer exhaustion, process termination, rate limiting, and priority-based load shedding (`cpt-cf-usage-collector-fr-sdk-retry`, `cpt-cf-usage-collector-fr-load-shedding`). All loss scenarios **MUST** be observable via metrics and alerts.

**Design Trade-off:** This design prioritizes system availability and ingestion performance over absolute durability. The Usage Collector remains available and responsive under overload conditions, accepting billing-critical usage data while shedding lower-priority events. The alternative—blocking all ingestion until buffers clear—would cause cascading failures in usage sources and block revenue-critical operations.

**Rationale**: Explicit acknowledgment of persistence boundaries prevents false assumptions about data durability and sets correct expectations for operators and downstream consumers.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`

#### Pluggable Storage Plugin Framework

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-pluggable-storage`

The system **MUST** support pluggable storage backends (ClickHouse, TimescaleDB, custom) via backend plugins. Each plugin implements the full write-path interface for its backend. This follows the platform's gateway pattern where the Usage Collector gateway delegates to the active plugin discovered via the types registry.

**Rationale**: Pluggable storage avoids lock-in to a specific database technology and allows operators to choose the backend that best fits their retention and performance needs.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-storage-backend`

#### Retention Policy Management

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-retention-policies`

The system **MUST** support configurable retention policies (global, per-tenant, per-usage-type) with automated enforcement.

**Rationale**: Retention policies balance storage costs with compliance and operational needs.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Storage Health Monitoring

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-storage-health`

The system **MUST** monitor storage plugin health, buffer records during failures, retry with backoff, and alert on persistent issues.

**Rationale**: Storage failures must not result in data loss; buffering and retry ensure durability.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-storage-backend`

### 5.5 Backfill & Amendment

#### Late-Arriving Event Handling

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-late-events`

The system **MUST** accept usage events with timestamps within a configurable grace period (default 24 hours, configurable per tenant and per usage type) via the standard ingestion path, applying normal deduplication and schema validation.

**Rationale**: In distributed systems, clock skew, batch processing delays, and asynchronous architectures cause events to routinely arrive after their actual timestamp. A grace period allows these events to be processed without requiring explicit backfill operations.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-operator`

#### Backfill API

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-backfill-api`

The system **MUST** provide a backfill API that allows operators to retroactively submit historical usage data for a specific time range (scoped to a single tenant and usage type). The backfill operation archives all events that exist in the range at the time the operation begins, then persists the provided replacement events. The operation **MUST** be transactionally atomic: if the persist step fails after archiving, the operation **MUST** be rolled back so that no partial change is visible to consumers.

Real-time ingestion **MUST** continue uninterrupted during a backfill operation. Real-time events that arrive in the target range during or after the backfill are recorded normally and are not affected by the backfill. Backfill and real-time ingestion operate independently on the same range.

The backfill API **MUST** be isolated from the real-time ingestion path with independent rate limits and lower processing priority to prevent backfill operations from degrading real-time ingestion performance.

**Rationale**: When usage data is lost due to outages, pipeline failures, or misconfigured sources, operators need a mechanism to retroactively submit corrected data for an entire time range. Keeping backfill and real-time ingestion independent avoids any interruption to ongoing usage reporting and eliminates the need for conflict resolution logic — the backfill corrects what existed at correction time, and the system continues recording new events without interference.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-usage-source`

#### Individual Event Amendment

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-event-amendment`

The system **MUST** support amending individual usage events (updating properties except tenant ID and timestamp) and deprecating individual events (marking them as inactive while retaining them for audit). Downstream consumers **MUST** be able to distinguish active from deprecated records when querying.

**Interaction with backfill**: If a backfill operation targets a time range that contains previously amended events, the backfill **MUST** archive the amended events along with all other events in that range. Amendment history is not preserved — the backfill's replacement events become the sole active record. This means backfill unconditionally supersedes any prior amendments within its range, keeping the correction model simple: amendments are for surgical fixes to individual events, while backfill is a wholesale replacement that starts from a clean slate.

**Rationale**: Not all corrections require full timeframe backfill. Individual event amendments handle cases like incorrect resource attribution or value errors on specific events.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Backfill Time Boundaries

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-fr-backfill-boundaries`

The system **MUST** enforce configurable time boundaries for backfill operations: a maximum backfill window (default 90 days) beyond which backfill requests are rejected, and a future timestamp tolerance (default 5 minutes) to account for clock drift. Backfill requests exceeding the maximum window **MUST** require elevated authorization.

**Rationale**: Unbounded backfill creates risks for data integrity and billing accuracy. Time boundaries constrain the blast radius of backfill operations while allowing legitimate corrections. Different limits for automated retry (grace period) vs. operator-initiated backfill (90 days) match different use cases. The 5-minute future tolerance follows Stripe's pattern for handling clock drift in distributed systems.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Backfill Event Archival

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-backfill-archival`

When a backfill operation replaces events in a time range, the system **MUST** archive (not delete) the replaced events. Archived events **MUST** remain queryable for audit purposes via the usage-query module (`cpt-cf-usage-query-fr-query-api`) but **MUST** be clearly distinguishable from active records so that downstream consumers can exclude them from their processing.

**Rationale**: Permanent deletion of replaced events destroys audit trail and makes it impossible to investigate billing disputes or reconstruct historical state.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Backfill Audit Trail

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-backfill-audit`

Every backfill operation **MUST** produce an immutable audit record containing: operator identity, initiation timestamp, affected time range, affected tenant ID (backfills are scoped to a single tenant), number of events added/replaced/deprecated, reason or justification, and whether the operation affected an already-invoiced period.

**Rationale**: Backfill operations are high-risk changes to billing-critical data. Comprehensive audit records are essential for dispute resolution, compliance, and operational visibility.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Metadata Exposure

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-fr-metadata-exposure`

The system **MUST** expose per-source and per-tenant metadata — including event counts, latest event timestamps (watermarks), and ingestion statistics — via API, enabling external reconciliation and observability systems to detect gaps and perform integrity checks.

**Rationale**: While reconciliation logic is out of scope for UC, exposing the raw metadata needed for gap detection enables external systems to build reconciliation workflows. This keeps UC focused while not blocking operational integrity monitoring.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-monitoring-system`

### 5.6 Type System

#### Usage Type Validation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-type-validation`

The system **MUST** validate all usage records against registered type schemas, rejecting invalid records with actionable error messages.

**Rationale**: Schema validation prevents corrupt or malformed data from entering the system.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-types-registry`

#### Custom Unit Registration

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-custom-units`

The system **MUST** allow registration of custom measuring units via API without code changes.

Primary use cases: AI/LLM token metering (input/output tokens, custom credit units), compute metering (vCPU-hours, memory-GB-hours, GPU-hours), API request metering (calls by tenant and endpoint), storage metering (GB-hours across tiers), network transfer (bytes ingress/egress).

When a custom measuring unit is registered, the platform operator **MUST** also configure the authorization policies that declare which sources are permitted to emit records of this type (`cpt-cf-usage-collector-fr-ingestion-authorization`).

**Rationale**: New resource types (AI tokens, GPU-hours) must be meterable without service redeployment.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

### 5.7 Rate Limiting

#### Per-Tenant Ingestion Rate Limiting

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-tenant-rate-limit`

The system **MUST** enforce per-tenant ingestion rate limits with independently configurable sustained rate (events per second) and burst size parameters. Requests exceeding the rate limit **MUST** be rejected with an appropriate rate limit error.

**Rationale**: Without per-tenant rate limiting, a single misbehaving or high-volume tenant can exhaust ingestion capacity and degrade service for all other tenants. Burst tolerance is required because usage event emission is inherently bursty (e.g., a batch job completing and emitting thousands of records at once).
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-operator`

#### Per-Source Rate Limiting

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-source-rate-limit`

The system **MUST** enforce per-source rate limits within each tenant, keyed by the `(subject_id, subject_type)` source identity derived from SecurityContext — the same identity used for ingestion authorization (`cpt-cf-usage-collector-fr-ingestion-authorization`). This prevents a single usage source (e.g., a misconfigured LLM Gateway) from consuming the tenant's entire ingestion quota. Per-source limits **MUST** be configurable independently of the tenant-level limit.

**Rationale**: Tenant-level rate limits alone do not prevent a single noisy source from starving other sources within the same tenant. Per-source limits provide fault isolation within a tenant's service portfolio.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-operator`

#### Multi-Dimensional Rate Limits

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-fr-multi-dimensional-limits`

The system **MUST** enforce rate limits across multiple dimensions simultaneously: events per second, bytes per second, maximum batch size (events per request), and maximum record size (bytes per event). All dimensions **MUST** pass for a request to be accepted.

**Rationale**: Single-dimension rate limits are insufficient; a tenant could comply with events/sec limits while submitting oversized payloads that exhaust network or storage bandwidth. Multi-dimensional limits protect all resource types (CPU for event processing, network for payload transfer, storage for persistence).
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-operator`

#### Rate Limit Configuration and Overrides

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-rate-limit-config`

The system **MUST** support rate limit configuration with system-wide defaults and per-tenant overrides. Per-tenant overrides **MUST** be hot-reloadable without service restart. Unspecified fields in overrides **MUST** inherit from the system defaults.

**Rationale**: Different tenants have different throughput needs based on their workload profile. Hot-reloadable overrides enable operators to respond to capacity issues or tenant requests without service disruption.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Rate Limit Response Format

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-rate-limit-response`

When rate limits are exceeded, the system **MUST** respond with a rate limit error and include metadata indicating when the client can retry. The system **MUST** provide rate limit status information on all API responses to enable clients to monitor their quota consumption. Rate limit error responses **MUST** communicate:

- The recommended delay before the client should retry.
- The total request allowance for the current rate limit window.
- The remaining budget within the current window.
- The time at which the current window resets.

This information **MUST** be available across all supported transports (HTTP, gRPC, native Rust API). Clients **MUST** treat missing or unparseable values conservatively (i.e., assume the limit is exhausted and apply a default backoff). The specific field names, header names, status codes, and encoding formats are defined in the API contract documentation.

**Rationale**: Rate limit metadata enables clients to track quota consumption and schedule retries, reducing wasted requests against an already-exhausted quota. Making this information transport-agnostic ensures clients can integrate regardless of protocol.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`

#### SDK Retry and Buffering on Rate Limit

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-sdk-retry`

The SDK **MUST** buffer usage events in a bounded in-memory queue and retry with exponential backoff and jitter on rate limit errors and backfill conflict errors, honoring the retry delay communicated in the rate limit response (`cpt-cf-usage-collector-fr-rate-limit-response`) when present. Under buffer exhaustion or sustained overload, the SDK will drop oldest events and **MUST** emit observable loss signals (event drop counts, drop rate, buffer utilization) tagged with sufficient context (usage type, tenant, drop reason) and **MUST** trigger alerts when drop rate exceeds configurable thresholds. The SDK **MUST NOT** block the calling service due to rate limiting.

**Loss Conditions**: Data loss at the SDK level is acceptable under the following conditions:
- Buffer exhaustion: The in-memory queue reaches capacity and new events arrive before older events can be successfully retried
- Sustained overload: Rate limiting persists beyond the SDK's retry window and buffer capacity
- Process termination: The SDK process crashes or is terminated before buffered events are flushed

When loss occurs, the SDK **MUST** expose observable loss signals (drop count, drop rate, and buffer utilization), tagged with sufficient context to identify the affected usage type, tenant, and cause. Operators **MUST** configure alerts on sustained drops (e.g., >1% drop rate over 5 minutes). The specific metric names and tag keys are defined in the DESIGN documentation.

**Rationale**: Usage sources generate events regardless of collector availability. The SDK must absorb temporary rate limiting transparently, retrying without burdening the caller. Exponential backoff with jitter prevents synchronized retry bursts across sources. Non-blocking behavior is critical because usage emission must not degrade the source service's primary function. Explicit loss conditions with observability enable operators to detect and remediate issues (e.g., increase rate limits, scale collectors, reduce event volume) rather than silently losing data.
**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`, `cpt-cf-usage-collector-actor-platform-operator`

#### Priority-Based Load Shedding

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-load-shedding`

The system **MUST** support priority classification of usage event types (e.g., billing-critical counters vs. analytics metrics) and, when operating under sustained overload beyond rate limits, **MUST** preferentially accept higher-priority events while shedding lower-priority ones. The priority classification **MUST** be configurable per usage type.

**Rationale**: Under extreme load, indiscriminate rejection causes billing-critical data loss. Priority-based load shedding ensures that the most business-critical usage data (which affects revenue accuracy) is preserved even when the system cannot accept all traffic.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-usage-source`

#### Rate Limit Observability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-rate-limit-observability`

The system **MUST** expose per-tenant and per-source rate limit consumption as metrics (current usage vs. limit, rejection counts, throttle duration) for operator dashboards. The system **MUST** emit alerts when tenants approach configured warning thresholds (e.g., 75%, 90% of capacity).

**Rationale**: Operators need visibility into rate limit utilization to proactively adjust limits before tenants experience rejections. Approaching-limit alerts enable capacity planning.
**Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-monitoring-system`

## 6. Non-Functional Requirements

### 6.1 Module-Specific NFRs

#### High Availability

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-availability`

The system **MUST** maintain 99.95% monthly availability for usage collection endpoints.

**Threshold**: 99.95% uptime per calendar month
**Rationale**: Usage collection is on the critical path for all billable operations.

#### Ingestion Throughput

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-throughput`

The system **MUST** support sustained ingestion of at least 600,000 usage records per minute (10,000 events per second) under normal operation.

**Threshold**: >= 600,000 records/minute (10,000 events/sec) sustained
**Rationale**: High-volume services (LLM Gateway, API Gateway) generate significant event throughput.

#### Ingestion Latency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-ingestion-latency`

The system **MUST** complete usage record ingestion within 200ms (p95) under normal load.

**Threshold**: p95 <= 200ms
**Rationale**: Low ingestion latency prevents blocking in usage source services.

#### Query/Retention Isolation

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-query-isolation`

The system **MUST** ensure that retention enforcement jobs do not degrade ingestion latency. Retention workloads **MUST** be isolated from the ingestion path such that retention job execution maintains ingestion p95 latency within the `cpt-cf-usage-collector-nfr-ingestion-latency` threshold (200ms). Query operations are served by the usage-query module.

**Threshold**: Ingestion p95 latency remains ≤200ms during concurrent retention operations
**Rationale**: Retention is a batch-processing workload that can compete for storage resources with the latency-sensitive ingestion path. Without isolation, retention cleanup could degrade ingestion performance, blocking usage sources.

#### Exactly-Once Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-exactly-once`

The system **MUST** guarantee exactly-once processing for all usage records successfully ingested by the API; zero usage records lost or duplicated after ingestion under normal operation. A record is considered **successfully ingested** only once the collector has **durably persisted** the record to storage **and** returned an acknowledgment (HTTP 2xx or gRPC OK) to the SDK. If the collector acknowledges a batch before durable persistence completes and a crash occurs, those records are **NOT** covered by the exactly-once guarantee and must be treated as SDK-level failures per `cpt-cf-usage-collector-fr-sdk-retry`. This guarantee applies to the service layer (collector, storage plugins) but does **NOT** apply to SDK-level drops caused by buffer exhaustion or sustained overload as defined in `cpt-cf-usage-collector-fr-sdk-retry`.

**Threshold**: Zero data loss or duplication after successful API ingestion under normal operation
**Rationale**: Duplicate or missing records directly impact billing accuracy. The exactly-once guarantee begins when the collector durably persists a record and returns success (HTTP 2xx/gRPC OK); acknowledged records are guaranteed to be queryable by downstream consumers (billing, quota enforcement). SDK-level drops before acknowledgment are observable via metrics and alerts, enabling operators to address capacity or configuration issues.

#### Audit Trail

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-audit-trail`

The system **MUST** preserve immutable audit records for all usage data including source, timestamps, and any corrections.

**Threshold**: 100% of usage operations audited
**Rationale**: Audit trails are required for billing disputes and compliance.

#### Authentication Required

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-authentication`

The system **MUST** require authentication (OAuth 2.0, mTLS, or API key) for all API operations.

**Threshold**: Zero unauthenticated API access
**Rationale**: Usage data is sensitive and must be protected.

#### Authorization Enforcement

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-authorization`

The system **MUST** enforce authorization for read/write operations based on tenant context and usage type permissions.

**Threshold**: Zero unauthorized data access
**Rationale**: Authorization prevents unauthorized usage data manipulation.

#### Horizontal Scalability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-scalability`

The system **MUST** scale horizontally to handle increased load without architectural changes.

**Threshold**: Linear throughput scaling with added instances
**Rationale**: Usage volume grows with platform adoption.

#### Storage Fault Tolerance

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-fault-tolerance`

The system **MUST** buffer usage records during storage backend failures and recover without data loss. This guarantee applies to records successfully accepted by the ingestion API; SDK-level drops due to buffer exhaustion before ingestion are covered by `cpt-cf-usage-collector-fr-sdk-retry`.

**Threshold**: Zero data loss during storage backend failures for records accepted by the collector
**Rationale**: Storage outages must not result in lost usage data after successful ingestion. The service layer must buffer and retry to ensure durability.

#### Configurable Retention

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-retention`

The system **MUST** support retention periods from 7 days to 7 years depending on usage type and compliance requirements.

**Threshold**: Configurable retention from 7 days to 7 years
**Rationale**: Different usage types have different compliance and operational retention needs.

#### Graceful Degradation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-graceful-degradation`

The system **MUST** continue accepting usage records even if downstream consumers (billing, monitoring) are unavailable.

**Threshold**: Zero ingestion failures due to downstream consumer unavailability
**Rationale**: Usage collection must not be blocked by consumer outages.

## 7. Public Library Interfaces

### 7.1 Public API Surface

#### Usage Ingestion SDK

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-interface-sdk`

**Type**: Client library
**Stability**: stable
**Description**: Client-side SDK with automatic batching for high-throughput usage emission. Primary ingestion path for platform services. Provides Rust API (in-process) and gRPC transports.
**Breaking Change Policy**: Major version bump required

#### HTTP API

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-interface-http-api`

**Type**: HTTP/REST API
**Stability**: stable
**Description**: HTTP API for usage record ingestion and administration. Supports external integrations and simple client scenarios.
**Breaking Change Policy**: Major version bump required

### 7.2 External Integration Contracts

#### Usage Collector Plugin Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-contract-plugin`

**Direction**: required from client
**Protocol/Format**: Rust trait / plugin interface
**Compatibility**: Backward-compatible within major version; plugins implement a defined trait for write-path operations (record persistence, deduplication storage, retention enforcement). Each storage backend provides a corresponding plugin.

#### Types Registry Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-contract-types-registry`

**Direction**: required from client
**Protocol/Format**: Internal API
**Compatibility**: Schema validation contract; UC depends on Types Registry for unit and type definitions.

## 8. Use Cases

### UC: High-Volume Usage Emission via SDK

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-sdk-emission`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`

**Preconditions**:
- Usage type registered in Types Registry

**Main Flow**:
1. Service calls SDK to record usage event
2. SDK queues event in memory
3. SDK batches events (by count or time threshold)
4. SDK sends batch to UC
5. UC validates each record against type schema
6. UC persists to configured storage backend via the active plugin
7. SDK receives acknowledgment

**Postconditions**:
- Usage records persisted with tenant/resource attribution; available for query by downstream consumers via usage-query module

### UC: Configure Custom Measuring Unit

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-custom-unit`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Preconditions**:
- Unit name is unique

**Main Flow**:
1. Operator defines unit schema (name, type: counter/gauge, base unit)
2. Operator submits via API
3. UC validates unit definition
4. UC registers unit with Types Registry
5. UC confirms registration

**Postconditions**:
- New unit immediately available for usage collection; sources can emit records with new unit type

### UC: Backfill Usage After Outage

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-usecase-backfill-after-outage`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Preconditions**:
- Gap detected (by external reconciliation system or operator investigation)
- Replacement usage data available from secondary source (infrastructure metrics, API gateway logs, or source service replay)

**Main Flow**:
1. Operator identifies gap (via external reconciliation system alerts or manual investigation)
2. Operator prepares replacement events from secondary source
3. Operator submits backfill request specifying time range, tenant, and replacement events
4. UC validates time range is within backfill window
5. UC archives existing events in the time range (if any)
6. UC validates and persists replacement events with backfill idempotency namespace
7. UC creates audit record for the backfill operation

**Postconditions**:
- Gap filled with corrected data; archived events retained for audit; downstream consumers can query corrected raw records via usage-query module

## 9. Acceptance Criteria

- [ ] All billable platform services emit usage through UC
- [ ] Usage records can be attributed to specific subjects (users, service accounts) within a tenant
- [ ] Custom unit registration completes in less than 5 minutes without code changes
- [ ] High-volume services can emit 10,000+ events per second without blocking
- [ ] 99.95%+ monthly availability maintained
- [ ] Zero data loss or duplication after successful ingestion (usage records accepted by the collector API are persisted exactly once)
- [ ] SDK buffer drops (pre-ingestion) are observable via loss metrics and trigger alerts when drop rate exceeds configurable thresholds (as defined in `cpt-cf-usage-collector-fr-sdk-retry`)
- [ ] Backfill API can restore missing usage data for any time range within the backfill window
- [ ] Zero data permanently deleted during backfill operations (archive-only)
- [ ] Usage metadata (event counts, watermarks) is available via API for external reconciliation systems
- [ ] A single tenant exceeding its rate limit does not degrade ingestion latency for other tenants
- [ ] Rate limit configuration changes take effect without service restart
- [ ] Usage records can carry optional extensible metadata (JSON object) that is persisted as-is and returned in query results
- [ ] Records with metadata exceeding the configured size limit (default 8 KB) are rejected with an actionable error

## 10. Dependencies

| Dependency | Description | Criticality |
|------------|-------------|-------------|
| Storage Backend (TimescaleDB or ClickHouse) | At least one storage backend available in the platform | p1 |
| Types Registry | Schema validation for usage types, custom measuring units, and source-to-usage-type authorization bindings | p1 |
| Platform Auth Infrastructure | Authentication and authorization infrastructure (OAuth 2.0, mTLS) | p1 |

## 11. Assumptions

- At least one storage backend (TimescaleDB or ClickHouse) is available in the platform
- Types Registry service is available for schema validation
- Platform authentication/authorization infrastructure exists
- High-volume consumers (Billing, Monitoring) integrate primarily via the usage-query module's aggregation API

## 12. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Storage backend unavailability | Usage records lost during outage | Buffering with retry and backoff |
| High ingestion volume exceeds capacity | Usage sources rejected; delayed billing data | Horizontal scaling; SDK-side batching and buffering; per-tenant rate limiting |
| Schema evolution breaks existing sources | Usage sources fail validation after type changes | Backward-compatible schema evolution; versioned type schemas |
| Cross-tenant data leakage | Security and compliance violation | Fail-closed authorization; tenant isolation at all layers |
| Insufficient usage metadata | External reconciliation systems cannot detect gaps if UC does not expose adequate metadata | Expose per-source event counts, watermarks, and ingestion statistics via dedicated API |
| Backfill data quality | Incorrect replacement data worsens the gap instead of fixing it | Full schema validation on backfill events; archive-not-delete allows rollback; audit trail enables investigation |
| Noisy-neighbor ingestion | A single tenant or source exhausts ingestion capacity, degrading service for all tenants | Hierarchical rate limiting (global, per-tenant, per-source); priority-based load shedding |
| Rate limit misconfiguration | Limits set too low cause SDK buffer exhaustion and event drops (as defined in `cpt-cf-usage-collector-fr-sdk-retry`); limits set too high provide no overload protection | System defaults with per-tenant hot-reloadable overrides; rate limit observability and approaching-limit alerts; SDK drop metrics and alerts enable operators to detect and remediate capacity issues |
| Retry storms after rate limiting | Synchronized client retries after a rate limit event amplify load | SDK enforces exponential backoff with jitter; rate limit responses provide explicit retry delay guidance (`cpt-cf-usage-collector-fr-rate-limit-response`) |

## 13. Open Questions

- Retention policy enforcement mechanism (TTL vs. scheduled cleanup)
- Exact SDK batching defaults (count threshold, time threshold)
- Default rate limit values for system-wide defaults and per-source defaults
- Specific metadata fields and API shape for metadata exposure (to support external reconciliation)
