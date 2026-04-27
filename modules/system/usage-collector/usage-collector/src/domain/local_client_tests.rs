use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use modkit::client_hub::{ClientHub, ClientScope};
use types_registry_sdk::{
    GtsEntity, ListQuery, RegisterResult, TypesRegistryClient, TypesRegistryError,
};
use usage_collector_sdk::UsageCollectorClientV1;
use usage_collector_sdk::UsageKind;
use usage_collector_sdk::UsageRecord;
use usage_collector_sdk::{
    UsageCollectorError, UsageCollectorPluginClientV1, UsageCollectorStoragePluginSpecV1,
};
use uuid::Uuid;

use super::UsageCollectorLocalClient;
use crate::config::{MetricConfig, UsageCollectorConfig};

// ── MockRegistry ──────────────────────────────────────────────────

struct MockRegistry {
    instances: Vec<GtsEntity>,
    list_calls: std::sync::atomic::AtomicUsize,
}

impl MockRegistry {
    fn new(instances: Vec<GtsEntity>) -> Self {
        Self {
            instances,
            list_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl TypesRegistryClient for MockRegistry {
    async fn list(&self, _query: ListQuery) -> Result<Vec<GtsEntity>, TypesRegistryError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.instances.clone())
    }

    async fn get(&self, gts_id: &str) -> Result<GtsEntity, TypesRegistryError> {
        self.instances
            .iter()
            .find(|e| e.gts_id == gts_id)
            .cloned()
            .ok_or_else(|| TypesRegistryError::not_found(gts_id))
    }

    async fn register(
        &self,
        _entities: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError> {
        Ok(vec![])
    }
}

struct OkPlugin;

#[async_trait::async_trait]
impl UsageCollectorPluginClientV1 for OkPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Ok(())
    }
}

fn plugin_content(gts_id: &str, vendor: &str) -> serde_json::Value {
    serde_json::json!({
        "id": gts_id,
        "vendor": vendor,
        "priority": 0,
        "properties": {}
    })
}

fn hub_with_plugin(
    instance_id: &str,
    vendor: &str,
    plugin: Arc<dyn UsageCollectorPluginClientV1>,
) -> Arc<ClientHub> {
    let hub = Arc::new(ClientHub::default());
    let entity = GtsEntity {
        id: Uuid::nil(),
        gts_id: instance_id.to_owned(),
        segments: vec![],
        is_schema: false,
        content: plugin_content(instance_id, vendor),
        description: None,
    };
    let reg: Arc<dyn TypesRegistryClient> = Arc::new(MockRegistry::new(vec![entity]));
    hub.register::<dyn TypesRegistryClient>(reg);
    hub.register_scoped::<dyn UsageCollectorPluginClientV1>(
        ClientScope::gts_id(instance_id),
        plugin,
    );
    hub
}

fn make_client() -> UsageCollectorLocalClient {
    let instance_id = format!(
        "{}test._.lc_test.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    let hub = Arc::new(ClientHub::default());
    let entity = GtsEntity {
        id: Uuid::nil(),
        gts_id: instance_id.clone(),
        segments: vec![],
        is_schema: false,
        content: plugin_content(&instance_id, "hyperspot"),
        description: None,
    };
    let reg: Arc<dyn TypesRegistryClient> = Arc::new(MockRegistry::new(vec![entity]));
    hub.register::<dyn TypesRegistryClient>(reg);
    hub.register_scoped::<dyn UsageCollectorPluginClientV1>(
        ClientScope::gts_id(&instance_id),
        Arc::new(OkPlugin),
    );
    UsageCollectorLocalClient::new(UsageCollectorConfig::default(), hub)
}

fn make_client_with_vendor(hub: Arc<ClientHub>, vendor: &str) -> UsageCollectorLocalClient {
    UsageCollectorLocalClient::new(
        UsageCollectorConfig {
            vendor: vendor.to_owned(),
            ..UsageCollectorConfig::default()
        },
        hub,
    )
}

fn record(tenant_id: Uuid) -> UsageRecord {
    UsageRecord {
        tenant_id,
        module: "test-module".to_owned(),
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        subject_id: Uuid::nil(),
        subject_type: "test.subject".to_owned(),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

#[tokio::test]
async fn create_usage_record_delegates_success_to_plugin() {
    let client = make_client();
    let tenant = Uuid::new_v4();
    let rec = record(tenant);
    assert!(client.create_usage_record(rec).await.is_ok());
}

// ── plugin timeout ────────────────────────────────────────────────

struct SlowPlugin;

#[async_trait::async_trait]
impl UsageCollectorPluginClientV1 for SlowPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        tokio::time::sleep(Duration::from_mins(1)).await;
        Ok(())
    }
}

#[tokio::test]
async fn plugin_timeout_returns_plugin_timeout_error() {
    let instance_id = format!(
        "{}test._.lc_test.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    let hub = hub_with_plugin(&instance_id, "hyperspot", Arc::new(SlowPlugin));
    let client = UsageCollectorLocalClient::new(
        UsageCollectorConfig {
            vendor: "hyperspot".to_owned(),
            plugin_timeout: Duration::from_millis(1),
            ..UsageCollectorConfig::default()
        },
        hub,
    );
    let tenant = Uuid::new_v4();
    let rec = record(tenant);
    let err = client.create_usage_record(rec).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::PluginTimeout));
}

// ── GTS plugin resolution caching ─────────────────────────────────

