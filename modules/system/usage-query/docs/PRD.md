# PRD — Usage Query

## 1. Overview

### 1.1 Purpose

A read-path module for querying, aggregating, and summarizing usage data collected by the usage-collector module. The Usage Query module provides both raw record access for auditing and debugging, and pluggable backend plugins for efficient data consumption. Business logic (pricing, rating, billing rules, invoice generation) remains the responsibility of downstream systems — Usage Query provides data aggregation primitives (SUM, COUNT, GROUP BY) but does not interpret, price, or act on the data.

### 1.2 Background / Problem Statement

The Usage Query module serves as the read-path complement to the usage-collector module. While usage-collector handles ingestion, storage, and data integrity, Usage Query handles querying, aggregation, and data exposure. It supports raw record retrieval with cursor-based pagination, server-side aggregation with pluggable backend plugins, and pre-computed rollups for high-volume consumers.

The module addresses the data gravity problem: at scale (10,000+ events/sec), exposing only raw records via paginated API forces consumers to make thousands of API calls and re-aggregate data externally — data should be aggregated where it is stored, not transferred over the network for external aggregation.

Key problems:

- **Data gravity problem**: At scale (10,000+ events/sec), exposing only raw records via paginated API forces consumers to make thousands of API calls and re-aggregate data externally
- **Backend lock-in**: No flexibility for different query and aggregation capabilities across storage backends
- **Query consistency**: Concurrent writes during pagination can cause missing or duplicate records without proper cursor and snapshot semantics

### 1.3 Goals (Business Outcomes)

- Consumers can retrieve aggregated data via server-side aggregation API without pulling and re-processing raw records
- Raw record access available for auditing, debugging, and dispute resolution
- Backend plugins are pluggable — no lock-in to a specific database technology
- Aggregation queries for 30-day ranges complete within 500ms (p95) with rollups, 2,000ms (p95) without

### 1.4 Glossary

| Term | Definition |
|------|------------|
| Usage Record | A single data point representing resource consumption by a tenant |
| Usage Query Plugin | A plugin that provides backend-specific data access for a specific storage backend (e.g., ClickHouse, TimescaleDB). Each plugin implements the full read-path interface for its backend: raw record retrieval with cursor-based pagination, server-side aggregation using backend-native capabilities (e.g., ClickHouse columnar GROUP BY, TimescaleDB continuous aggregates), and optionally pre-computed rollups. |
| Rollup | A pre-computed aggregate (e.g., hourly or daily SUM per tenant and usage type) maintained by the usage query plugin, typically via backend-native mechanisms such as materialized views or continuous aggregates |
| Time Bucket | A time interval (e.g., 1 hour, 1 day, 1 month) used to group usage records for aggregation |
| Snapshot Read | A query that sees data as it existed at a specific point in time, providing consistency across paginated requests despite concurrent data modifications |

## 2. Actors

### 2.1 Human Actors

#### Platform Operator

**ID**: `cpt-cf-usage-query-actor-platform-operator`

**Role**: Configures backend plugins, rollup policies, and monitors query performance.
**Needs**: Ability to manage backend plugins, define rollup configurations, and monitor query health without code changes.

#### Tenant Administrator

**ID**: `cpt-cf-usage-query-actor-tenant-admin`

**Role**: Queries raw and aggregated usage data for their tenant.
**Needs**: Access to raw and aggregated usage records filtered by type, subject, and resource for their tenant only, with time-range filtering.

#### Platform Developer

**ID**: `cpt-cf-usage-query-actor-platform-developer`

**Role**: Integrates services with Usage Query using APIs to consume usage data.
**Needs**: Well-documented APIs for querying raw and aggregated usage data with minimal integration effort.

### 2.2 System Actors

#### Billing System

**ID**: `cpt-cf-usage-query-actor-billing-system`

**Role**: Consumes aggregated and raw usage records from Usage Query for rating, pricing, and invoice generation.

#### Monitoring System

**ID**: `cpt-cf-usage-query-actor-monitoring-system`

**Role**: Consumes aggregated usage metrics from Usage Query for dashboards, alerting, and operational visibility.

#### Storage Backend

**ID**: `cpt-cf-usage-query-actor-storage-backend`

