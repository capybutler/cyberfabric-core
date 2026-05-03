//! Plugin metrics output port.

/// Observable metrics port for the TimescaleDB storage plugin.
///
/// Implementations live in `src/infra/` (e.g. OpenTelemetry counters).
/// The domain client depends only on this trait for testability.
pub trait PluginMetrics: Send + Sync {
    fn record_ingestion_success(&self);
    fn record_ingestion_error(&self);
    fn record_ingestion_latency_ms(&self, elapsed_ms: f64);
    fn record_dedup(&self);
    fn record_schema_validation_error(&self);
    fn record_query_latency_ms(&self, query_type: &str, elapsed_ms: f64);
}

/// No-op metrics implementation for unit tests and fallback initialization.
pub struct NoopMetrics;

impl PluginMetrics for NoopMetrics {
    fn record_ingestion_success(&self) {}
    fn record_ingestion_error(&self) {}
    fn record_ingestion_latency_ms(&self, _elapsed_ms: f64) {}
    fn record_dedup(&self) {}
    fn record_schema_validation_error(&self) {}
    fn record_query_latency_ms(&self, _query_type: &str, _elapsed_ms: f64) {}
}
