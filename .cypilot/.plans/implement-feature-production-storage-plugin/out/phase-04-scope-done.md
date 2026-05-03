# Phase 4 Output: Scope Translator

## Status: PASS

## Function Signature

```rust
pub fn scope_to_sql(
    scope: &AccessScope,
) -> Result<(String, Vec<SqlValue>), ScopeTranslationError>
```

Located in: `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`

## SqlValue Type

Defined locally in `scope.rs` (not in SDK):

```rust
pub enum SqlValue {
    Uuid(Uuid),
    UuidArray(Vec<Uuid>),
    Text(String),
    TextArray(Vec<String>),
}
```

## ScopeTranslationError Variants

Defined in `src/domain/error.rs`:

- `EmptyScope` — returned when `scope.constraints().is_empty()` (covers both deny-all and allow-all)
- `UnsupportedPredicate { kind: String }` — returned for `InGroup`/`InGroupSubtree` filters, or unrecognised property names

## Files Created

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/scope.rs`

## Files Modified

- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/Cargo.toml` — added `modkit-security`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/error.rs` — added `ScopeTranslationError`
- `modules/system/usage-collector/plugins/timescaledb-usage-collector-storage-plugin/src/domain/mod.rs` — added `pub mod scope;`

## Marker Pairs Placed

All markers use algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1`:

- `inst-s2s-1` — empty-scope guard
- `inst-s2s-2` — initialization of `group_fragments` and `bind_params`
- `inst-s2s-3` — outer for-each loop over constraints
- `inst-s2s-3a` — initialization of `predicate_fragments` per group
- `inst-s2s-3b` — inner for-each loop over filters
- `inst-s2s-3b-i` — InGroup/InGroupSubtree hard error
- `inst-s2s-3b-ii` — tenant_id filter handling
- `inst-s2s-3b-iii` — resource_id filter handling
- `inst-s2s-3b-iv` — resource_type filter handling
- `inst-s2s-3c` — group fragment join and append
- `inst-s2s-4` — final fragment join
- `inst-s2s-5` — Ok return

## Property-to-Column Mapping

- `pep_properties::OWNER_TENANT_ID` ("owner_tenant_id") → `tenant_id` column, UUID values
- `"resource_id"` → `resource_id` column, UUID values
- `"resource_type"` → `resource_type` column, Text values

## Tests

8 unit tests in `scope.rs`, all passing:
- `test_scope_to_sql_empty_scope_fail_closed`
- `test_scope_to_sql_unconstrained_scope_fail_closed`
- `test_scope_to_sql_single_group`
- `test_scope_to_sql_multiple_groups_or_of_ands_preserved`
- `test_scope_to_sql_ingroup_predicate_rejection`
- `test_scope_to_sql_resource_id_filter`
- `test_scope_to_sql_resource_type_filter`
- `test_scope_to_sql_multi_predicate_and_within_group`

## FEATURE Checkboxes Updated

All `inst-s2s-*` steps (including sub-steps) marked `[x]` in the FEATURE spec.
Parent algo ID `cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql` NOT yet marked `[x]` — depends on all downstream phases completing.