**Role**: The underlying data store (ClickHouse, TimescaleDB, or external system) that holds usage records ingested by usage-collector. Accessed by the usage query plugin for read operations. Shared with usage-collector.

## 3. Operational Concept & Environment

No module-specific environment constraints beyond project defaults.

## 4. Scope

### 4.1 In Scope

- Usage query API for raw record retrieval with filtering and cursor-based pagination
- Stable query result ordering for consistent pagination
- Snapshot read consistency for point-in-time queries
- Pluggable backend plugin framework for raw record retrieval and backend-native server-side aggregation
- Server-side aggregation query API (SUM, COUNT, MIN, MAX, AVG, GROUP BY, time bucketing)
- Multi-value query filters (multiple tenants, subjects, resources in a single query; multi-tenant `tenant_ids` filters require PDP authorization — for principals not authorized for multi-tenant queries, the gateway overrides the tenant filter to enforce single-tenant isolation)
- Plugin capabilities discovery API
- Pre-computed rollups (hourly, daily) via backend-native mechanisms
- Tenant query isolation

### 4.2 Out of Scope

- **Usage Ingestion & Storage**: Ingestion, deduplication, write-path storage plugins, retention policies, backfill, and rate limiting are handled by the usage-collector module.
- **Business Aggregation & Interpretation**: Business-specific data interpretation — pricing, rating, billing rules, invoice generation, chargeback allocation, quota policy decisions — is the responsibility of downstream consumers. Usage Query provides data aggregation primitives (SUM, COUNT, GROUP BY, time bucketing) but does not embed business logic.
- **Report Generation**: Usage reports, dashboards, and visualizations are handled by Monitoring/Analytics systems; Usage Query provides raw and aggregated data access.
- **Usage Prediction/Forecasting**: Deferred to future phase.

## 5. Functional Requirements

### 5.1 Querying

#### Usage Query API

- [ ] `p1` - **ID**: `cpt-cf-usage-query-fr-query-api`

The system **MUST** provide an API for querying raw usage records with filtering by time range, tenant, subject (subject_id and/or subject_type), resource, and usage type with cursor-based pagination. The raw record query API is intended for auditing, debugging, backfill verification, and scenarios where consumers need access to individual records. For high-volume data retrieval and summarization, the aggregation query API (`cpt-cf-usage-query-fr-aggregation-api`) is the preferred interface. The API **MUST** support querying archived records from backfill operations (`cpt-cf-usage-collector-fr-backfill-archival`).

**Rationale**: Downstream consumers need flexible access to raw usage data for auditing, debugging, dispute resolution, and scenarios requiring individual record inspection. Per-subject filtering enables chargeback scenarios and per-subject usage analytics.
**Actors**: `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`, `cpt-cf-usage-query-actor-tenant-admin`

#### Stable Query Result Ordering

- [ ] `p1` - **ID**: `cpt-cf-usage-query-fr-stable-ordering`

The system **MUST** return query results in a stable, deterministic order across all API endpoints. The ordering **MUST** be consistent across pagination requests to prevent records from being missed or duplicated when combined with cursor-based pagination.

**Rationale**: Stable ordering is essential for cursor-based pagination to work correctly. Without deterministic ordering, cursors cannot reliably mark positions in the result set, leading to missing or duplicate records across pages. This is critical for billing accuracy where downstream consumers must process complete, non-duplicated datasets.
**Actors**: `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`, `cpt-cf-usage-query-actor-tenant-admin`

#### Cursor-Based Pagination

- [ ] `p1` - **ID**: `cpt-cf-usage-query-fr-cursor-pagination`

The system **MUST** implement cursor-based pagination for all query APIs. Each page response **MUST** include an opaque cursor token that marks the position after the last record in the current page. Clients **MUST** pass this cursor in subsequent requests to retrieve the next page. Cursors **MUST** remain valid for at least 24 hours after issuance.

**Rationale**: Offset-based pagination (`LIMIT/OFFSET`) is unreliable when data is being inserted concurrently — new insertions shift offsets, causing records to be skipped or duplicated across pages. Cursor-based pagination provides stable position markers that are unaffected by concurrent writes. This is essential for billing systems that must process complete usage datasets without gaps or duplicates.
**Actors**: `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`, `cpt-cf-usage-query-actor-tenant-admin`, `cpt-cf-usage-query-actor-platform-developer`

