# Phase 5 Output: Ingest Write Path

## Status: PASS

## Function Signature

```rust
async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError>
```

Located in: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/client.rs`

## ON CONFLICT Target

The INSERT uses:

```sql
ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL
DO NOTHING
```

This targets the partial unique index `idx_usage_records_tenant_idempotency` created in phase 2 migrations.

Duplicate records (same `tenant_id` + `idempotency_key`) are silently ignored via DO NOTHING — not double-counted.

## ingested_at Approach

`ingested_at` is set via `NOW()` in the INSERT SQL — it is NOT populated from Rust-side code or passed from the caller.

The SQL contains: `ingested_at` in the column list with `NOW()` as the corresponding value.

The `NULLIF($7, '')` ensures empty idempotency_key strings (gauge records) are stored as NULL, so the partial unique index does not apply to them.

## Marker Pairs Placed

All markers use algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1`
and flow ID `cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1`:

Algo markers (inst-cur-*):
- `inst-cur-1` — counter value >= 0 validation guard
- `inst-cur-2` — counter idempotency_key presence check
- `inst-cur-3` — INSERT query execution
- `inst-cur-4` — unexpected unique constraint violation handler
- `inst-cur-5` — transient DB error handler
- `inst-cur-6` — ingested_at = NOW() SQL definition (insert_sql variable)
- `inst-cur-7` — Ok(()) return

Flow markers (inst-flow-ing-*):
- `inst-flow-ing-1` — plugin entry point comment
- `inst-flow-ing-2` — wraps the validation section (inst-cur-1 + inst-cur-2)
- `inst-flow-ing-3` — wraps the INSERT execution (inst-cur-3)
- `inst-flow-ing-4` — wraps the transient error return (inst-cur-5)
- `inst-flow-ing-5` — wraps the Ok(()) return (inst-cur-7)

Scope markers at function entry:
- `@cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1`
- `@cpt-flow:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1`

## Prometheus Metrics Incremented

- `usage_schema_validation_errors_total` — incremented on inst-cur-1 and inst-cur-2 validation failures
- `usage_ingestion_latency_ms` — recorded (elapsed_ms since function entry) on successful return
- `usage_dedup_total` — incremented when `rows_affected() == 0` (ON CONFLICT DO NOTHING fired)
- `usage_ingestion_total` (status=success) — incremented on every successful Ok(()) return

Metrics are injected via the `PluginMetrics` trait stored in `TimescaleDbPluginClient.metrics: Arc<dyn PluginMetrics>`.

## New Files Created

- `src/domain/metrics.rs` — `PluginMetrics` trait and `NoopMetrics` implementation

## Files Modified

- `src/domain/client.rs` — full `create_usage_record` implementation with all markers and metrics
- `src/domain/mod.rs` — added `pub mod metrics;`

## FEATURE Checkboxes Updated

- `inst-cur-1` through `inst-cur-7` marked `[x]` under `### create_usage_record — Idempotent Ingest`
- `inst-flow-ing-1` through `inst-flow-ing-5` marked `[x]` under `### Storage Backend: Ingest Record`
- Parent algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record` marked `[x]`
- Parent flow ID `cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest` marked `[x]`

## Acceptance Criteria Verification

- [x] `create_usage_record` fully implemented — no stubs
- [x] INSERT includes `ON CONFLICT (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING`
- [x] `ingested_at` set via `NOW()` in SQL — not from Rust-side caller
- [x] All 7 inst-cur-* have distinct begin/end marker pairs
- [x] Flow scope marker `@cpt-flow:...` present at function entry
- [x] All 4 metrics incremented at correct code paths
- [x] FEATURE checkboxes for inst-cur-1..7 and inst-flow-ing-1..5 marked [x]
- [x] Parent IDs both marked [x]
- [x] out/phase-05-ingest-done.md exists
- [x] No unresolved {variables} outside code fences
