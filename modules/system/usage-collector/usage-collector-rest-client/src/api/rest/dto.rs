//! Wire-format DTOs for the usage-collector REST client.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use usage_collector_sdk::models::UsageKind;

/// JSON body for `POST /usage-collector/v1/records`.
#[derive(Serialize)]
pub struct CreateUsageRecordBody {
    pub module: String,
    pub tenant_id: uuid::Uuid,
    pub resource_type: String,
    pub resource_id: uuid::Uuid,
    pub metric: String,
    pub kind: UsageKind,
    pub subject_id: uuid::Uuid,
    pub subject_type: String,
    pub idempotency_key: String,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}