#### Snapshot Read Consistency

- [ ] `p3` - **ID**: `cpt-cf-usage-query-fr-snapshot-reads`

The system **MUST** support snapshot reads, allowing clients to query data with a consistent point-in-time view. When a query is initiated with snapshot isolation, all subsequent pagination requests in that query session **MUST** see the dataset as it existed at the snapshot timestamp, regardless of concurrent insertions, updates, or backfill operations.

**Rationale**: Even with cursor-based pagination, concurrent data modifications (late-arriving events, backfill operations) can cause inconsistencies across paginated queries. Snapshot reads provide the strongest consistency guarantee: a billing system paginating through a month of data sees the exact same records on every page, as they existed when the query started. This is marked p3 because cursor-based pagination with stable ordering provides sufficient consistency for most use cases, but snapshot isolation is valuable when absolute consistency is required for auditing or financial reconciliation.
**Actors**: `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`, `cpt-cf-usage-query-actor-tenant-admin`

### 5.2 Server-Side Aggregation

#### Aggregation Query API

- [ ] `p1` - **ID**: `cpt-cf-usage-query-fr-aggregation-api`

The system **MUST** provide a server-side aggregation query API that executes aggregation operations within the storage backend, returning aggregated results directly to consumers. The API **MUST** support the following aggregation functions at minimum: SUM, COUNT, MIN, MAX, AVG. The API **MUST** support grouping results by any combination of: tenant, subject (subject_id, subject_type), resource (resource_id, resource_type), usage type, source, and configurable time buckets (e.g., 1 hour, 1 day, 1 month). The API **MUST** support multi-value filters — accepting multiple values for tenant_ids, usage_types, subject_ids, resource_types, and source_ids in a single query. Multi-tenant queries (multiple tenant_ids in a single request) require PDP authorization; for principals not authorized for multi-tenant queries, the gateway **MUST** constrain or override the tenant_ids filter to the principal's own tenant, enforcing isolation. The gateway **MUST** evaluate all usage queries with `barrier_mode: "none"` — barriers are not enforced for usage data per the platform tenant model, which designates usage/billing metrics as barrier-exempt so that parent tenants can aggregate usage across self-managed child tenants.

The aggregation query API is a complement to the raw record query API (`cpt-cf-usage-query-fr-query-api`), not a replacement. The raw record API remains available for auditing, debugging, and backfill verification. The aggregation API is the primary interface for high-volume consumers (billing, monitoring, analytics) that need summarized data without transferring and re-processing raw records.

**Rationale**: At sustained ingestion rates of 10,000+ events per second, exposing only raw records via paginated API forces consumers to make thousands of paginated calls and re-aggregate data externally. Server-side aggregation leverages the storage backend's native aggregation capabilities (e.g., ClickHouse columnar GROUP BY, TimescaleDB continuous aggregates) to return results in a single call, reducing network transfer, consumer complexity, and end-to-end latency by orders of magnitude.
**Actors**: `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`, `cpt-cf-usage-query-actor-tenant-admin`

#### Pluggable Backend Plugin Framework

- [ ] `p1` - **ID**: `cpt-cf-usage-query-fr-plugin-framework`

The system **MUST** support pluggable backend plugins that implement the full read-path interface for a specific storage backend. Each storage backend **MUST** have a corresponding usage query plugin that provides: raw record retrieval with cursor-based pagination, server-side aggregation using backend-native query capabilities, and optionally pre-computed rollup management. This follows the platform's gateway pattern where the Usage Query gateway delegates to the active plugin discovered via the types registry.

Each plugin encapsulates all backend-specific read logic, including differences in query syntax, pagination implementation, aggregation capabilities, and rollup mechanisms.

**Rationale**: Different storage backends have fundamentally different query and aggregation capabilities: ClickHouse excels at columnar aggregation with materialized views, TimescaleDB offers continuous aggregates, PostgreSQL uses different cursor semantics, etc. A unified plugin per backend avoids fragmenting backend-specific logic across multiple extension points and ensures that raw record retrieval and aggregation are implemented consistently by the same component that understands the backend's native capabilities.
**Actors**: `cpt-cf-usage-query-actor-platform-operator`, `cpt-cf-usage-query-actor-storage-backend`

