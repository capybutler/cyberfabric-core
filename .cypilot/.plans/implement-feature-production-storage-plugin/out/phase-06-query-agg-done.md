# Phase 06 — query_aggregated — Output Report

## Status: PASS

## Function signature implemented

```rust
async fn query_aggregated(
    &self,
    query: AggregationQuery,
) -> Result<Vec<AggregationResult>, UsageCollectorError>
```

## Checklist

- [x] Both routing branches present: `use_raw_path` (raw hypertable `usage_records`) and cagg path (`usage_agg_1h`)
- [x] AVG formula on cagg path: `(SUM(sum_val) / NULLIF(SUM(cnt_val), 0))::float8 AS agg_value`
- [x] `scope_to_sql` called first; returns `UsageCollectorError::authorization_failed("scope translation failed — access denied")` on failure
- [x] All 7 `inst-qagg-*` markers placed:
  - [x] inst-qagg-1 — scope translation via `scope_to_sql`
  - [x] inst-qagg-2 — routing decision with `tracing::debug!`
  - [x] inst-qagg-3 — raw hypertable SQL building
  - [x] inst-qagg-4 — cagg SQL building
  - [x] inst-qagg-5 — execute query with `sqlx::query_with`
  - [x] inst-qagg-6 — row mapping to `Vec<AggregationResult>`
  - [x] inst-qagg-7 — `Ok(results)`
- [x] `self.metrics.record_query_latency_ms("aggregated", elapsed_ms)` recorded before return
- [x] `PluginMetrics` trait extended with `record_query_latency_ms(&self, query_type: &str, elapsed_ms: f64)`
- [x] `NoopMetrics` impl updated with no-op for new method
- [x] Helper functions added: `raw_agg_expr`, `cagg_agg_expr`, `bucket_size_to_pg_interval`
- [x] Dynamic SQL built with `PgArguments` / `sqlx::Arguments`
- [x] `cargo check -p timescaledb-usage-collector-storage-plugin` exits 0 (warnings only, no errors)
