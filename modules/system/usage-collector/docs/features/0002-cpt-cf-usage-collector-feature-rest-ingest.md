---
cpt:
  version: "1.3"
  changelog:
    - version: "1.3"
      date: "2026-04-29"
      changes:
        - "Document TLS/HTTPS requirements for base_url: add Security sub-section to §5 and AC item for startup behaviour on http:// scheme with non-localhost host (SEC-FDESIGN-004)"
    - version: "1.2"
      date: "2026-04-29"
      changes:
        - "Document at-least-once delivery idempotency semantics: add duplicate-delivery note to DoD §5 and flow error scenario, add AC item for idempotent upsert on idempotency_key (REL-FDESIGN-003)"
    - version: "1.1"
      date: "2026-04-29"
      changes:
        - "Add module_name URL percent-encoding requirement to inst-cfg-rem-3 (SEC-FDESIGN-003)"
    - version: "1.0"
      date: "2026-04-28"
      changes:
        - "Initial feature specification"
---

# Feature: REST Client & Remote Ingest Delivery

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Remote Usage Emission Flow](#remote-usage-emission-flow)
  - [Module Config Retrieval Flow (REST)](#module-config-retrieval-flow-rest)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [REST Client Module Initialization](#rest-client-module-initialization)
- [4. States (CDSL)](#4-states-cdsl)
- [5. Definitions of Done](#5-definitions-of-done)
  - [`usage-collector-rest-client` Crate](#usage-collector-rest-client-crate)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Non-Applicability Notes](#7-non-applicability-notes)

<!-- /toc -->

- [x] `p1` - **ID**: `cpt-cf-usage-collector-featstatus-rest-ingest`
<!-- STATUS: IMPLEMENTED — all p1 DoD items and all CDSL blocks are [x]. -->

<!-- reference to DECOMPOSITION entry -->
- [x] `p1` - `cpt-cf-usage-collector-feature-rest-ingest`

## 1. Feature Context

### 1.1 Overview

Enables out-of-process usage sources to emit records using the same `UsageEmitterV1` API as in-process sources by providing the `usage-collector-rest-client` crate, which delivers records to the collector gateway over HTTP with service-to-service bearer token authentication.

### 1.2 Purpose

Implements the remote delivery counterpart to Feature 1's in-process delivery path. Out-of-process sources call `authorize_for()` and `enqueue()` identically to in-process sources; only the outbox delivery hop differs — the background pipeline HTTP-POSTs each record to the gateway ingest endpoint instead of calling it in-process. The `usage-collector-rest-client` crate registers `UsageEmitterV1` in `ClientHub`, backed by `UsageCollectorRestClient` implementing `UsageCollectorClientV1`.

**Requirements**: `cpt-cf-usage-collector-fr-rest-ingestion`

**NFR targets**: See PRD §NFRs; `cpt-cf-usage-collector-nfr-recovery` constrains `outbox_backoff_max` to below 15 minutes.

**Principles**: `cpt-cf-usage-collector-principle-fail-closed`, `cpt-cf-usage-collector-principle-tenant-from-ctx`

**Constraints**: `cpt-cf-usage-collector-constraint-security-context`, `cpt-cf-usage-collector-constraint-modkit`, `cpt-cf-usage-collector-constraint-outbox-infra`

### 1.3 Actors

**Actors** (defined in PRD.md):

- `cpt-cf-usage-collector-actor-usage-source` — out-of-process usage source emitting records via the REST client

### 1.4 References

- **PRD**: [PRD.md](../PRD.md)
- **Design**: [DESIGN.md](../DESIGN.md)
- **DECOMPOSITION**: [DECOMPOSITION.md](../DECOMPOSITION.md)
- **Dependencies**: `cpt-cf-usage-collector-feature-sdk-and-ingest-core`

## 2. Actor Flows (CDSL)

**Sequences**: `cpt-cf-usage-collector-seq-emit-remote`

### Remote Usage Emission Flow

- [x] `p1` - **ID**: `cpt-cf-usage-collector-flow-rest-ingest-remote-emit`

**Actor**: `cpt-cf-usage-collector-actor-usage-source` (out-of-process)

**Success Scenarios**:
- Outbox delivers record to gateway; gateway returns 204 No Content; outbox advances partition cursor

**Error Scenarios**:
- AuthN resolver unavailable → transient; outbox retries with exponential backoff
- Client credentials permanently rejected by AuthN resolver → delivery attempt fails; message rejected to dead-letter store
- Gateway returns 401 (token expired or invalid) → transient; next attempt acquires a fresh bearer token
- Gateway returns 429 or 5xx → transient; outbox retries with exponential backoff
- Gateway returns 4xx (excluding 401 and 429) → permanent; message moved to dead-letter store
- Gateway 204 on duplicate delivery (same `idempotency_key`) → idempotent; storage layer performs no-op upsert; outbox advances cursor normally

**Steps**:
1. [x] - `p1` - Outbox background pipeline calls REST client `UsageCollectorClientV1::create_usage_record(record)` for each ready outbox message - `inst-rem-1`
2. [x] - `p1` - REST client acquires a bearer token from the platform AuthN resolver via client credentials flow - `inst-rem-2`
3. [x] - `p1` - **IF** AuthN resolver returns a transient error (service temporarily unreachable, network failure, or any error other than credential rejection) - `inst-rem-3`
   1. [x] - `p1` - **RETURN** `UsageCollectorError::Unavailable`; outbox library applies exponential backoff retry - `inst-rem-3a`
4. [x] - `p1` - **IF** AuthN resolver rejects credentials as permanently invalid (`Unauthorized`) OR no AuthN plugin is registered (`NoPluginAvailable`) - `inst-rem-4`
   1. [x] - `p1` - **RETURN** `UsageCollectorError::AuthorizationFailed` or `UsageCollectorError::Internal`; outbox moves message to dead-letter store - `inst-rem-4a`
5. [x] - `p1` - HTTP POST `POST /usage-collector/v1/records` with `Authorization: Bearer <token>` header and serialized `UsageRecord` JSON body; `subject_id`, `subject_type`, and `metadata` serialize as absent JSON fields when `None` - `inst-rem-5`
6. [x] - `p1` - **IF** HTTP request times out - `inst-rem-6`
   1. [x] - `p1` - **RETURN** `UsageCollectorError::PluginTimeout`; outbox retries with exponential backoff; `outbox_backoff_max` MUST be configured below 15 minutes to satisfy `cpt-cf-usage-collector-nfr-recovery` - `inst-rem-6a`
7. [x] - `p1` - **IF** HTTP transport error (connection refused, DNS failure, TLS error, or other non-timeout network error) - `inst-rem-7`
   1. [x] - `p1` - **RETURN** `UsageCollectorError::Unavailable`; outbox retries with exponential backoff - `inst-rem-7a`
8. [x] - `p1` - **IF** gateway returns 204 No Content - `inst-rem-8`
   1. [x] - `p1` - **RETURN** `Ok(())`; outbox advances partition cursor — record is confirmed delivered - `inst-rem-8a`
9. [x] - `p1` - **IF** gateway returns 401 Unauthenticated (token invalid or expired) - `inst-rem-9`
   1. [x] - `p1` - **RETURN** `UsageCollectorError::AuthorizationFailed`; outbox retries — next attempt acquires a fresh bearer token; no record is lost - `inst-rem-9a`
10. [x] - `p1` - **IF** gateway returns 429 Too Many Requests or any 5xx - `inst-rem-10`
    1. [x] - `p1` - **RETURN** `UsageCollectorError::PluginTimeout`; outbox retries with exponential backoff - `inst-rem-10a`
11. [x] - `p1` - **IF** gateway returns any other 4xx (excluding 401 and 429) - `inst-rem-11`
    1. [x] - `p1` - **RETURN** `UsageCollectorError::Internal`; outbox moves message to dead-letter store and surfaces via monitoring - `inst-rem-11a`

### Module Config Retrieval Flow (REST)

- [x] `p2` - **ID**: `cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Success Scenarios**:
- Gateway returns `ModuleConfig` with the static `allowed_metrics` list for the requesting module

**Error Scenarios**:
- Module not registered in gateway static config → gateway returns 404; emitter surfaces `UsageEmitterError::ModuleNotConfigured`
- AuthN resolver or gateway temporarily unavailable → emitter surfaces `UsageEmitterError::Internal` per `inst-authz-5b`

**Steps**:
1. [x] - `p2` - During `authorize_for()` phase 1, emitter calls `UsageCollectorClientV1::get_module_config(module_name)` on the REST client - `inst-cfg-rem-1`
2. [x] - `p2` - REST client acquires a bearer token from the platform AuthN resolver via client credentials flow - `inst-cfg-rem-2`
3. [x] - `p2` - HTTP GET `GET /usage-collector/v1/modules/{module_name}/config`
   with `Authorization: Bearer <token>` header; `module_name` MUST be
   percent-encoded (URL percent-encoding) when interpolated into the URL path - `inst-cfg-rem-3`
4. [x] - `p2` - **IF** gateway returns 200 OK - `inst-cfg-rem-4`
   1. [x] - `p2` - Deserialize response body into `ModuleConfig`; **RETURN** `Ok(ModuleConfig { module_name, allowed_metrics })` - `inst-cfg-rem-4a`
5. [x] - `p2` - **IF** gateway returns 404 Not Found - `inst-cfg-rem-5`
   1. [x] - `p2` - **RETURN** `UsageCollectorError::ModuleNotFound(module_name)`; emitter surfaces `UsageEmitterError::ModuleNotConfigured` - `inst-cfg-rem-5a`
6. [x] - `p2` - **IF** any other error (transport failure, 4xx, 5xx) - `inst-cfg-rem-6`
   1. [x] - `p2` - **RETURN** appropriate `UsageCollectorError` variant; emitter handles per `inst-authz-5b` — infrastructure failures surface as `UsageEmitterError::Internal` - `inst-cfg-rem-6a`

## 3. Processes / Business Logic (CDSL)

### REST Client Module Initialization

- [x] `p1` - **ID**: `cpt-cf-usage-collector-algo-rest-ingest-module-init`

**Input**: `ModuleCtx` (config, `ClientHub`, DB connection)

**Output**: `UsageEmitterV1` registered in `ClientHub`; outbox schema migrations registered via `DatabaseCapability`

**Steps**:
1. [x] - `p1` - Load and validate `UsageCollectorRestClientConfig` from `ModuleCtx`; required fields `client_id` and `client_secret` must be non-empty; fail module startup with descriptive error on missing or invalid config - `inst-init-1`
2. [x] - `p1` - Acquire DB connection from `ModuleCtx`; fail module startup if DB is not available - `inst-init-2`
3. [x] - `p1` - Retrieve `AuthNResolverClient` from `ClientHub`; fail module startup if not registered - `inst-init-3`
4. [x] - `p1` - Retrieve `AuthZResolverClient` from `ClientHub`; fail module startup if not registered - `inst-init-4`
5. [x] - `p1` - Construct `UsageCollectorRestClient` from config and `AuthNResolverClient`; build `HttpClient` with configured `request_timeout` (default 30 s); trim trailing slashes from `base_url` - `inst-init-5`
6. [x] - `p1` - Build `UsageEmitter` with `AuthZResolverClient`, `UsageCollectorRestClient` as the delivery target, DB connection, and `UsageEmitterConfig` — the emitter owns the source's outbox queue; outbox background pipeline will call `create_usage_record()` on the REST client for each delivery attempt - `inst-init-6`
7. [x] - `p1` - Register `UsageEmitterV1` in `ClientHub` — out-of-process sources retrieve this emitter at initialization to access the two-phase emission API (`authorize_for()` / `enqueue()`) - `inst-init-7`

**DatabaseCapability**: `migrations()` returns `modkit_db::outbox::outbox_migrations()` — this module owns the source's local outbox queue; the same schema migration set as the in-process path applies.

## 4. States (CDSL)

Not applicable for this feature. No new entity state machines are introduced by the REST client. `UsageRecord.status` transitions (`active` → `inactive`) remain owned by Feature 8. The outbox message lifecycle is managed by the `modkit-db` outbox library and is not a domain state machine defined here.

## 5. Definitions of Done

### `usage-collector-rest-client` Crate

- [x] `p1` - **ID**: `cpt-cf-usage-collector-dod-rest-ingest-rest-client-crate`

The system **MUST** implement the `usage-collector-rest-client` crate providing:

- `UsageCollectorRestClientModule` implementing `Module::init()`: loads `UsageCollectorRestClientConfig`, acquires `AuthNResolverClient` and `AuthZResolverClient` from `ClientHub`, constructs `UsageCollectorRestClient` with configured `HttpClient`, builds `UsageEmitter` backed by the REST client, and registers `UsageEmitterV1` in `ClientHub`
- `UsageCollectorRestClient` implementing `UsageCollectorClientV1::create_usage_record()`: acquires a bearer token via the platform AuthN resolver (client credentials flow) before each delivery attempt; HTTP-POSTs the serialized `UsageRecord` to `POST /usage-collector/v1/records` with `Authorization: Bearer <token>`; maps `204` to `Ok(())`; maps `401`/`403` to `AuthorizationFailed`; maps `429`/`5xx` to `PluginTimeout` (triggers Retry); maps other `4xx` to `Internal` (triggers Reject); maps HTTP timeouts to `PluginTimeout`; maps other transport errors to `Unavailable`
- `UsageCollectorRestClient` implementing `UsageCollectorClientV1::get_module_config()`: acquires a bearer token; HTTP-GETs `GET /usage-collector/v1/modules/{module_name}/config`; deserializes `200` into `ModuleConfig`; maps `404` to `ModuleNotFound`; maps other errors appropriately
- `DatabaseCapability::migrations()` returning `modkit_db::outbox::outbox_migrations()` — owns the source's local outbox schema

**Implements**:
- `cpt-cf-usage-collector-flow-rest-ingest-remote-emit`
- `cpt-cf-usage-collector-flow-rest-ingest-fetch-module-config`
- `cpt-cf-usage-collector-algo-rest-ingest-module-init`
- `cpt-cf-usage-collector-component-rest-client`

**Constraints**: `cpt-cf-usage-collector-constraint-security-context`, `cpt-cf-usage-collector-constraint-modkit`, `cpt-cf-usage-collector-constraint-outbox-infra`

**Touches**:
- API: `POST /usage-collector/v1/records` (client-side HTTP delivery), `GET /usage-collector/v1/modules/{module_name}/config` (client-side config fetch)
- DB: outbox queue in source's local DB — same schema as in-process path (`cpt-cf-usage-collector-dbtable-outbox`)
- Entities: `UsageRecord`, `ModuleConfig`

**Configuration**:

| Parameter | Type | Default | Required | Notes |
|-----------|------|---------|----------|-------|
| `base_url` | string | `http://127.0.0.1:8080` | No | Trailing slash trimmed |
| `client_id` | string | — | Yes | OAuth2 client identifier |
| `client_secret` | secret string | — | Yes | Env-expanded; never logged |
| `scopes` | list\<string\> | `[]` | No | OAuth2 scopes; IdP defaults when empty |
| `request_timeout` | duration | `30s` | No | Per-request HTTP timeout |
| `emitter` | `UsageEmitterConfig` | defaults | No | Outbox/authorization tuning; `outbox_backoff_max` MUST be below 15 minutes |

**Delivery guarantees**: At-least-once delivery via the source's transactional outbox — identical to the in-process path. A bearer token is acquired on each delivery attempt, so expired tokens trigger retry with a fresh token and do not cause permanent record loss. All records durably committed to the source's local outbox are guaranteed to eventually reach the gateway. At-least-once delivery via the transactional outbox may produce duplicate `create_usage_record()` calls on retry after a network timeout or transient failure. The gateway **MUST** deduplicate repeated deliveries of the same record via idempotent upsert on `idempotency_key`; a second delivery of the same record **MUST** be a no-op at the storage layer.

**Observability**: Structured log events MUST be emitted for: token acquisition failure (`WARN`), delivery retry (`INFO`), dead-letter routing (`ERROR`). Outbox queue depth and delivery attempt count are surfaced via the same metrics as the in-process path. Bearer tokens and client secrets MUST NOT appear in log output.

**Data Protection**: Bearer tokens are short-lived credentials managed by the AuthN resolver; this crate holds them only for the duration of a single delivery attempt and does not persist or cache them. `client_secret` is stored as `SecretString` and is not exposed in logs or error messages.

### TLS/HTTPS Configuration

- [x] `p1` - **ID**: `cpt-cf-dod-rest-ingest-tls-config`

The system **MUST** enforce the following transport security requirements for the `base_url` configuration field:

- In production environments, `base_url` **MUST** use the `https://` scheme. An `http://` scheme is permitted only when the host resolves to a localhost or loopback address (`127.0.0.1`, `::1`, or `localhost`) — exclusively for development and testing environments.
- TLS certificate validation is enforced by the HTTP client by default. Disabling certificate validation or trusting a self-signed certificate **MUST** require an explicit operator opt-in (for example, via a dedicated configuration flag); silent trust-all behaviour is prohibited.
- The module **MUST** emit a startup warning (`WARN` level) when `base_url` uses the `http://` scheme with a non-localhost host, or **MUST** refuse to start with a descriptive configuration validation error, consistent with the platform security baseline.

**Implements**:
- `cpt-cf-usage-collector-dod-rest-ingest-rest-client-crate`

**Constraints**: `cpt-cf-usage-collector-constraint-security-context`

## 6. Acceptance Criteria

- [ ] Out-of-process source retrieves `UsageEmitterV1` from `ClientHub` after `usage-collector-rest-client` `init()` completes; `authorize_for()` and `enqueue()` behave identically to the in-process path
- [ ] `create_usage_record()` acquires a bearer token from the platform AuthN resolver via client credentials before each HTTP POST to `POST /usage-collector/v1/records`
- [ ] Gateway 204 No Content causes `HandlerResult::Success`; outbox partition cursor advances and the outbox row is deleted
- [ ] Gateway 401 causes retry — next delivery attempt acquires a fresh bearer token; a temporarily expired token does not cause permanent record loss
- [ ] Gateway 429 or 5xx causes `HandlerResult::Retry`; outbox applies exponential backoff
- [ ] Gateway 4xx (excluding 401 and 429) causes `HandlerResult::Reject`; message is moved to dead-letter store
- [ ] AuthN resolver transient failure (network error, service restart) causes `HandlerResult::Retry`; records in the source outbox are not lost
- [ ] AuthN resolver permanent credential rejection causes `HandlerResult::Reject`; message is moved to dead-letter store
- [ ] HTTP timeout causes `HandlerResult::Retry` via `PluginTimeout`; network transport errors cause `HandlerResult::Retry` via `Unavailable`
- [ ] `get_module_config()` returns `ModuleConfig` on gateway 200 OK; returns `ModuleNotFound` on 404
- [ ] `DatabaseCapability::migrations()` registers outbox schema migrations; source's local outbox is created on module startup
- [ ] Missing or invalid `client_id` / `client_secret` configuration causes `init()` to fail with a descriptive error; the process does not start
- [ ] Bearer token and `client_secret` do not appear in log output or error messages
- [ ] `create_usage_record()` called twice with the same `idempotency_key` produces gateway 204 on both attempts; the second delivery is a no-op at the storage layer (idempotent upsert on `idempotency_key`)
- [ ] A `base_url` configured with an `http://` scheme pointing to a non-localhost host either causes `init()` to fail with a descriptive configuration validation error or emits a `WARN`-level startup warning; this behaviour is documented in the crate README and is consistent with the platform security baseline (SEC-FDESIGN-004)

**Test data requirements**:
(1) AuthN resolver stub must support transient (`Unavailable`) and permanent (`Unauthorized`, `NoPluginAvailable`) error simulation.
(2) Gateway HTTP stub must be configurable to return 204, 401, 403, 429, 500, and 404 for `POST /records` and `GET /modules/{name}/config`.
(3) Integration tests verify the full outbox-to-gateway delivery path with the REST client transport using a gateway stub.

## 7. Non-Applicability Notes

**COMPL (Regulatory & Privacy Compliance)**: Not applicable. The REST client transmits the same opaque UUIDs and numeric values as the in-process path. Bearer tokens are short-lived and not stored. No regulated personal data is introduced by this crate.

**UX (User Experience & Accessibility)**: Not applicable. This feature provides a Rust library crate and a machine-to-machine HTTP client. There is no user-facing UI, no end-user interaction, and no accessibility requirements.

**State Management**: Not applicable. No new entity lifecycle or state machines are introduced. `UsageRecord.status` transitions remain owned by Feature 8 (Operator Operations).