#### Plugin Capabilities Discovery

- [ ] `p2` - **ID**: `cpt-cf-usage-query-fr-plugin-capabilities`

The system **MUST** expose the capabilities of the active plugin via API, allowing consumers to discover which aggregation functions, time bucket granularities, and features (e.g., rollup support, snapshot reads, percentiles, histograms) are available. Consumers **MUST** be able to query capabilities before submitting requests.

**Rationale**: Because backend plugins are pluggable and different backends support different capabilities, consumers need a way to discover what operations are available at runtime. This prevents runtime errors from unsupported requests and allows consumers to adapt their queries to the available backend.
**Actors**: `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`, `cpt-cf-usage-query-actor-platform-developer`

#### Pre-Computed Rollups

- [ ] `p2` - **ID**: `cpt-cf-usage-query-fr-rollups`

The plugin **MUST** support pre-computed rollups — automatically maintained aggregates at configured time granularities (e.g., hourly, daily totals per tenant and usage type). Rollup configuration **MUST** be manageable by operators without code changes. When rollups are available, the aggregation query API **MUST** transparently use them for queries that match the rollup granularity, falling back to on-the-fly aggregation for non-matching queries.

Rollup implementation is backend-specific: ClickHouse may use materialized views, TimescaleDB may use continuous aggregates. The plugin encapsulates this backend-specific mechanism.

**Rationale**: Pre-computed rollups dramatically reduce query latency and resource consumption for common aggregation patterns (e.g., daily billing summaries, hourly monitoring dashboards). By maintaining rollups at ingest time (where supported by the backend), the system avoids scanning raw records for every aggregation query. Rollup configuration by operators (without code changes) enables adapting to new business needs as they arise.
**Actors**: `cpt-cf-usage-query-actor-platform-operator`, `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`

### 5.3 Tenant Isolation

#### Tenant Query Isolation

- [ ] `p1` - **ID**: `cpt-cf-usage-query-fr-tenant-isolation`

The system **MUST** enforce strict tenant isolation on all query operations, ensuring usage data is never accessible across tenants, failing closed on authorization failures. Data-level isolation is enforced at ingestion by the usage-collector module (`cpt-cf-usage-collector-fr-tenant-isolation`); this requirement covers query-path enforcement.

**Rationale**: Tenant data isolation is a security and compliance requirement that must be enforced at every access point, including the query path.
**Actors**: `cpt-cf-usage-query-actor-platform-operator`, `cpt-cf-usage-query-actor-tenant-admin`, `cpt-cf-usage-query-actor-billing-system`, `cpt-cf-usage-query-actor-monitoring-system`

## 6. Non-Functional Requirements

### 6.1 Module-Specific NFRs

#### Raw Query Latency

- [ ] `p1` - **ID**: `cpt-cf-usage-query-nfr-query-latency`

The system **MUST** complete raw usage record queries for 30-day ranges within 500ms (p95) under normal load.

**Threshold**: p95 <= 500ms for 30-day range raw record queries
**Rationale**: Auditing and debugging scenarios require responsive raw record access.

#### Aggregation Query Latency

- [ ] `p1` - **ID**: `cpt-cf-usage-query-nfr-aggregation-latency`

The system **MUST** complete aggregation queries for 30-day ranges within 500ms (p95) under normal load when pre-computed rollups are available, and within 2,000ms (p95) for ad-hoc aggregations without rollups.

**Threshold**: p95 <= 500ms (rollup-backed), p95 <= 2,000ms (ad-hoc) for 30-day range aggregation queries
**Rationale**: Billing and monitoring systems require fast aggregated query responses. Pre-computed rollups serve the most common query patterns with low latency; ad-hoc aggregations over raw data have a higher but still bounded latency ceiling.

#### Authentication Required

- [ ] `p1` - **ID**: `cpt-cf-usage-query-nfr-authentication`

The system **MUST** require authentication (OAuth 2.0, mTLS, or API key) for all API operations.

