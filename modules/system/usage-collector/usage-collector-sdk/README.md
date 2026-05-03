# Usage Collector SDK

Transport-agnostic contracts for the usage-collector module family.

## What this crate provides

| Item | Description |
|------|-------------|
| `UsageCollectorClientV1` | Ingest trait implemented by client modules (`usage-collector`, `usage-collector-rest-client`). **Never registered in `ClientHub`** to prevent unauthorized usage emission. |
| `UsageCollectorPluginClientV1` | Storage-plugin trait implemented by backend plugins. |
| `UsageRecord`, `UsageKind` | Transport-agnostic models (`UsageRecord` fields are public for direct construction, serde, and tests). |
| `ModuleConfig`, `AllowedMetric` | Per-module configuration returned by `get_module_config()`; `AllowedMetric` holds a metric name and its `UsageKind`. |
| `UsageCollectorError` | Error type shared by both traits. |
| `UsageCollectorStoragePluginSpecV1` | GTS schema for storage plugin registration. |

> **Emitting usage** — use the `usage-emitter` crate, which wraps `UsageCollectorClientV1` with PDP authorization and outbox buffering.

## Usage

### Querying module config

Source modules can fetch their allowed metrics at init time via `UsageCollectorClientV1`:

```rust
use usage_collector_sdk::UsageCollectorClientV1;

let config = collector.get_module_config("my_module").await?;
for metric in &config.allowed_metrics {
    println!("{}: {:?}", metric.name, metric.kind);
}
```

### Building a `UsageRecord` directly

For tests, plugins, or offline construction, set fields on `UsageRecord` directly (public struct fields):

```rust
use chrono::Utc;
use usage_collector_sdk::{UsageKind, UsageRecord};
use uuid::Uuid;

let record = UsageRecord {
    module: "my_module".to_owned(),
    tenant_id: Uuid::new_v4(),
    metric: "requests".to_owned(),
    kind: UsageKind::Counter,
    value: 1.0,
    resource_id: Uuid::new_v4(),
    resource_type: "resource_type".to_owned(),
    subject_id: Some(Uuid::new_v4()),
    subject_type: Some("subject_type".to_owned()),
    idempotency_key: Uuid::new_v4().to_string(),
    timestamp: Utc::now(),
    metadata: None,
};
```

### Implementing a storage plugin

```rust
use async_trait::async_trait;
use usage_collector_sdk::{UsageCollectorError, UsageCollectorPluginClientV1, UsageRecord};

struct MyStoragePlugin { /* ... */ }

#[async_trait]
impl UsageCollectorPluginClientV1 for MyStoragePlugin {
    async fn create_usage_record(
        &self,
        record: UsageRecord,
    ) -> Result<(), UsageCollectorError> {
        // idempotent upsert keyed on record.idempotency_key
        todo!()
    }
}
```

## Error handling

```rust
use usage_collector_sdk::UsageCollectorError;

match result {
    Ok(()) => {}
    Err(UsageCollectorError::AuthorizationFailed { message }) => { /* PDP denied */ }
    Err(UsageCollectorError::ModuleNotFound { module_name }) => { /* module has no configured metrics */ }
    Err(UsageCollectorError::PluginTimeout) => { /* storage plugin timed out */ }
    Err(UsageCollectorError::CircuitOpen) => { /* storage plugin circuit breaker open */ }
    Err(UsageCollectorError::Internal { message }) => { /* unexpected error */ }
}
```

## Security invariant

`UsageCollectorClientV1` is **never** registered in `ClientHub`. It is passed directly to the emitter via constructor, ensuring the sole path to the collector is through a PDP-authorized emitter.