#[tokio::test]
async fn gts_plugin_selector_calls_registry_only_once_across_multiple_create_usage_records() {
    let instance_id = format!(
        "{}test._.lc_test.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    let reg = Arc::new(MockRegistry::new(vec![GtsEntity {
        id: Uuid::nil(),
        gts_id: instance_id.clone(),
        segments: vec![],
        is_schema: false,
        content: plugin_content(&instance_id, "hyperspot"),
        description: None,
    }]));
    let hub = Arc::new(ClientHub::default());
    hub.register::<dyn TypesRegistryClient>(Arc::clone(&reg) as Arc<dyn TypesRegistryClient>);
    hub.register_scoped::<dyn UsageCollectorPluginClientV1>(
        ClientScope::gts_id(&instance_id),
        Arc::new(OkPlugin),
    );
    let client = make_client_with_vendor(hub, "hyperspot");
    let tenant = Uuid::new_v4();

    let rec1 = record(tenant);
    client.create_usage_record(rec1).await.unwrap();
    let rec2 = record(tenant);
    client.create_usage_record(rec2).await.unwrap();
    let rec3 = record(tenant);
    client.create_usage_record(rec3).await.unwrap();

    assert_eq!(
        reg.list_calls.load(Ordering::SeqCst),
        1,
        "GTS registry should be queried exactly once after initial resolution"
    );
}

// ── no plugin registered in hub ───────────────────────────────────

#[tokio::test]
async fn no_plugin_client_in_hub_returns_internal_error() {
    let instance_id = format!(
        "{}test._.lc_test.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    // Register the entity in types-registry but deliberately omit the plugin client.
    let hub = Arc::new(ClientHub::default());
    let entity = GtsEntity {
        id: Uuid::nil(),
        gts_id: instance_id.clone(),
        segments: vec![],
        is_schema: false,
        content: plugin_content(&instance_id, "hyperspot"),
        description: None,
    };
    let reg: Arc<dyn TypesRegistryClient> = Arc::new(MockRegistry::new(vec![entity]));
    hub.register::<dyn TypesRegistryClient>(reg);
    // plugin client NOT registered
    let client = make_client_with_vendor(hub, "hyperspot");
    let tenant = Uuid::new_v4();
    let rec = record(tenant);
    let err = client.create_usage_record(rec).await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::Internal { .. }));
}

// ── get_module_config ─────────────────────────────────────────────

fn config_with_metrics(metrics: HashMap<String, MetricConfig>) -> UsageCollectorConfig {
    UsageCollectorConfig {
        metrics,
        ..UsageCollectorConfig::default()
    }
}

#[tokio::test]
async fn get_module_config_returns_not_found_when_no_metrics_configured() {
    let client = UsageCollectorLocalClient::new(
        UsageCollectorConfig::default(),
        Arc::new(ClientHub::default()),
    );
    let err = client.get_module_config("any-module").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ModuleNotFound { .. }));
}

#[tokio::test]
async fn get_module_config_returns_metric_when_modules_restriction_is_absent() {
    let mut metrics = HashMap::new();
    metrics.insert(
        "cpu.usage".to_owned(),
        MetricConfig {
            kind: UsageKind::Gauge,
            modules: None,
        },
    );
    let client = UsageCollectorLocalClient::new(
        config_with_metrics(metrics),
        Arc::new(ClientHub::default()),
    );
    let cfg = client.get_module_config("any-module").await.unwrap();
    assert_eq!(cfg.allowed_metrics.len(), 1);
    assert_eq!(cfg.allowed_metrics[0].name, "cpu.usage");
    assert!(matches!(cfg.allowed_metrics[0].kind, UsageKind::Gauge));
}

#[tokio::test]
async fn get_module_config_returns_metric_when_module_is_in_allow_list() {
    let mut metrics = HashMap::new();
    metrics.insert(
        "req.count".to_owned(),
        MetricConfig {
            kind: UsageKind::Counter,
            modules: Some(vec!["my-module".to_owned()]),
        },
    );
    let client = UsageCollectorLocalClient::new(
        config_with_metrics(metrics),
        Arc::new(ClientHub::default()),
    );
    let cfg = client.get_module_config("my-module").await.unwrap();
    assert_eq!(cfg.allowed_metrics.len(), 1);
    assert_eq!(cfg.allowed_metrics[0].name, "req.count");
    assert!(matches!(cfg.allowed_metrics[0].kind, UsageKind::Counter));
}

#[tokio::test]
async fn get_module_config_returns_not_found_when_module_not_in_allow_list() {
    let mut metrics = HashMap::new();
    metrics.insert(
        "cpu.usage".to_owned(),
        MetricConfig {
            kind: UsageKind::Gauge,
            modules: Some(vec!["other-module".to_owned()]),
        },
    );
    let client = UsageCollectorLocalClient::new(
        config_with_metrics(metrics),
        Arc::new(ClientHub::default()),
    );
    let err = client.get_module_config("my-module").await.unwrap_err();
    assert!(matches!(err, UsageCollectorError::ModuleNotFound { .. }));
}

#[tokio::test]
async fn get_module_config_returns_only_matching_metrics_from_mixed_config() {
    let mut metrics = HashMap::new();
    metrics.insert(
        "cpu.usage".to_owned(),
        MetricConfig {
            kind: UsageKind::Gauge,
            modules: None,
        },
    );
    metrics.insert(
        "disk.io".to_owned(),
        MetricConfig {
            kind: UsageKind::Counter,
            modules: Some(vec!["storage".to_owned()]),
        },
    );
    let client = UsageCollectorLocalClient::new(
        config_with_metrics(metrics),
        Arc::new(ClientHub::default()),
    );
    let cfg = client.get_module_config("my-module").await.unwrap();
    assert_eq!(cfg.allowed_metrics.len(), 1);
    assert_eq!(cfg.allowed_metrics[0].name, "cpu.usage");
}
