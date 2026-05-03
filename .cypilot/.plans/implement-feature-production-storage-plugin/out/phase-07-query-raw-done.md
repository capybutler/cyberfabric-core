# Phase 07 — query_raw — Output Report

## Status: PASS

## Function signature implemented

```rust
// @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1
async fn query_raw(
    &self,
    query: RawQuery,
) -> Result<PagedResult<UsageRecord>, UsageCollectorError>
```

## All 3 query operations now implemented in client.rs

- [x] `create_usage_record` — implemented in phase 5
- [x] `query_aggregated` — implemented in phase 6
- [x] `query_raw` — implemented in this phase

## Implementation checklist

- [x] `scope_to_sql` called first; returns `UsageCollectorError::authorization_failed(...)` on translation failure (inst-qraw-1)
- [x] Cursor extracted from `query.cursor` (already decoded by Cursor::Deserialize at HTTP layer); `(timestamp, id)` extracted when present (inst-qraw-2)
- [x] SELECT built against `usage_records` with scope fragment, time range, and optional user filters: `usage_type`, `resource_id`, `resource_type`, `subject_id`, `subject_type` (inst-qraw-3)
- [x] Keyset advancement condition appended when cursor present: `(timestamp > $ts OR (timestamp = $ts AND id > $id))` (inst-qraw-4)
- [x] `ORDER BY timestamp ASC, id ASC LIMIT $page_size` appended (inst-qraw-5)
- [x] `sqlx::query_with` used; transient errors → `UsageCollectorError::unavailable(...)` (inst-qraw-6)
- [x] Rows mapped to `Vec<UsageRecord>`; next cursor set from last row `(timestamp, id)` when `rows.len() == page_size`; `None` when page exhausted (inst-qraw-7)
- [x] Returns `Ok(PagedResult { items, next_cursor })` (inst-qraw-8)
- [x] `self.metrics.record_query_latency_ms("raw", elapsed_ms)` emitted before return
- [x] `value::float8 AS value` cast in SELECT to decode NUMERIC column as f64
- [x] All 8 `@cpt-begin`/`@cpt-end` marker pairs placed in client.rs
- [x] FEATURE checkboxes for inst-qraw-1 through inst-qraw-8 and parent algo ID all marked `[x]`
- [x] `cargo check -p timescaledb-usage-collector-storage-plugin` exits 0 (warnings only, no errors)