**Threshold**: Zero unauthenticated API access
**Rationale**: Usage data is sensitive and must be protected.

#### Authorization Enforcement

- [ ] `p1` - **ID**: `cpt-cf-usage-query-nfr-authorization`

The system **MUST** enforce authorization for read operations based on tenant context and usage type permissions.

**Threshold**: Zero unauthorized data access
**Rationale**: Authorization prevents unauthorized usage data access.

## 7. Public Library Interfaces

### 7.1 Public API Surface

#### Usage Query SDK

- [ ] `p1` - **ID**: `cpt-cf-usage-query-interface-sdk`

**Type**: Client library
**Stability**: stable
**Description**: Client-side SDK for programmatic access to raw record queries, aggregation queries, and plugin capabilities discovery. Primary interface for other platform modules (billing, monitoring, quota enforcement).
**Breaking Change Policy**: Major version bump required

#### HTTP API

- [ ] `p1` - **ID**: `cpt-cf-usage-query-interface-http-api`

**Type**: HTTP/REST API
**Stability**: stable
**Description**: HTTP API for raw record querying, aggregation queries, and plugin capabilities discovery. Supports external integrations and administrative access.
**Breaking Change Policy**: Major version bump required

### 7.2 External Integration Contracts

#### Usage Query Plugin Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-query-contract-plugin`

**Direction**: required from client
**Protocol/Format**: Rust trait / plugin interface
**Compatibility**: Backward-compatible within major version; plugins implement a defined trait for raw record retrieval with cursor-based pagination, executing server-side aggregation queries, declaring capabilities, and optionally managing pre-computed rollups. Each storage backend provides a corresponding plugin. The plugin is the sole extension point for backend-specific read logic — there is no separate storage adapter contract.

## 8. Use Cases

### UC: Query Aggregated Tenant Usage for Billing

- [ ] `p1` - **ID**: `cpt-cf-usage-query-usecase-billing-query`

**Actor**: `cpt-cf-usage-query-actor-billing-system`

**Preconditions**:
- Billing period defined by Billing System
- Backend plugin active with required capabilities

**Main Flow**:
1. Billing System submits aggregation query for billing period (time range, tenant_ids, usage types, group_by=[tenant, usage_type, subject], time_bucket=1 day, aggregations=[SUM, COUNT])
2. Usage Query gateway delegates to active plugin, which executes backend-native aggregation
3. Usage Query returns aggregated results (e.g., daily totals per tenant per usage type)
4. Billing System applies pricing rules and generates invoices from aggregated data
5. For dispute resolution or audit, Billing System optionally queries the raw record API (`cpt-cf-usage-query-fr-query-api`) for specific time ranges

**Postconditions**:
- Billing System has accurate, aggregated usage data without transferring millions of raw records
- Aggregation was performed server-side, leveraging storage backend's native capabilities
- Raw records remain available for audit and dispute resolution

### UC: Query Raw Tenant Usage Records

- [ ] `p1` - **ID**: `cpt-cf-usage-query-usecase-raw-billing-query`

**Actor**: `cpt-cf-usage-query-actor-billing-system`

**Preconditions**:
- Billing period defined by Billing System

**Main Flow**:
1. Billing System queries raw usage API for billing period (time range, tenant, optional subject filter, usage types)
2. Usage Query retrieves raw usage records matching the filter criteria in stable order
3. Usage Query returns first page with cursor token
4. Billing System processes page and requests next page using cursor
5. Steps 3-4 repeat until all pages retrieved
6. Billing System processes complete record set (e.g., for auditing, dispute resolution, or aggregation logic not supported by the plugin)

**Postconditions**:
- Billing System has accurate, deduplicated raw usage records
- No records missed or duplicated due to concurrent insertions during pagination
- If subject filtering was applied, billing can generate per-subject chargeback reports

### UC: Query Tenant Usage Data

- [ ] `p1` - **ID**: `cpt-cf-usage-query-usecase-tenant-usage-query`

**Actor**: `cpt-cf-usage-query-actor-tenant-admin`

**Main Flow**:
1. Administrator queries usage API for a time period with optional subject and resource filters
2. Usage Query retrieves raw usage records scoped to the tenant only in stable order
3. Usage Query returns paginated raw records with cursor tokens, filtered by type, subject, and resource
4. Administrator retrieves all pages using cursors
5. Administrator (or downstream reporting system) processes the complete data

