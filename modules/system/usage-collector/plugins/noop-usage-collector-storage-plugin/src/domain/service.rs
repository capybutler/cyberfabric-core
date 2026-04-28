//! No-op usage-collector storage domain service.

use modkit_macros::domain_model;

/// Stateless service backing the storage plugin client; records are not retained.
#[domain_model]
pub struct Service;

impl Service {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}
