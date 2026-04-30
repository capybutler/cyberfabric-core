use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    AggregationQuery, AggregationResult, PagedResult, RawQuery, UsageCollectorError,
    UsageCollectorPluginClientV1, UsageCollectorStoragePluginSpecV1,
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

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult {
            items: vec![],
            next_cursor: None,
        })
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
        subject_id: Some(Uuid::nil()),
        subject_type: Some("test.subject".to_owned()),
        idempotency_key: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        metadata: None,
    }
}

// PDP validation is enforced at the REST handler layer before the domain client is called.
// The domain client forwards UsageRecord to the storage plugin without inspecting subject fields.
fn record_no_subject(tenant_id: Uuid) -> UsageRecord {
    UsageRecord {
        tenant_id,
        module: "test-module".to_owned(),
        metric: "test.metric".to_owned(),
        kind: UsageKind::Gauge,
        value: 1.0,
        resource_id: Uuid::new_v4(),
        resource_type: "test.resource".to_owned(),
        subject_id: None,
        subject_type: None,
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

// PDP is enforced at the REST handler layer; the domain client accepts records with or without
// subject fields and forwards them to the storage plugin unconditionally.
#[tokio::test]
async fn create_usage_record_with_no_subject_succeeds() {
    let client = make_client();
    let tenant = Uuid::new_v4();
    let rec = record_no_subject(tenant);
    assert!(client.create_usage_record(rec).await.is_ok());
}

#[tokio::test]
async fn create_usage_record_with_subject_succeeds() {
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

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult {
            items: vec![],
            next_cursor: None,
        })
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

// ── circuit breaker ──────────────────────────────────────────────────────────

/// A storage plugin that always returns an error.
struct FailPlugin;

#[async_trait::async_trait]
impl UsageCollectorPluginClientV1 for FailPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        Err(UsageCollectorError::internal("simulated plugin failure"))
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Err(UsageCollectorError::internal("simulated plugin failure"))
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Err(UsageCollectorError::internal("simulated plugin failure"))
    }
}

/// A storage plugin that counts every `store` invocation via an atomic counter.
/// Succeeds or fails depending on the `should_fail` flag.
struct CountingPlugin {
    counter: Arc<AtomicUsize>,
    should_fail: bool,
}

impl CountingPlugin {
    fn failing(counter: Arc<AtomicUsize>) -> Self {
        Self {
            counter,
            should_fail: true,
        }
    }
}

#[async_trait::async_trait]
impl UsageCollectorPluginClientV1 for CountingPlugin {
    async fn create_usage_record(&self, _record: UsageRecord) -> Result<(), UsageCollectorError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        if self.should_fail {
            Err(UsageCollectorError::internal("simulated plugin failure"))
        } else {
            Ok(())
        }
    }

    async fn query_aggregated(
        &self,
        _query: AggregationQuery,
    ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
        Ok(vec![])
    }

    async fn query_raw(
        &self,
        _query: RawQuery,
    ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
        Ok(PagedResult {
            items: vec![],
            next_cursor: None,
        })
    }
}

