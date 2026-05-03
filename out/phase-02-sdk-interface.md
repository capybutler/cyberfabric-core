# Phase 02 – SDK Core Update: Changed Public Signatures

## Removed Types

| Type | Location | Reason |
|------|----------|--------|
| `Cursor` struct | `models.rs` | Replaced by `modkit_odata::CursorV1` |
| `CursorDecodeError` enum | `models.rs` | Replaced by `modkit_odata::Error` in plugin impls |
| `PagedResult<T>` struct | `models.rs` | Replaced by `modkit_odata::Page<T>` |
| `cursor_field` fn | `models.rs` | Internal helper; removed with `Cursor` |

## Changed Signatures

### `RawQuery.cursor` field (`models.rs`)

Before: `pub cursor: Option<Cursor>`
After:  `pub cursor: Option<modkit_odata::CursorV1>`

### `UsageCollectorPluginClientV1::query_raw` trait method (`plugin_api.rs`)

Before: `async fn query_raw(&self, query: RawQuery) -> Result<PagedResult<UsageRecord>, UsageCollectorError>`
After:  `async fn query_raw(&self, query: RawQuery) -> Result<modkit_odata::Page<UsageRecord>, UsageCollectorError>`

## New Re-exports (`lib.rs`)

```rust
pub use modkit_odata::{CursorV1, Page, PageInfo};
```

These replace the removed `Cursor`, `CursorDecodeError`, and `PagedResult` re-exports.

## New Dependency (`Cargo.toml`)

```toml
modkit-odata = { workspace = true }
```

Added to `[dependencies]` in `modules/system/usage-collector/usage-collector-sdk/Cargo.toml`.

## Import Paths for Downstream Phases

- `modkit_odata::CursorV1` – cursor type for `RawQuery.cursor` and plugin encode/decode
- `modkit_odata::Page<T>` – return type for `query_raw`; available via `usage_collector_sdk::Page`
- `modkit_odata::PageInfo` – page metadata; available via `usage_collector_sdk::PageInfo`
- `modkit_odata::SortDir` – used when constructing `CursorV1` (`SortDir::Asc`)

### CursorV1 construction (for plugin impls — Phases 3–4)

```rust
let cursor = CursorV1 {
    k: vec![timestamp.to_rfc3339(), id.to_string()],
    o: modkit_odata::SortDir::Asc,
    s: "+timestamp,+id".to_owned(),
    f: None,
    d: "fwd".to_owned(),
};
let encoded: String = cursor.encode().expect("CursorV1 encode is infallible for valid data");
```

Decode:
```rust
let cursor: CursorV1 = CursorV1::decode(&token_str).map_err(|e| UsageCollectorError::...)?;
```

### Page construction (for plugin impls — Phases 3–4)

```rust
modkit_odata::Page::new(
    records,
    modkit_odata::PageInfo { next_cursor: Some(encoded), prev_cursor: None, limit: query.page_size as u64 },
)
```

Empty page:
```rust
modkit_odata::Page::empty(query.page_size as u64)
```
