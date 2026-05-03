//! TimescaleDB storage plugin client.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use usage_collector_sdk::models::{
    AggregationFn, AggregationQuery, AggregationResult, BucketSize, Cursor, GroupByDimension,
    PagedResult, RawQuery, UsageKind, UsageRecord,
};
use usage_collector_sdk::{UsageCollectorError, UsageCollectorPluginClientV1};
use uuid::Uuid;

use crate::domain::insert_port::InsertPort;
use crate::domain::metrics::PluginMetrics;
use crate::domain::scope::{scope_to_sql, SqlValue};

/// Storage plugin client backed by a TimescaleDB connection pool.
pub struct TimescaleDbPluginClient {
    insert_port: Arc<dyn InsertPort>,
    pool: PgPool,
    metrics: Arc<dyn PluginMetrics>,
}

impl TimescaleDbPluginClient {
    /// Creates a new client wrapping the given insert port, connection pool, and metrics port.
    pub fn new(
        insert_port: Arc<dyn InsertPort>,
        pool: PgPool,
        metrics: Arc<dyn PluginMetrics>,
    ) -> Self {
        Self {
            insert_port,
            pool,
            metrics,
        }
    }
}

fn is_transient_error(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::PoolTimedOut => true,
        sqlx::Error::PoolClosed => true,
        sqlx::Error::Io(_) => true,
        sqlx::Error::Database(db_err) => matches!(
            db_err.code().as_deref(),
            Some("40001" | "40P01" | "57P03" | "53300" | "08006" | "08001")
        ),
        _ => false,
    }
}

