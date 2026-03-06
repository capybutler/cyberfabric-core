use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::error::DomainError;

/// Resolves and validates model IDs against the user's policy catalog.
///
/// If `model` is `None`, returns the default model for the given `user_id`.
/// If `model` is `Some`, validates it is non-empty and exists in the catalog.
///
/// # Errors
///
/// Returns [`DomainError`] if the model is empty, not found in the catalog,
/// or the policy snapshot for `user_id` cannot be retrieved.
#[async_trait]
pub trait ModelResolver: Send + Sync {
    async fn resolve_model(
        &self,
        user_id: Uuid,
        model: Option<String>,
    ) -> Result<String, DomainError>;
}
