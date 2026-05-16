use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use modkit::client_hub::{ClientHub, ClientScope};
use types_registry_sdk::testing::make_test_instance;
use types_registry_sdk::{
    GtsInstance, GtsTypeSchema, InstanceQuery, RegisterResult, TypeSchemaQuery,
    TypesRegistryClient, TypesRegistryError,
};
use usage_collector_sdk::{
    AggregationQuery, AggregationResult, UsageCollectorError, Page, RawQuery,
    UsageCollectorClientV1, UsageCollectorPluginClientV1, UsageCollectorPluginSpecV1, UsageKind,
    UsageRecord,
};
use uuid::Uuid;

use super::{Service, UsageCollectorLocalClient};
use crate::config::{MetricConfig, UsageCollectorConfig};

// Minimal mocks reused across the few tests below.

struct MockRegistry {
    instances: Vec<GtsInstance>,
}

#[async_trait::async_trait]
impl TypesRegistryClient for MockRegistry {
    async fn register(
        &self,
        _entities: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
    async fn register_type_schemas(
        &self,
        _: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
    async fn get_type_schema(&self, _: &str) -> Result<GtsTypeSchema, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_type_schema_by_uuid(
        &self,
        _: Uuid,
    ) -> Result<GtsTypeSchema, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_type_schemas(
        &self,
        _: Vec<String>,
    ) -> HashMap<String, Result<GtsTypeSchema, TypesRegistryError>> {
        unimplemented!()
    }
    async fn get_type_schemas_by_uuid(
        &self,
        _: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsTypeSchema, TypesRegistryError>> {
        unimplemented!()
    }
    async fn list_type_schemas(
        &self,
        _: TypeSchemaQuery,
    ) -> Result<Vec<GtsTypeSchema>, TypesRegistryError> {
        unimplemented!()
    }
    async fn register_instances(
        &self,
        _: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
    async fn get_instance(&self, _: &str) -> Result<GtsInstance, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_instance_by_uuid(&self, _: Uuid) -> Result<GtsInstance, TypesRegistryError> {
        unimplemented!()
    }
    async fn get_instances(
        &self,
        _: Vec<String>,
    ) -> HashMap<String, Result<GtsInstance, TypesRegistryError>> {
        unimplemented!()
    }
    async fn get_instances_by_uuid(
        &self,
        _: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsInstance, TypesRegistryError>> {
        unimplemented!()
    }
    async fn list_instances(
        &self,
        _: InstanceQuery,
    ) -> Result<Vec<GtsInstance>, TypesRegistryError> {
        Ok(self.instances.clone())
    }
}

struct OkPlugin;

#[async_trait::async_trait]
impl UsageCollectorPluginClientV1 for OkPlugin {
    async fn create_usage_record(&self, _: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
    async fn query_aggregated(
        &self,
        _: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }
    async fn query_raw(&self, _: RawQuery) -> Result<Page<UsageRecord>, UsageCollectorError> {
        Ok(Page::empty(100))
    }
}

fn plugin_content(gts_id: &str, vendor: &str) -> serde_json::Value {
    serde_json::json!({
        "id": gts_id,
        "vendor": vendor,
        "priority": 0,
        "properties": {},
    })
}

fn service_with_plugin() -> Arc<Service> {
    let instance_id = format!(
        "{}test.usage.mock.lc_test.v1",
        UsageCollectorPluginSpecV1::gts_schema_id()
    );
    let hub = Arc::new(ClientHub::default());
    let instance = make_test_instance(&instance_id, plugin_content(&instance_id, "cyberfabric"));
    hub.register::<dyn TypesRegistryClient>(Arc::new(MockRegistry {
        instances: vec![instance],
    }));
    hub.register_scoped::<dyn UsageCollectorPluginClientV1>(
        ClientScope::gts_id(&instance_id),
        Arc::new(OkPlugin),
    );
    Arc::new(Service::new(UsageCollectorConfig::default(), hub))
}

fn record() -> UsageRecord {
    UsageRecord {
        tenant_id: Uuid::new_v4(),
        module: "test-module".to_owned(),
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

#[tokio::test]
async fn create_usage_record_succeeds_through_local_client() {
    let svc = service_with_plugin();
    let client = UsageCollectorLocalClient::new(svc);
    assert!(client.create_usage_record(record()).await.is_ok());
}

#[tokio::test]
async fn module_not_configured_maps_to_canonical_not_found() {
    let svc = Arc::new(Service::new(
        UsageCollectorConfig::default(),
        Arc::new(ClientHub::default()),
    ));
    let client = UsageCollectorLocalClient::new(svc);
    let err = client.get_module_config("unknown").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::NotFound { .. }));
}

#[tokio::test]
async fn module_config_returns_allowed_metrics() {
    let mut metrics = HashMap::new();
    metrics.insert(
        "cpu.usage".to_owned(),
        MetricConfig {
            kind: UsageKind::Gauge,
            modules: None,
        },
    );
    let svc = Arc::new(Service::new(
        UsageCollectorConfig {
            metrics,
            ..UsageCollectorConfig::default()
        },
        Arc::new(ClientHub::default()),
    ));
    let client = UsageCollectorLocalClient::new(svc);
    let cfg = client.get_module_config("any-module").await.unwrap();
    assert_eq!(cfg.allowed_metrics.len(), 1);
    assert_eq!(cfg.allowed_metrics[0].name, "cpu.usage");
}
