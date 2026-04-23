//! Client implementation for the RG tenant resolver plugin.
//!
//! Implements `TenantResolverPluginClient` using the domain service.

use std::collections::HashSet;

use async_trait::async_trait;
use modkit_security::SecurityContext;
use tenant_resolver_sdk::{
    BarrierMode, GetAncestorsOptions, GetAncestorsResponse, GetDescendantsOptions,
    GetDescendantsResponse, GetTenantsOptions, IsAncestorOptions, TenantId, TenantInfo,
    TenantResolverError, TenantResolverPluginClient, matches_status,
};

use super::service::Service;

#[async_trait]
impl TenantResolverPluginClient for Service {
    async fn get_tenant(
        &self,
        ctx: &SecurityContext,
        id: TenantId,
    ) -> Result<TenantInfo, TenantResolverError> {
        self.resolve_tenant(ctx, id).await
    }

    async fn get_root_tenant(
        &self,
        ctx: &SecurityContext,
    ) -> Result<TenantInfo, TenantResolverError> {
        // Walk ancestors of the context tenant until the topmost tenant-typed
        // group is reached -- that group is the single root of the tenant tree.
        let ctx_tenant = TenantId(ctx.subject_tenant_id());
        if ctx_tenant.is_nil() {
            return Err(TenantResolverError::Internal(
                "rg-tr-plugin: cannot resolve root tenant -- security context has nil tenant id"
                    .to_owned(),
            ));
        }

        // Ignore barriers here: the true forest root must be returned even when
        // an ancestor is self-managed. Respecting barriers in `get_root_tenant`
        // would stop at the nearest barrier and return a non-root tenant.
        let opts = GetAncestorsOptions {
            barrier_mode: BarrierMode::Ignore,
        };
        let resp = self.get_ancestors(ctx, ctx_tenant, &opts).await?;

        // Ancestors are ordered direct-parent -> root-ward; the last one IS the
        // root. When the context tenant itself has no ancestors, it is the root.
        let root_id = resp.ancestors.last().map_or(ctx_tenant, |a| a.id);
        self.resolve_tenant(ctx, root_id).await
    }

    async fn get_tenants(
        &self,
        ctx: &SecurityContext,
        ids: &[TenantId],
        options: &GetTenantsOptions,
    ) -> Result<Vec<TenantInfo>, TenantResolverError> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        for id in ids {
            if !seen.insert(id) {
                continue; // Skip duplicate IDs
            }
            match self.resolve_tenant(ctx, *id).await {
                Ok(tenant) if matches_status(&tenant, &options.status) => {
                    result.push(tenant);
                }
                Ok(_) | Err(TenantResolverError::TenantNotFound { .. }) => {
                    // Doesn't match status filter or not found — silently skip
                }
                Err(e) => return Err(e),
            }
        }

        Ok(result)
    }

    async fn get_ancestors(
        &self,
        ctx: &SecurityContext,
        id: TenantId,
        options: &GetAncestorsOptions,
    ) -> Result<GetAncestorsResponse, TenantResolverError> {
        let (tenant, ancestors) = self
            .resolve_ancestors(ctx, id, options.barrier_mode)
            .await?;

        Ok(GetAncestorsResponse { tenant, ancestors })
    }

    async fn get_descendants(
        &self,
        ctx: &SecurityContext,
        id: TenantId,
        options: &GetDescendantsOptions,
    ) -> Result<GetDescendantsResponse, TenantResolverError> {
        let (tenant, descendants) = self
            .resolve_descendants(
                ctx,
                id,
                &options.status,
                options.barrier_mode,
                options.max_depth,
            )
            .await?;

        Ok(GetDescendantsResponse {
            tenant,
            descendants,
        })
    }

    async fn is_ancestor(
        &self,
        ctx: &SecurityContext,
        ancestor_id: TenantId,
        descendant_id: TenantId,
        options: &IsAncestorOptions,
    ) -> Result<bool, TenantResolverError> {
        self.check_is_ancestor(ctx, ancestor_id, descendant_id, options.barrier_mode)
            .await
    }
}
