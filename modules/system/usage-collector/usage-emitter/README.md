# Usage Emitter

> **Emitter library** — two-phase PDP authorization, transactional outbox enqueue, and async delivery to the usage collector.

A plain library crate (no `#[modkit::module]`). Each host module (`usage-collector`, `usage-collector-rest-client`, future gRPC client) calls `UsageEmitter::build(config, db, authz, collector)` in its own `init()` and registers the result as `dyn UsageEmitterV1` in `ClientHub`.

## Security invariant

`UsageCollectorClientV1` is **never** registered in `ClientHub`. It is supplied to `UsageEmitter::build` as a constructor argument and stays private inside the emitter. Only `dyn UsageEmitterV1` is published to the hub, so the sole path a source module has to the collector is through a PDP-authorized, tenant/resource-bound `AuthorizedUsageEmitter::enqueue*`. The PDP resource type and action constants (`gts.x.core.usage.record.v1 / create`) are `pub(crate)` in this crate and cannot be referenced externally.

## API

| Item | Description |
|------|-------------|
| `UsageEmitterV1` | Source-facing trait. Obtain from `ClientHub`. Call `for_module(MODULE_NAME)` once (e.g. in `init()`) to get a `ScopedUsageEmitter`. |
| `UsageEmitter` | Concrete implementation; built with `UsageEmitter::build`. |
| `ScopedUsageEmitter` | Returned by `for_module`; carries the module name and knows the allowed metrics list. Call `authorize_for` or `authorize` to get a time-limited handle. |
| `AuthorizedUsageEmitter` | Time-limited handle returned by `authorize` / `authorize_for`. Call `enqueue`, `enqueue_in`, or `build_usage_record`. |
| `UsageRecordBuilder` | Returned by `AuthorizedUsageEmitter::build_usage_record(metric, value)`; tenant, resource, module, and kind come from the authorized handle. Optionally set `with_idempotency_key` / `with_timestamp`, then `enqueue` / `enqueue_in`. |
| `UsageEmitterConfig` | Tunable authorization TTL, outbox queue name, partition count. Embedded in the host module's own config struct. |
| `UsageEmitterError` | Typed errors for authorization, validation, and enqueue phases. |

## Emitting a usage record

```rust
use usage_emitter::UsageEmitterV1;

// In init(): obtain from ClientHub and scope to this module's name.
let emitter = hub.get::<dyn UsageEmitterV1>()?;
let scoped = emitter.for_module(Self::MODULE_NAME);

// In a handler — Phase 1: authorize (calls PDP + fetches allowed metrics;
// valid for UsageEmitterConfig::authorization_max_age).
let authorized = scoped
    .authorize_for(&ctx, tenant_id, resource_id, resource_type.clone())
    .await?;
// Or for the subject's home tenant: scoped.authorize(&ctx, resource_id, resource_type).await?

// Phase 2a: build and enqueue on the emitter's DB connection.
authorized
    .build_usage_record("requests", 1.0)
    .enqueue()
    .await?;

// Phase 2b: enqueue inside a caller transaction (atomic with your write).
// authorized.build_usage_record("requests", 1.0).enqueue_in(&txn).await?;

// Optional: set idempotency key or timestamp.
// authorized
//     .build_usage_record("requests", 1.0)
//     .with_idempotency_key("key123")
//     .with_timestamp(Utc::now())
//     .enqueue()
//     .await?;
```

## Configuration

`UsageEmitterConfig` is embedded in the host module's own config struct with `#[serde(default)]`, so all fields are optional in YAML:

```yaml
modules:
  usage-collector:         # or usage-collector-rest-client
    config:
      emitter:
        authorization_max_age: "30s"   # default
        outbox_queue: "usage-records"  # default
        outbox_partition_count: 4      # default; power of 2 in 1–64
```

| Field | Default | Description |
|-------|---------|-------------|
| `authorization_max_age` | `30s` | Maximum age of an `AuthorizedUsageEmitter` handle before `enqueue*` rejects it with `AuthorizationExpired` |
| `outbox_queue` | `usage-records` | Outbox queue name |
| `outbox_partition_count` | `4` | Partition count (power of 2 in 1–64) |

## Error handling

```rust
use usage_emitter::UsageEmitterError;

match authorized.enqueue(record).await {
    Ok(()) => {}
    Err(UsageEmitterError::AuthorizationExpired) => { /* re-authorize and retry */ }
    Err(UsageEmitterError::AuthorizationFailed { message }) => { /* PDP denied */ }
    Err(UsageEmitterError::MetricNotAllowed { metric }) => { /* metric not configured for this module */ }
    Err(UsageEmitterError::NegativeCounterValue { value }) => { /* counter delta must be >= 0 */ }
    Err(UsageEmitterError::InvalidRecord { message }) => { /* incomplete builder / record */ }
    Err(UsageEmitterError::Outbox(e)) => { /* transactional DB write failed */ }
    Err(UsageEmitterError::Internal { message }) => { /* unexpected PDP or runtime error */ }
}
```

## Background delivery

The outbox worker dequeues from `usage-records` and calls `UsageCollectorClientV1::create_usage_record` per message:

- **`Ok`** — message acknowledged
- **`PluginTimeout`** — transient; message is retried
- **Other errors** — permanent; message is dead-lettered

Delivery is independent of the request that enqueued the record and survives process restarts through the durable outbox.

## Testing

```bash
devbox run cargo test -p cf-usage-emitter
```
