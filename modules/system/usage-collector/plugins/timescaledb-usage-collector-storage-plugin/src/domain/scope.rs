//! `AccessScope` → SQL WHERE fragment translator.

use modkit_security::{AccessScope, ScopeFilter, pep_properties};
use uuid::Uuid;

use crate::domain::error::ScopeTranslationError;

/// Property name for the resource identifier used in usage records.
const PROP_RESOURCE_ID: &str = "resource_id";

/// Property name for the resource type used in usage records.
const PROP_RESOURCE_TYPE: &str = "resource_type";

/// A typed SQL bind parameter produced by [`scope_to_sql`].
///
/// Callers bind these values positionally to a sqlx query in the order they appear
/// in the returned `Vec<SqlValue>`.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Uuid(Uuid),
    UuidArray(Vec<Uuid>),
    Text(String),
    TextArray(Vec<String>),
}

// @cpt-algo:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1
/// Translate an `AccessScope` into a SQL WHERE fragment and positional bind parameters.
///
/// The returned fragment is ready to embed in `WHERE (<fragment>)`. Bind parameters
/// must be appended after any pre-existing parameters the caller already has.
///
/// # Errors
///
/// Returns [`ScopeTranslationError::EmptyScope`] when the scope has no constraints
/// (deny-all or allow-all — callers must fail closed in both cases).
///
/// Returns [`ScopeTranslationError::UnsupportedPredicate`] when the scope contains
/// `InGroup`/`InGroupSubtree` predicates or an unrecognised property name.
pub fn scope_to_sql(
    scope: &AccessScope,
) -> Result<(String, Vec<SqlValue>), ScopeTranslationError> {
    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-1
    if scope.constraints().is_empty() {
        return Err(ScopeTranslationError::EmptyScope);
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-1

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-2
    let mut group_fragments: Vec<String> = Vec::new();
    let mut bind_params: Vec<SqlValue> = Vec::new();
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-2

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3
    for constraint in scope.constraints() {
        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3a
        let mut predicate_fragments: Vec<String> = Vec::new();
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3a

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b
        for filter in constraint.filters() {
            // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-i
            if matches!(filter, ScopeFilter::InGroup(_) | ScopeFilter::InGroupSubtree(_)) {
                return Err(ScopeTranslationError::UnsupportedPredicate {
                    kind: "InGroup/InGroupSubtree".to_owned(),
                });
            }
            // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-i

            let param_n = bind_params.len() + 1;

            // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-ii
            if filter.property() == pep_properties::OWNER_TENANT_ID {
                let (frag, val) = build_uuid_filter(filter, param_n, "tenant_id")?;
                predicate_fragments.push(frag);
                bind_params.push(val);
            }
            // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-ii
            // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-iii
            else if filter.property() == PROP_RESOURCE_ID {
                let (frag, val) = build_uuid_filter(filter, param_n, "resource_id")?;
                predicate_fragments.push(frag);
                bind_params.push(val);
            }
            // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-iii
            // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-iv
            else if filter.property() == PROP_RESOURCE_TYPE {
                let (frag, val) = build_text_filter(filter, param_n, "resource_type");
                predicate_fragments.push(frag);
                bind_params.push(val);
            }
            // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b-iv
            else {
                return Err(ScopeTranslationError::UnsupportedPredicate {
                    kind: format!("unknown property: {}", filter.property()),
                });
            }
        }
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3b

        // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3c
        if predicate_fragments.is_empty() {
            return Err(ScopeTranslationError::EmptyScope);
        }
        group_fragments.push(format!("({})", predicate_fragments.join(" AND ")));
        // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3c
    }
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-3

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-4
    let sql_fragment = format!("({})", group_fragments.join(" OR "));
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-4

    // @cpt-begin:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-5
    Ok((sql_fragment, bind_params))
    // @cpt-end:cpt-cf-usage-collector-algo-production-storage-plugin-scope-to-sql:p1:inst-s2s-5
}

fn build_uuid_filter(
    filter: &ScopeFilter,
    param_n: usize,
    column: &str,
) -> Result<(String, SqlValue), ScopeTranslationError> {
    match filter {
        ScopeFilter::Eq(f) => {
            let uuid = f.value().as_uuid().ok_or_else(|| ScopeTranslationError::UnsupportedPredicate {
                kind: format!("non-UUID value for {column}"),
            })?;
            Ok((format!("{column} = ${param_n}"), SqlValue::Uuid(uuid)))
        }
        ScopeFilter::In(f) => {
            let uuids: Result<Vec<Uuid>, ScopeTranslationError> = f
                .values()
                .iter()
                .map(|v| {
                    v.as_uuid().ok_or_else(|| ScopeTranslationError::UnsupportedPredicate {
                        kind: format!("non-UUID value for {column}"),
                    })
                })
                .collect();
            Ok((format!("{column} = ANY(${param_n})"), SqlValue::UuidArray(uuids?)))
        }
        _ => unreachable!("InGroup/InGroupSubtree handled before calling build_uuid_filter"),
    }
}

fn build_text_filter(filter: &ScopeFilter, param_n: usize, column: &str) -> (String, SqlValue) {
    match filter {
        ScopeFilter::Eq(f) => (
            format!("{column} = ${param_n}"),
            SqlValue::Text(f.value().to_string()),
        ),
        ScopeFilter::In(f) => {
            let texts: Vec<String> = f.values().iter().map(std::string::ToString::to_string).collect();
            (format!("{column} = ANY(${param_n})"), SqlValue::TextArray(texts))
        }
        _ => unreachable!("InGroup/InGroupSubtree handled before calling build_text_filter"),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use modkit_security::{AccessScope, ScopeConstraint, ScopeFilter, ScopeValue, pep_properties};
    use uuid::Uuid;

    fn uid() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn test_scope_to_sql_empty_scope_fail_closed() {
        let scope = AccessScope::deny_all();
        assert!(matches!(scope_to_sql(&scope), Err(ScopeTranslationError::EmptyScope)));
    }

    #[test]
    fn test_scope_to_sql_unconstrained_scope_fail_closed() {
        let scope = AccessScope::allow_all();
        assert!(matches!(scope_to_sql(&scope), Err(ScopeTranslationError::EmptyScope)));
    }

    #[test]
    fn test_scope_to_sql_single_group() {
        let tid = uid();
        let scope = AccessScope::for_tenant(tid);
        let (sql, params) = scope_to_sql(&scope).unwrap();
        assert!(sql.contains("tenant_id = ANY($1)"), "sql: {sql}");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], SqlValue::UuidArray(vec![tid]));
    }

    #[test]
    fn test_scope_to_sql_multiple_groups_or_of_ands_preserved() {
        let tid1 = uid();
        let tid2 = uid();
        let scope = AccessScope::from_constraints(vec![
            ScopeConstraint::new(vec![ScopeFilter::in_uuids(
                pep_properties::OWNER_TENANT_ID,
                vec![tid1],
            )]),
            ScopeConstraint::new(vec![ScopeFilter::in_uuids(
                pep_properties::OWNER_TENANT_ID,
                vec![tid2],
            )]),
        ]);
        let (sql, params) = scope_to_sql(&scope).unwrap();
        // Must have exactly one OR joining two AND-groups
        assert!(sql.contains(" OR "), "sql must contain OR: {sql}");
        assert_eq!(params.len(), 2, "each group contributes one bind param");
        // Must be wrapped in outer parens
        assert!(sql.starts_with('(') && sql.ends_with(')'), "sql: {sql}");
    }

    #[test]
    fn test_scope_to_sql_ingroup_predicate_rejection() {
        let scope = AccessScope::single(ScopeConstraint::new(vec![ScopeFilter::in_group(
            pep_properties::OWNER_TENANT_ID,
            vec![ScopeValue::Uuid(uid())],
        )]));
        match scope_to_sql(&scope) {
            Err(ScopeTranslationError::UnsupportedPredicate { kind }) => {
                assert!(kind.contains("InGroup"), "kind: {kind}");
            }
            other => panic!("expected UnsupportedPredicate, got: {other:?}"),
        }
    }

    #[test]
    fn test_scope_to_sql_resource_id_filter() {
        let rid = uid();
        let scope = AccessScope::single(ScopeConstraint::new(vec![ScopeFilter::in_uuids(
            PROP_RESOURCE_ID,
            vec![rid],
        )]));
        let (sql, params) = scope_to_sql(&scope).unwrap();
        assert!(sql.contains("resource_id = ANY($1)"), "sql: {sql}");
        assert_eq!(params[0], SqlValue::UuidArray(vec![rid]));
    }

    #[test]
    fn test_scope_to_sql_resource_type_filter() {
        let scope = AccessScope::single(ScopeConstraint::new(vec![ScopeFilter::r#in(
            PROP_RESOURCE_TYPE,
            vec![ScopeValue::String("vm".to_string())],
        )]));
        let (sql, params) = scope_to_sql(&scope).unwrap();
        assert!(sql.contains("resource_type = ANY($1)"), "sql: {sql}");
        assert_eq!(params[0], SqlValue::TextArray(vec!["vm".to_string()]));
    }

    #[test]
    fn test_scope_to_sql_multi_predicate_and_within_group() {
        let tid = uid();
        let rid = uid();
        let scope = AccessScope::single(ScopeConstraint::new(vec![
            ScopeFilter::in_uuids(pep_properties::OWNER_TENANT_ID, vec![tid]),
            ScopeFilter::in_uuids(PROP_RESOURCE_ID, vec![rid]),
        ]));
        let (sql, params) = scope_to_sql(&scope).unwrap();
        assert!(sql.contains(" AND "), "sql must AND predicates within group: {sql}");
        assert_eq!(params.len(), 2);
    }
}