#[async_trait]
impl UsageCollectorPluginClientV1 for TimescaleDbPluginClient {
    // @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1
    // @cpt-flow:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1
    async fn create_usage_record(&self, record: UsageRecord) -> Result<(), UsageCollectorError> {
        let start = Instant::now();

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-1
        // Plugin entry point; called by the gateway when delegating record storage.
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-1

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-2
        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-1
        if record.kind == UsageKind::Counter && record.value < 0.0 {
            self.metrics.record_schema_validation_error();
            return Err(UsageCollectorError::internal(
                "invalid record: counter value must be >= 0",
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-1

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-2
        if record.kind == UsageKind::Counter && record.idempotency_key.is_empty() {
            self.metrics.record_schema_validation_error();
            return Err(UsageCollectorError::internal(
                "invalid record: idempotency_key required for counter records",
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-2
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-2

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-3
        let result = self.insert_port.insert_usage_record(&record).await;
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-3

        let rows_affected = match result {
            Ok(n) => n,
            Err(e) => {
                // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-4
                if let sqlx::Error::Database(ref db_err) = e {
                    if db_err.code().as_deref() == Some("23505") {
                        self.metrics.record_ingestion_error();
                        return Err(UsageCollectorError::internal(format!(
                            "unexpected unique constraint violation: {db_err}"
                        )));
                    }
                }
                // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-4

                // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-4
                // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-5
                if is_transient_error(&e) {
                    self.metrics.record_ingestion_error();
                    return Err(UsageCollectorError::unavailable(format!(
                        "transient error: {e}"
                    )));
                }
                // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-5
                // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-4

                self.metrics.record_ingestion_error();
                return Err(UsageCollectorError::internal(format!("storage error: {e}")));
            }
        };

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        if rows_affected == 0 {
            self.metrics.record_dedup();
        }
        self.metrics.record_ingestion_latency_ms(elapsed_ms);
        self.metrics.record_ingestion_success();

        // @cpt-begin:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-5
        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-7
        Ok(())
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-create-usage-record:p1:inst-cur-7
        // @cpt-end:cpt-cf-usage-collector-flow-production-storage-plugin-storage-backend-ingest:p1:inst-flow-ing-5
    }

    // @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1
    async fn query_aggregated(
        &self,
        query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        use sqlx::postgres::PgArguments;
        use sqlx::Arguments as _;
        use sqlx::Row as _;

        let start = Instant::now();

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-1
        let (scope_sql, scope_params) = scope_to_sql(&query.scope)
            .map_err(|_| UsageCollectorError::authorization_failed("scope translation failed — access denied"))?;
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-1

        let group_by = &query.group_by;

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-2
        let use_raw_path = query.resource_id.is_some()
            || query.subject_id.is_some()
            || group_by.contains(&GroupByDimension::Resource)
            || group_by.contains(&GroupByDimension::Subject);
        tracing::debug!(use_raw_path, "query_aggregated routing decision");
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-2

        let mut args = PgArguments::default();
        let mut param_idx: usize = 0;

        // Bind scope params first
        for sv in &scope_params {
            param_idx += 1;
            match sv {
                SqlValue::Uuid(u) => { let _ = args.add(*u); }
                SqlValue::UuidArray(v) => { let _ = args.add(v.clone()); }
                SqlValue::Text(s) => { let _ = args.add(s.clone()); }
                SqlValue::TextArray(v) => { let _ = args.add(v.clone()); }
            }
        }

        // Time range params
        let time_start_idx = param_idx + 1;
        param_idx += 1;
        let _ = args.add(query.time_range.0);
        let time_end_idx = param_idx + 1;
        param_idx += 1;
        let _ = args.add(query.time_range.1);

        let has_time_bucket = group_by.iter().any(|d| matches!(d, GroupByDimension::TimeBucket(_)));
        let has_usage_type = group_by.contains(&GroupByDimension::UsageType);
        let has_subject = group_by.contains(&GroupByDimension::Subject);
        let has_resource = group_by.contains(&GroupByDimension::Resource);
        let has_source = group_by.contains(&GroupByDimension::Source);

        let sql;

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-3
        if use_raw_path {
            let time_col = "timestamp";
            let agg_expr = raw_agg_expr(query.function);

            let mut select_cols: Vec<String> = Vec::new();
            let mut group_by_exprs: Vec<String> = Vec::new();

            if has_time_bucket {
                if let Some(GroupByDimension::TimeBucket(bs)) = group_by.iter().find(|d| matches!(d, GroupByDimension::TimeBucket(_))) {
                    let interval = bucket_size_to_pg_interval(*bs);
                    select_cols.push(format!("time_bucket('{}', {}) AS bucket_start", interval, time_col));
                    group_by_exprs.push(format!("time_bucket('{}', {})", interval, time_col));
                }
            }
            if has_usage_type {
                select_cols.push("metric AS usage_type".to_string());
                group_by_exprs.push("metric".to_string());
            }
            if has_subject {
                select_cols.push("subject_id".to_string());
                select_cols.push("subject_type".to_string());
                group_by_exprs.push("subject_id".to_string());
                group_by_exprs.push("subject_type".to_string());
            }
            if has_resource {
                select_cols.push("resource_id".to_string());
                select_cols.push("resource_type".to_string());
                group_by_exprs.push("resource_id".to_string());
                group_by_exprs.push("resource_type".to_string());
            }
            if has_source {
                select_cols.push("module AS source".to_string());
                group_by_exprs.push("module".to_string());
            }
            select_cols.push(agg_expr.to_string());

            let select_clause = select_cols.join(", ");

            let mut where_clauses: Vec<String> = Vec::new();
            where_clauses.push(scope_sql.clone());
            where_clauses.push(format!("{} >= ${}", time_col, time_start_idx));
            where_clauses.push(format!("{} <= ${}", time_col, time_end_idx));

            if let Some(ref metric) = query.usage_type {
                param_idx += 1;
                where_clauses.push(format!("metric = ${}", param_idx));
                let _ = args.add(metric.clone());
            }
            if let Some(resource_id) = query.resource_id {
                param_idx += 1;
                where_clauses.push(format!("resource_id = ${}", param_idx));
                let _ = args.add(resource_id);
            }
            if let Some(ref resource_type) = query.resource_type {
                param_idx += 1;
                where_clauses.push(format!("resource_type = ${}", param_idx));
                let _ = args.add(resource_type.clone());
            }
            if let Some(subject_id) = query.subject_id {
                param_idx += 1;
                where_clauses.push(format!("subject_id = ${}", param_idx));
                let _ = args.add(subject_id);
            }
            if let Some(ref subject_type) = query.subject_type {
                param_idx += 1;
                where_clauses.push(format!("subject_type = ${}", param_idx));
                let _ = args.add(subject_type.clone());
            }
            if let Some(ref source) = query.source {
                param_idx += 1;
                where_clauses.push(format!("module = ${}", param_idx));
                let _ = args.add(source.clone());
            }

            let where_clause = where_clauses.join(" AND ");

            let limit_idx = param_idx + 1;
            let _ = args.add(query.max_rows as i64);

            let order_clause = if has_time_bucket { " ORDER BY bucket_start ASC" } else { "" };
            let group_clause = if !group_by_exprs.is_empty() {
                format!(" GROUP BY {}", group_by_exprs.join(", "))
            } else {
                String::new()
            };

            sql = format!(
                "SELECT {} FROM usage_records WHERE {}{}{} LIMIT ${}",
                select_clause, where_clause, group_clause, order_clause, limit_idx
            );
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-3

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-4
        } else {
            let time_col = "bucket";
            let agg_expr = cagg_agg_expr(query.function);

            let mut select_cols: Vec<String> = Vec::new();
            let mut group_by_exprs: Vec<String> = Vec::new();

            if has_time_bucket {
                if let Some(GroupByDimension::TimeBucket(bs)) = group_by.iter().find(|d| matches!(d, GroupByDimension::TimeBucket(_))) {
                    let interval = bucket_size_to_pg_interval(*bs);
                    select_cols.push(format!("time_bucket('{}', {}) AS bucket_start", interval, time_col));
                    group_by_exprs.push(format!("time_bucket('{}', {})", interval, time_col));
                }
            }
            if has_usage_type {
                select_cols.push("metric AS usage_type".to_string());
                group_by_exprs.push("metric".to_string());
            }
            if has_subject {
                select_cols.push("subject_type".to_string());
                group_by_exprs.push("subject_type".to_string());
            }
            if has_resource {
                select_cols.push("resource_type".to_string());
                group_by_exprs.push("resource_type".to_string());
            }
            if has_source {
                select_cols.push("module AS source".to_string());
                group_by_exprs.push("module".to_string());
            }
            select_cols.push(agg_expr.to_string());

            let select_clause = select_cols.join(", ");

            let mut where_clauses: Vec<String> = Vec::new();
            where_clauses.push(scope_sql.clone());
            where_clauses.push(format!("{} >= ${}", time_col, time_start_idx));
            where_clauses.push(format!("{} <= ${}", time_col, time_end_idx));

            if let Some(ref metric) = query.usage_type {
                param_idx += 1;
                where_clauses.push(format!("metric = ${}", param_idx));
                let _ = args.add(metric.clone());
            }
            if let Some(ref resource_type) = query.resource_type {
                param_idx += 1;
                where_clauses.push(format!("resource_type = ${}", param_idx));
                let _ = args.add(resource_type.clone());
            }
            if let Some(ref subject_type) = query.subject_type {
                param_idx += 1;
                where_clauses.push(format!("subject_type = ${}", param_idx));
                let _ = args.add(subject_type.clone());
            }
            if let Some(ref source) = query.source {
                param_idx += 1;
                where_clauses.push(format!("module = ${}", param_idx));
                let _ = args.add(source.clone());
            }

            let where_clause = where_clauses.join(" AND ");

            let limit_idx = param_idx + 1;
            let _ = args.add(query.max_rows as i64);

            let order_clause = if has_time_bucket { " ORDER BY bucket_start ASC" } else { "" };
            let group_clause = if !group_by_exprs.is_empty() {
                format!(" GROUP BY {}", group_by_exprs.join(", "))
            } else {
                String::new()
            };

            sql = format!(
                "SELECT {} FROM usage_agg_1h WHERE {}{}{} LIMIT ${}",
                select_clause, where_clause, group_clause, order_clause, limit_idx
            );
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-4

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-5
        let rows = sqlx::query_with(&sql, args)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                if is_transient_error(&e) {
                    UsageCollectorError::unavailable(format!("transient error: {e}"))
                } else {
                    UsageCollectorError::internal(format!("storage error: {e}"))
                }
            })?;
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-5

        // Return an error if the result was truncated at max_rows, signaling the query is too broad.
        if rows.len() == query.max_rows {
            return Err(UsageCollectorError::query_result_too_large(rows.len(), query.max_rows));
        }

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-6
        let results: Vec<AggregationResult> = rows
            .iter()
            .filter_map(|row| {
                // Skip rows where agg_value is NULL (e.g. AVG over an empty partition).
                let value = row.try_get::<f64, _>("agg_value").ok()?;
                let bucket_start = if has_time_bucket { row.try_get("bucket_start").ok() } else { None };
                let usage_type = if has_usage_type { row.try_get("usage_type").ok() } else { None };
                let subject_id = if has_subject && use_raw_path { row.try_get("subject_id").ok() } else { None };
                let subject_type = if has_subject { row.try_get("subject_type").ok() } else { None };
                let resource_id = if has_resource && use_raw_path { row.try_get("resource_id").ok() } else { None };
                let resource_type = if has_resource { row.try_get("resource_type").ok() } else { None };
                let source = if has_source { row.try_get("source").ok() } else { None };
                Some(AggregationResult {
                    function: query.function,
                    value,
                    bucket_start,
                    usage_type,
                    subject_id,
                    subject_type,
                    resource_id,
                    resource_type,
                    source,
                })
            })
            .collect();
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-6

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.metrics.record_query_latency_ms("aggregated", elapsed_ms);

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-7
        Ok(results)
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-aggregated:p1:inst-qagg-7
    }

    // @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1
    async fn query_raw(
        &self,
        query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        use sqlx::postgres::PgArguments;
        use sqlx::Arguments as _;
        use sqlx::Row as _;

        let start = Instant::now();

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-1
        let (scope_sql, scope_params) = scope_to_sql(&query.scope)
            .map_err(|_| UsageCollectorError::authorization_failed("scope translation failed — access denied"))?;
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-1

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-2
        // The Cursor type decodes base64 on Deserialize; extract (timestamp, id) when present.
        let cursor_pos: Option<(DateTime<Utc>, Uuid)> = query.cursor.as_ref().map(|c| (c.timestamp, c.id));
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-2

        let mut args = PgArguments::default();
        let mut param_idx: usize = 0;

        for sv in &scope_params {
            param_idx += 1;
            match sv {
                SqlValue::Uuid(u) => { let _ = args.add(*u); }
                SqlValue::UuidArray(v) => { let _ = args.add(v.clone()); }
                SqlValue::Text(s) => { let _ = args.add(s.clone()); }
                SqlValue::TextArray(v) => { let _ = args.add(v.clone()); }
            }
        }

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-3
        let time_start_idx = param_idx + 1;
        param_idx += 1;
        let _ = args.add(query.time_range.0);
        let time_end_idx = param_idx + 1;
        param_idx += 1;
        let _ = args.add(query.time_range.1);

        let mut where_clauses: Vec<String> = Vec::new();
        where_clauses.push(scope_sql);
        where_clauses.push(format!("timestamp >= ${time_start_idx}"));
        where_clauses.push(format!("timestamp <= ${time_end_idx}"));

        if let Some(ref metric) = query.usage_type {
            param_idx += 1;
            where_clauses.push(format!("metric = ${param_idx}"));
            let _ = args.add(metric.clone());
        }
        if let Some(resource_id) = query.resource_id {
            param_idx += 1;
            where_clauses.push(format!("resource_id = ${param_idx}"));
            let _ = args.add(resource_id);
        }
        if let Some(ref resource_type) = query.resource_type {
            param_idx += 1;
            where_clauses.push(format!("resource_type = ${param_idx}"));
            let _ = args.add(resource_type.clone());
        }
        if let Some(subject_id) = query.subject_id {
            param_idx += 1;
            where_clauses.push(format!("subject_id = ${param_idx}"));
            let _ = args.add(subject_id);
        }
        if let Some(ref subject_type) = query.subject_type {
            param_idx += 1;
            where_clauses.push(format!("subject_type = ${param_idx}"));
            let _ = args.add(subject_type.clone());
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-3

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-4
        if let Some((cursor_ts, cursor_id)) = cursor_pos {
            let ts_idx = param_idx + 1;
            param_idx += 1;
            let id_idx = param_idx + 1;
            param_idx += 1;
            let _ = args.add(cursor_ts);
            let _ = args.add(cursor_id);
            where_clauses.push(format!(
                "(timestamp > ${ts_idx} OR (timestamp = ${ts_idx} AND id > ${id_idx}))"
            ));
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-4

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-5
        let page_size_idx = param_idx + 1;
        let _ = args.add(query.page_size as i64);

        let where_clause = where_clauses.join(" AND ");
        let sql = format!(
            "SELECT id, tenant_id, module, kind, metric, value::float8 AS value, timestamp, \
             idempotency_key, resource_id, resource_type, subject_id, subject_type, metadata \
             FROM usage_records \
             WHERE {where_clause} \
             ORDER BY timestamp ASC, id ASC \
             LIMIT ${page_size_idx}"
        );
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-5

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-6
        let rows = sqlx::query_with(&sql, args)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                if is_transient_error(&e) {
                    UsageCollectorError::unavailable(format!("transient error: {e}"))
                } else {
                    UsageCollectorError::internal(format!("storage error: {e}"))
                }
            })?;
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-6

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-7
        let next_cursor = if rows.len() == query.page_size {
            if let Some(last_row) = rows.last() {
                let last_ts: DateTime<Utc> = last_row
                    .try_get("timestamp")
                    .map_err(|e| UsageCollectorError::internal(format!("cursor extraction error: {e}")))?;
                let last_id: Uuid = last_row
                    .try_get("id")
                    .map_err(|e| UsageCollectorError::internal(format!("cursor extraction error: {e}")))?;
                Some(Cursor { timestamp: last_ts, id: last_id })
            } else {
                None
            }
        } else {
            None
        };

        let records: Vec<UsageRecord> = rows
            .iter()
            .map(|row| -> Result<UsageRecord, UsageCollectorError> {
                let kind_str: String = row
                    .try_get("kind")
                    .map_err(|e| UsageCollectorError::internal(format!("row decode error (kind): {e}")))?;
                let kind = match kind_str.as_str() {
                    "counter" => UsageKind::Counter,
                    "gauge" => UsageKind::Gauge,
                    other => return Err(UsageCollectorError::internal(format!("unknown kind value in storage: {other}"))),
                };
                Ok(UsageRecord {
                    module: row
                        .try_get("module")
                        .map_err(|e| UsageCollectorError::internal(format!("row decode error (module): {e}")))?,
                    tenant_id: row
                        .try_get("tenant_id")
                        .map_err(|e| UsageCollectorError::internal(format!("row decode error (tenant_id): {e}")))?,
                    metric: row
                        .try_get("metric")
                        .map_err(|e| UsageCollectorError::internal(format!("row decode error (metric): {e}")))?,
                    kind,
                    value: row
                        .try_get::<f64, _>("value")
                        .map_err(|e| UsageCollectorError::internal(format!("row decode error (value): {e}")))?,
                    resource_id: row.try_get("resource_id").unwrap_or_default(),
                    resource_type: row.try_get("resource_type").unwrap_or_default(),
                    subject_id: row.try_get::<Option<Uuid>, _>("subject_id").unwrap_or(None),
                    subject_type: row.try_get::<Option<String>, _>("subject_type").unwrap_or(None),
                    idempotency_key: row
                        .try_get::<Option<String>, _>("idempotency_key")
                        .unwrap_or(None)
                        .unwrap_or_default(),
                    timestamp: row
                        .try_get("timestamp")
                        .map_err(|e| UsageCollectorError::internal(format!("row decode error (timestamp): {e}")))?,
                    metadata: row
                        .try_get::<Option<serde_json::Value>, _>("metadata")
                        .unwrap_or(None),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-7

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.metrics.record_query_latency_ms("raw", elapsed_ms);

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-8
        Ok(PagedResult {
            items: records,
            next_cursor,
        })
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-query-raw:p1:inst-qraw-8
    }
}

fn raw_agg_expr(func: AggregationFn) -> &'static str {
    match func {
        AggregationFn::Sum => "SUM(value)::float8 AS agg_value",
        AggregationFn::Count => "COUNT(*)::float8 AS agg_value",
        AggregationFn::Min => "MIN(value)::float8 AS agg_value",
        AggregationFn::Max => "MAX(value)::float8 AS agg_value",
        AggregationFn::Avg => "AVG(value)::float8 AS agg_value",
    }
}

fn cagg_agg_expr(func: AggregationFn) -> &'static str {
    match func {
        AggregationFn::Sum => "SUM(sum_val)::float8 AS agg_value",
        AggregationFn::Count => "SUM(cnt_val)::float8 AS agg_value",
        AggregationFn::Min => "MIN(min_val)::float8 AS agg_value",
        AggregationFn::Max => "MAX(max_val)::float8 AS agg_value",
        AggregationFn::Avg => "(SUM(sum_val) / NULLIF(SUM(cnt_val), 0))::float8 AS agg_value",
    }
}

fn bucket_size_to_pg_interval(size: BucketSize) -> &'static str {
    match size {
        BucketSize::Minute => "1 minute",
        BucketSize::Hour => "1 hour",
        BucketSize::Day => "1 day",
        BucketSize::Week => "1 week",
        BucketSize::Month => "1 month",
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "client_tests.rs"]
mod client_tests;
