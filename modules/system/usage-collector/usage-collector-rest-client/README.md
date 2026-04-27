# Usage Collector REST Client

> **Separate-binary bridge** — builds a `UsageCollectorRestClient` that forwards `create_usage_record` and `get_module_config` calls to a remote usage-collector REST API, authenticated with a bearer token from `AuthNResolverClient::exchange_client_credentials`.

ModKit module `usage-collector-rest-client`: builds [`UsageCollectorRestClient`](src/infra/rest_client.rs) at init, resolves `dyn AuthNResolverClient` and `dyn AuthZResolverClient` from `ClientHub`, then wires it into `UsageEmitter` (which registers `dyn UsageEmitterV1`). The module also implements `DatabaseCapability` and provides outbox migrations required by `UsageEmitter`.

## Dependencies

- The remote binary must expose the usage-collector REST API at `base_url`.

## Configuration

`client_id` and `client_secret` are **required** (they have no defaults). Optional fields:

- `base_url` — default `http://127.0.0.1:8080`
- `scopes` — default `[]` (IdP default scopes)
- `request_timeout` — default `30s`
- `emitter` — nested [`UsageEmitterConfig`](../usage-emitter/src/config.rs); all fields are optional:
  - `authorization_max_age` — default `30s`
  - `outbox_queue` — default `"usage-records"`
  - `outbox_partition_count` — default `4` (must be a power of 2 in 1–64)

```yaml
modules:
  authn-resolver:
    # ... your AuthN resolver / plugin config
  usage-collector-rest-client:
    config:
      # required
      client_id: "my-service"
      client_secret: "${MY_SERVICE_SECRET}"
      # optional
      base_url: "http://collector.internal:8080"
      scopes: ["usage-collector:write"]
      request_timeout: "30s"
      emitter:
        authorization_max_age: "30s"
        outbox_queue: "usage-records"
        outbox_partition_count: 4
```

## Testing

```bash
cargo test -p cf-usage-collector-rest-client
```