**Postconditions**:
- Administrator receives only their tenant's raw usage records; no cross-tenant data exposure
- Paginated results are consistent without gaps or duplicates
- Administrator can analyze usage by specific subjects (users, service accounts) within their tenant

## 9. Acceptance Criteria

- [ ] Downstream consumers (billing, monitoring) querying the same time range via aggregation API receive consistent aggregated results
- [ ] Downstream consumers querying the same time range via raw record API receive identical raw records
- [ ] Query results are returned in stable, deterministic order across all pagination requests
- [ ] Cursor-based pagination prevents record gaps or duplicates during concurrent insertions
- [ ] Cursors remain valid for at least 24 hours after issuance
- [ ] Aggregation query API returns correct results for SUM, COUNT, MIN, MAX, AVG with GROUP BY and time bucketing
- [ ] Aggregation queries for 30-day ranges complete within 500ms (p95) when rollups are available, and within 2,000ms (p95) for ad-hoc aggregations
- [ ] Plugin capabilities are discoverable at runtime via API
- [ ] A new backend plugin can be added without modifying core Usage Query code
- [ ] Multi-value filters (multiple tenant_ids, usage_types, subject_ids) work correctly in aggregation queries; multi-tenant tenant_ids filters are constrained by PDP authorization, and the gateway enforces single-tenant isolation for principals not authorized for multi-tenant queries
- [ ] Pre-computed rollups (where supported by the backend) are transparently used for matching aggregation queries
- [ ] Tenant query isolation prevents any cross-tenant data access on the query path

## 10. Dependencies

| Dependency | Description | Criticality |
|------------|-------------|-------------|
| Usage Collector Module | Provides ingested usage data in storage backends | p1 |
| Storage Backend (TimescaleDB or ClickHouse) | At least one storage backend available in the platform | p1 |
| Platform Auth Infrastructure | Authentication and authorization infrastructure (OAuth 2.0, mTLS) | p1 |

## 11. Assumptions

- Usage data is ingested and persisted by the usage-collector module (via its own plugins); Usage Query reads from the same storage backends via its own plugins
- The active storage backend has a corresponding usage query plugin
- Platform authentication/authorization infrastructure exists
- High-volume consumers (Billing, Monitoring) integrate primarily via aggregation query API; raw record query API is used for auditing and debugging

## 12. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Raw record API used as primary consumer interface | At 10k+ events/sec, consumers making thousands of paginated calls to pull raw data for external aggregation creates unsustainable network load and consumer complexity | Server-side aggregation API as primary consumer interface; raw record API reserved for auditing and debugging |
| Plugin capability gaps | A storage backend's plugin may not support all functions consumers need | Capabilities discovery API allows consumers to check before querying; a custom plugin can be implemented to extend capabilities; fallback to raw record API for unsupported operations |
| Rollup configuration drift | Pre-computed rollups may become stale or inconsistent if configuration changes aren't propagated to the backend | Rollup setup is managed through the plugin with operator-initiated configuration; plugin validates rollup health at startup |
| Cross-tenant data leakage via queries | Security and compliance violation | Fail-closed authorization on all query paths; tenant isolation enforced at query level in addition to storage level |

## 13. Open Questions

- Federated-query merge strategy for operational/historical storage split: how to guarantee stable ordering (`cpt-cf-usage-query-fr-stable-ordering`) when merging results from both tiers (e.g., merge by event timestamp + deterministic tiebreaker, or global sequence/watermark)
- Cursor semantics for federated queries: cursor token payload format to support pagination across operational and historical storage tiers (must include source shard identifiers, per-source offsets, and global watermark)
- Migration coordination for federated cursors: how cursors spanning an operational-to-historical migration are handled (e.g., migration watermark in cursor to consult both plugins, or invalidation error requiring cursor refresh)
- Rollup granularity defaults: what time bucket granularities should be pre-configured for rollups (hourly + daily, or also weekly/monthly)?
- Aggregation query result size limits: should there be a maximum number of groups returned in a single aggregation response, and what pagination model (if any) applies to aggregation results?
