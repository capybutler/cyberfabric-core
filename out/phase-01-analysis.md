# Phase 01 – Technical Analysis: ModKit Pagination Alignment

## (a) Cursor → CursorV1 Mapping

The bespoke `Cursor` struct holds two fields: `timestamp: DateTime<Utc>` and `id: Uuid`.
It serializes to base64-encoded `"timestamp=<RFC3339>&id=<UUID>"` (standard base64, STANDARD engine).

`CursorV1` from `modkit_odata` is a generic keyset cursor:
- `k: Vec<String>` – ordered list of key values at the page boundary
- `o: SortDir` – sort direction of the primary key (`Asc` or `Desc`)
- `s: String` – signed token representation of the full sort order, e.g. `"+timestamp,+id"`
- `f: Option<String>` – optional filter hash for consistency validation
- `d: String` – pagination direction: `"fwd"` (forward) or `"bwd"` (backward)

Serialization: base64url-no-padding of a JSON `{"v":1,"k":[...],"o":"asc","s":"...","d":"fwd"}` payload.
Encoding returns `serde_json::Result<String>` (fallible). Decoding returns `Result<CursorV1, modkit_odata::Error>`.

**Mapping for usage records (timestamp + id keyset, forward-only):**

| CursorV1 field | Value for usage-collector                      |
|---------------|------------------------------------------------|
| `k`           | `[timestamp.to_rfc3339(), id.to_string()]`     |
| `o`           | `SortDir::Asc`                                 |
| `s`           | `"+timestamp,+id"`                             |
| `f`           | `None` (no filter-hash validation required)    |
| `d`           | `"fwd"`                                        |

`CursorDecodeError` is replaced by `modkit_odata::Error` (variants: `CursorInvalidBase64`,
`CursorInvalidJson`, `CursorInvalidVersion`, `CursorInvalidKeys`, `CursorInvalidFields`, `CursorInvalidDirection`).

The `cursor_field` helper and `Serialize`/`Deserialize` impls on `Cursor` are removed.
`RawQuery.cursor` changes from `Option<Cursor>` to `Option<modkit_odata::CursorV1>`.

## (b) PagedResult\<T\> → Page\<T\> Mapping

| `PagedResult<T>` field | `Page<T>` equivalent              | Notes                                       |
|------------------------|-----------------------------------|---------------------------------------------|
| `items: Vec<T>`        | `items: Vec<T>`                   | Identical                                   |
| `next_cursor: Option<Cursor>` | `page_info.next_cursor: Option<String>` | Moves into nested `PageInfo`; type is now an opaque `String` |
| *(absent)*             | `page_info.prev_cursor: Option<String>` | Always `None` for forward-only pagination   |
| *(absent)*             | `page_info.limit: u64`            | Set to `query.page_size as u64`             |

`PageInfo` struct: `{ next_cursor: Option<String>, prev_cursor: Option<String>, limit: u64 }`.

Construction in plugin implementations:
```rust
Page::new(
    records,
    PageInfo { next_cursor: cursor_str, prev_cursor: None, limit: query.page_size as u64 },
)
```

Empty page: `Page::empty(query.page_size as u64)`.

## (c) ODataQuery Fit for RawQuery – Recommendation: Augment

`ODataQuery` fields: `filter: Option<Box<ast::Expr>>`, `order: ODataOrderBy`, `limit: Option<u64>`,
`cursor: Option<CursorV1>`, `filter_hash: Option<String>`, `select: Option<Vec<String>>`.

`RawQuery` has domain-specific strongly-typed fields (`scope: AccessScope`, `time_range`,
`usage_type`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `page_size`) that
have no equivalent in `ODataQuery`'s generic filter AST. Replacing `RawQuery` with `ODataQuery`
would require encoding all filter semantics as an AST expression, losing type safety and requiring
plugin implementations to parse AST nodes — a significant behavioural change.

**Decision: Augment.** Keep `RawQuery` struct unchanged except replace `cursor: Option<Cursor>` with
`cursor: Option<modkit_odata::CursorV1>`. This is the minimal change required to eliminate the
bespoke cursor type while preserving all domain semantics.

## (d) ODataQuery Fit for AggregationQuery – Recommendation: Skip

`AggregationQuery` is a non-paginated, aggregation-specific query type with fields `function: AggregationFn`,
`group_by: Vec<GroupByDimension>`, `bucket_size: Option<BucketSize>`, `max_rows: usize`, and domain filters.
`ODataQuery` has no aggregation primitives (no group-by, no aggregation function, no bucket concept).
There is also no cursor in `AggregationQuery`, so no alignment work is needed.

**Decision: Skip.** `AggregationQuery` requires no changes in this alignment task.

## (e) Required modkit-odata Types and Cargo Dependency

Types required from `modkit_odata`:
- `modkit_odata::CursorV1` — replaces bespoke `Cursor` in `RawQuery.cursor` and plugin encode/decode
- `modkit_odata::Page<T>` — replaces `PagedResult<T>` in `plugin_api.rs` return type and plugin impls
- `modkit_odata::PageInfo` — used when constructing `Page::new(items, PageInfo { ... })`
- `modkit_odata::Error` — cursor decode errors (used internally in plugin impls; not surfaced in public API)

**New Cargo dependency required:** `modkit-odata = { workspace = true }` must be added to
`modules/system/usage-collector/usage-collector-sdk/Cargo.toml` `[dependencies]`.

`modkit-odata` is already declared in the workspace root `Cargo.toml` as:
```toml
modkit-odata = { package = "cf-modkit-odata", version = "0.7.1", path = "libs/modkit-odata" }
```
so only the per-crate `[dependencies]` entry needs to be added (no workspace-level changes required).