/// Construct a `UsageCollectorLocalClient` backed by the given plugin, with configurable
/// circuit breaker parameters for fast, deterministic tests.
fn make_cb_client(
    plugin: Arc<dyn UsageCollectorPluginClientV1>,
    threshold: u32,
    window: Duration,
    recovery: Duration,
) -> UsageCollectorLocalClient {
    let instance_id = format!(
        "{}test._.cb_test.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    let hub = hub_with_plugin(&instance_id, "hyperspot", plugin);
    UsageCollectorLocalClient::new(
        UsageCollectorConfig {
            vendor: "hyperspot".to_owned(),
            circuit_breaker_failure_threshold: threshold,
            circuit_breaker_window: window,
            circuit_breaker_recovery_timeout: recovery,
            ..UsageCollectorConfig::default()
        },
        hub,
    )
}

/// After N consecutive failures, the (N+1)-th call must return `CircuitOpen`.
#[tokio::test]
async fn circuit_opens_after_n_consecutive_failures() {
    let threshold = 2u32;
    let client = make_cb_client(
        Arc::new(FailPlugin),
        threshold,
        Duration::from_secs(10),
        Duration::from_millis(1),
    );

    for _ in 0..threshold {
        drop(client.create_usage_record(record(Uuid::new_v4())).await);
    }

    let err = client
        .create_usage_record(record(Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(
        matches!(err, UsageCollectorError::CircuitOpen),
        "expected CircuitOpen after {threshold} failures, got {err:?}"
    );
}

/// Once the circuit is open, every call must return `CircuitOpen` without invoking the plugin.
#[tokio::test]
async fn open_circuit_rejects_without_calling_plugin() {
    let counter = Arc::new(AtomicUsize::new(0));
    let threshold = 2u32;
    let client = make_cb_client(
        Arc::new(CountingPlugin::failing(Arc::clone(&counter))),
        threshold,
        Duration::from_secs(10),
        Duration::from_millis(1),
    );

    // Drive to open state — these N calls do reach the plugin.
    for _ in 0..threshold {
        drop(client.create_usage_record(record(Uuid::new_v4())).await);
    }
    let calls_to_open = counter.load(Ordering::SeqCst);

    // All subsequent calls must be rejected without touching the plugin.
    for _ in 0..3 {
        let err = client
            .create_usage_record(record(Uuid::new_v4()))
            .await
            .unwrap_err();
        assert!(
            matches!(err, UsageCollectorError::CircuitOpen),
            "expected CircuitOpen, got {err:?}"
        );
    }

    assert_eq!(
        counter.load(Ordering::SeqCst),
        calls_to_open,
        "plugin must not be invoked while circuit is open"
    );
}

/// In `HalfOpen` state exactly one concurrent caller is admitted as the probe; all others are
/// rejected with `CircuitOpen`.
#[tokio::test]
async fn half_open_admits_exactly_one_concurrent_probe_others_rejected() {
    // Use a TogglePlugin: fails during the open-driving phase, then succeeds for the probe.
    // The probe yields via `tokio::task::yield_now()` so that the other two concurrent tasks
    // get a chance to call `is_open()` (which sees `HalfOpen` → returns `CircuitOpen`) before
    // the probe completes and transitions the circuit back to `Closed`.
    use std::sync::atomic::AtomicBool;

    struct TogglePlugin {
        counter: Arc<AtomicUsize>,
        fail: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl UsageCollectorPluginClientV1 for TogglePlugin {
        async fn create_usage_record(
            &self,
            _record: UsageRecord,
        ) -> Result<(), UsageCollectorError> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(UsageCollectorError::internal("toggle fail"));
            }
            // Yield so that other spawned tasks can run and see the HalfOpen state before
            // this probe completes and closes the circuit.
            tokio::task::yield_now().await;
            Ok(())
        }

        async fn query_aggregated(
            &self,
            _query: AggregationQuery,
        ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
            Ok(vec![])
        }

        async fn query_raw(
            &self,
            _query: RawQuery,
        ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
            Ok(PagedResult {
                items: vec![],
                next_cursor: None,
            })
        }
    }

    let threshold = 2u32;
    let fail_flag = Arc::new(AtomicBool::new(true));
    let probe_counter = Arc::new(AtomicUsize::new(0));
    let toggle_plugin = Arc::new(TogglePlugin {
        counter: Arc::clone(&probe_counter),
        fail: Arc::clone(&fail_flag),
    });

    let instance_id = format!(
        "{}test._.cb_halfopen.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    let hub = hub_with_plugin(&instance_id, "hyperspot", toggle_plugin);
    let client = Arc::new(UsageCollectorLocalClient::new(
        UsageCollectorConfig {
            vendor: "hyperspot".to_owned(),
            circuit_breaker_failure_threshold: threshold,
            circuit_breaker_window: Duration::from_secs(10),
            circuit_breaker_recovery_timeout: Duration::from_millis(1),
            ..UsageCollectorConfig::default()
        },
        hub,
    ));

    // Open the circuit.
    for _ in 0..threshold {
        drop(client.create_usage_record(record(Uuid::new_v4())).await);
    }
    let calls_during_open = probe_counter.load(Ordering::SeqCst);

    // Wait for recovery timeout to elapse so the circuit transitions to HalfOpen on next call.
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Switch plugin to succeed so the probe can close the circuit.
    fail_flag.store(false, Ordering::SeqCst);

    // Fire 3 concurrent calls. Exactly 1 should reach the plugin (the HalfOpen probe);
    // the other 2 must get `CircuitOpen` because they see the `HalfOpen` state in `is_open()`.
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..3 {
        let c = Arc::clone(&client);
        set.spawn(async move { c.create_usage_record(record(Uuid::new_v4())).await });
    }

    let mut circuit_open_count = 0usize;
    let mut success_count = 0usize;
    while let Some(result) = set.join_next().await {
        match result.expect("task panicked") {
            Ok(()) => success_count += 1,
            Err(UsageCollectorError::CircuitOpen) => circuit_open_count += 1,
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    // Exactly one probe reached the plugin (the rest were rejected at is_open()).
    let probe_calls = probe_counter.load(Ordering::SeqCst) - calls_during_open;
    assert_eq!(
        probe_calls, 1,
        "exactly one concurrent call should reach the plugin in HalfOpen"
    );
    assert_eq!(success_count, 1, "the single probe should succeed");
    assert_eq!(
        circuit_open_count, 2,
        "the other two callers should get CircuitOpen"
    );
}

/// After a successful probe from `HalfOpen`, the circuit closes and the next call succeeds.
#[tokio::test]
async fn successful_probe_closes_circuit() {
    // Use a CountingPlugin that fails for the first `threshold` calls then succeeds.
    use std::sync::atomic::AtomicBool;

    struct TogglePlugin {
        fail: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl UsageCollectorPluginClientV1 for TogglePlugin {
        async fn create_usage_record(
            &self,
            _record: UsageRecord,
        ) -> Result<(), UsageCollectorError> {
            if self.fail.load(Ordering::SeqCst) {
                Err(UsageCollectorError::internal("toggle fail"))
            } else {
                Ok(())
            }
        }

        async fn query_aggregated(
            &self,
            _query: AggregationQuery,
        ) -> Result<Vec<AggregationResult>, UsageCollectorError> {
            Ok(vec![])
        }

        async fn query_raw(
            &self,
            _query: RawQuery,
        ) -> Result<PagedResult<UsageRecord>, UsageCollectorError> {
            Ok(PagedResult {
                items: vec![],
                next_cursor: None,
            })
        }
    }

    let threshold = 2u32;

    let fail_flag = Arc::new(AtomicBool::new(true));
    let instance_id = format!(
        "{}test._.cb_close.v1",
        UsageCollectorStoragePluginSpecV1::gts_schema_id()
    );
    let hub = hub_with_plugin(
        &instance_id,
        "hyperspot",
        Arc::new(TogglePlugin {
            fail: Arc::clone(&fail_flag),
        }),
    );
    let client = UsageCollectorLocalClient::new(
        UsageCollectorConfig {
            vendor: "hyperspot".to_owned(),
            circuit_breaker_failure_threshold: threshold,
            circuit_breaker_window: Duration::from_secs(10),
            circuit_breaker_recovery_timeout: Duration::from_millis(1),
            ..UsageCollectorConfig::default()
        },
        hub,
    );

    // Open the circuit.
    for _ in 0..threshold {
        drop(client.create_usage_record(record(Uuid::new_v4())).await);
    }

    // Wait for recovery timeout; circuit should be ready to transition to HalfOpen.
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Switch plugin to succeed so the probe closes the circuit.
    fail_flag.store(false, Ordering::SeqCst);

    // The probe call: transitions Open → HalfOpen (via is_open()) then succeeds → Closed.
    client
        .create_usage_record(record(Uuid::new_v4()))
        .await
        .expect("probe call should succeed and close circuit");

    // Next call should also succeed (circuit is Closed).
    client
        .create_usage_record(record(Uuid::new_v4()))
        .await
        .expect("circuit should be closed after successful probe");
}

/// After a failed probe from `HalfOpen`, the circuit re-opens and the next call returns `CircuitOpen`.
#[tokio::test]
async fn failed_probe_reopens_circuit() {
    // Use threshold=1 so that a single probe failure immediately re-opens the circuit.
    let threshold = 1u32;
    let client = make_cb_client(
        Arc::new(FailPlugin),
        threshold,
        Duration::from_secs(10),
        Duration::from_millis(1),
    );

    // Open the circuit with 1 failure.
    drop(client.create_usage_record(record(Uuid::new_v4())).await);

    // Verify circuit is open.
    let err = client
        .create_usage_record(record(Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(err, UsageCollectorError::CircuitOpen));

    // Wait for recovery timeout.
    tokio::time::sleep(Duration::from_millis(5)).await;

    // The probe call: Open → HalfOpen (via is_open()), then fails → re-opens circuit.
    let probe_err = client
        .create_usage_record(record(Uuid::new_v4()))
        .await
        .unwrap_err();
    // The probe itself returns the plugin error, not CircuitOpen.
    assert!(
        matches!(probe_err, UsageCollectorError::Internal { .. }),
        "probe should propagate plugin error, got {probe_err:?}"
    );

    // Next call should see the circuit open again.
    let err = client
        .create_usage_record(record(Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(
        matches!(err, UsageCollectorError::CircuitOpen),
        "circuit should be open again after failed probe, got {err:?}"
    );
}
