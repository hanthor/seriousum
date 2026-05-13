//! Backend Mapping Engine - connects services to their endpoints
//!
//! Implements Issue #46 (P1.3)
//!
//! This component integrates:
//! - Track 1 (ServiceObserver): Receives service change notifications
//! - Track 2 (eBPF Maps): Populates backend pools
//! - Pod cache: Discovers endpoints matching service selectors
//! - Health tracking: Monitors pod readiness

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

// ============================================================================
// Core Types
// ============================================================================

/// Pod information (mirrors K8s Pod spec)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodInfo {
    pub namespace: String,
    pub name: String,
    pub pod_ip: Option<IpAddr>,
    pub node_name: Option<String>,
    pub labels: HashMap<String, String>,
    pub status: PodStatus,
    pub containers: Vec<ContainerStatus>,
}

/// Pod lifecycle status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PodStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

/// Container status in a pod
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatus {
    pub name: String,
    pub ready: bool,
    pub restart_count: u32,
}

/// Backend endpoint discovered for a service.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct DiscoveredBackend {
    /// Pod IP address.
    pub pod_ip: IpAddr,
    /// Service target port.
    pub port: u16,
    /// Source pod name.
    pub pod_name: String,
    /// Source pod namespace.
    pub namespace: String,
    /// Optional node hosting the pod.
    pub node_name: Option<String>,
    /// Whether the backend is currently considered healthy.
    pub healthy: bool,
}

impl DiscoveredBackend {
    /// Returns a stable namespace/name key for the backend.
    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.pod_name)
    }
}

/// Backend pool derived from pod discovery for a single service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceBackendPool {
    /// Service key in `namespace/name` format.
    pub service_key: String,
    /// Discovered backends for the service.
    pub backends: Vec<DiscoveredBackend>,
    /// Number of healthy backends in the pool.
    pub healthy_count: usize,
    /// Monotonic generation updated on every refresh.
    pub generation: u64,
}

impl ServiceBackendPool {
    /// Creates an empty discovered backend pool for a service.
    pub fn new(service_key: String) -> Self {
        Self {
            service_key,
            backends: Vec::new(),
            healthy_count: 0,
            generation: 0,
        }
    }

    /// Returns only healthy discovered backends.
    pub fn get_healthy(&self) -> Vec<&DiscoveredBackend> {
        self.backends
            .iter()
            .filter(|backend| backend.healthy)
            .collect()
    }

    /// Replaces the discovered backend set and bumps the generation counter.
    pub fn update_backends(&mut self, backends: Vec<DiscoveredBackend>) {
        self.backends = backends;
        self.healthy_count = self
            .backends
            .iter()
            .filter(|backend| backend.healthy)
            .count();
        self.generation = self.generation.wrapping_add(1);
    }
}

/// Backend state and health for load-balancer selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendState {
    /// Backend is eligible for new traffic.
    Active,
    /// Backend is draining existing connections.
    Terminating,
    /// Backend is temporarily held out of rotation.
    Quarantined,
    /// Backend is manually disabled for maintenance.
    Maintenance,
}

impl BackendState {
    /// Returns whether the backend can receive traffic.
    pub fn is_active(&self) -> bool {
        *self == Self::Active
    }
}

impl std::fmt::Display for BackendState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Terminating => write!(f, "terminating"),
            Self::Quarantined => write!(f, "quarantined"),
            Self::Maintenance => write!(f, "maintenance"),
        }
    }
}

/// Unique backend identifier (maps to an eBPF backend slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BackendID(pub u32);

impl BackendID {
    /// Creates a new backend identifier.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for BackendID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "backend-{}", self.0)
    }
}

/// A single load-balancer backend.
#[derive(Debug, Clone)]
pub struct Backend {
    /// Unique backend identifier.
    pub id: BackendID,
    /// Backend socket address.
    pub addr: SocketAddr,
    /// Current backend state.
    pub state: BackendState,
    /// Relative backend weight.
    pub weight: u16,
    /// Optional node hosting the backend.
    pub node_name: Option<String>,
    /// Optional routing zone for topology-aware selection.
    pub zone: Option<String>,
}

impl Backend {
    /// Creates a new active backend with default metadata.
    pub fn new(id: BackendID, addr: SocketAddr) -> Self {
        Self {
            id,
            addr,
            state: BackendState::Active,
            weight: 1,
            node_name: None,
            zone: None,
        }
    }

    /// Sets the backend weight.
    pub fn with_weight(mut self, w: u16) -> Self {
        self.weight = w;
        self
    }

    /// Associates the backend with a node.
    pub fn with_node(mut self, node: impl Into<String>) -> Self {
        self.node_name = Some(node.into());
        self
    }
}

/// Backend selection algorithm — mirrors Cilium's LB algorithm choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SelectionAlgorithm {
    /// Round-robin selection.
    #[default]
    RoundRobin,
    /// Random backend selection.
    Random,
    /// Pick the least-loaded backend.
    LeastConnections,
    /// ECMP-style source hash selection.
    SourceHash,
    /// Maglev hashing.
    Maglev,
}

/// Stateful round-robin selector over a backend pool.
pub struct RoundRobinSelector {
    backends: Vec<Backend>,
    cursor: std::sync::atomic::AtomicUsize,
}

impl RoundRobinSelector {
    /// Creates a round-robin selector over the provided backends.
    pub fn new(backends: Vec<Backend>) -> Self {
        Self {
            backends,
            cursor: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Selects the next active backend.
    pub fn select(&self) -> Option<&Backend> {
        let active: Vec<_> = self
            .backends
            .iter()
            .filter(|backend| backend.state.is_active())
            .collect();
        if active.is_empty() {
            return None;
        }
        let idx = self
            .cursor
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % active.len();
        Some(active[idx])
    }

    /// Returns the number of active backends.
    pub fn active_count(&self) -> usize {
        self.backends
            .iter()
            .filter(|backend| backend.state.is_active())
            .count()
    }
}

/// Source-IP-based consistent hash selector (pure hash math).
pub struct SourceHashSelector {
    backends: Vec<Backend>,
}

impl SourceHashSelector {
    /// Creates a source-hash selector over the provided backends.
    pub fn new(backends: Vec<Backend>) -> Self {
        Self { backends }
    }

    /// Selects a backend for the provided source socket address.
    pub fn select(&self, source: &SocketAddr) -> Option<&Backend> {
        let active: Vec<_> = self
            .backends
            .iter()
            .filter(|backend| backend.state.is_active())
            .collect();
        if active.is_empty() {
            return None;
        }
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let idx = (hasher.finish() as usize) % active.len();
        Some(active[idx])
    }
}

/// A service's backend pool — the core backend mapping data structure.
pub struct BackendPool {
    backends: HashMap<BackendID, Backend>,
    algorithm: SelectionAlgorithm,
    next_id: u32,
}

impl BackendPool {
    /// Creates an empty backend pool with the provided selection algorithm.
    pub fn new(algorithm: SelectionAlgorithm) -> Self {
        Self {
            backends: Default::default(),
            algorithm,
            next_id: 1,
        }
    }

    /// Adds a backend address to the pool and returns its allocated identifier.
    pub fn add(&mut self, addr: SocketAddr) -> BackendID {
        let id = BackendID::new(self.next_id);
        self.next_id += 1;
        self.backends.insert(id, Backend::new(id, addr));
        id
    }

    /// Removes a backend by identifier.
    pub fn remove(&mut self, id: &BackendID) -> Option<Backend> {
        self.backends.remove(id)
    }

    /// Updates a backend state, returning whether the backend existed.
    pub fn set_state(&mut self, id: &BackendID, state: BackendState) -> bool {
        if let Some(backend) = self.backends.get_mut(id) {
            backend.state = state;
            true
        } else {
            false
        }
    }

    /// Looks up a backend by identifier.
    pub fn get(&self, id: &BackendID) -> Option<&Backend> {
        self.backends.get(id)
    }

    /// Returns all active backends in the pool.
    pub fn active_backends(&self) -> Vec<&Backend> {
        self.backends
            .values()
            .filter(|backend| backend.state.is_active())
            .collect()
    }

    /// Returns the number of backends in the pool.
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    /// Returns whether the pool contains no backends.
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    /// Returns the pool's selection algorithm.
    pub fn algorithm(&self) -> SelectionAlgorithm {
        self.algorithm
    }

    /// Diffs two backend pools and returns added and removed backend addresses.
    pub fn diff(&self, other: &BackendPool) -> (Vec<SocketAddr>, Vec<SocketAddr>) {
        let self_addrs: std::collections::HashSet<_> =
            self.backends.values().map(|backend| backend.addr).collect();
        let other_addrs: std::collections::HashSet<_> = other
            .backends
            .values()
            .map(|backend| backend.addr)
            .collect();
        let added = other_addrs.difference(&self_addrs).copied().collect();
        let removed = self_addrs.difference(&other_addrs).copied().collect();
        (added, removed)
    }
}

/// Errors returned by backend mapping operations.
#[derive(Debug, thiserror::Error)]
pub enum BackendMappingError {
    /// Requested backend identifier was not present in the pool.
    #[error("backend not found: {0}")]
    NotFound(BackendID),
    /// Backend address already exists in the pool.
    #[error("backend already exists: {0}")]
    Duplicate(SocketAddr),
    /// Selection was attempted against an empty pool.
    #[error("pool is empty")]
    EmptyPool,
}

/// Pod cache key: "namespace/name"
pub type PodKey = String;

/// Pod cache
#[derive(Debug, Clone)]
pub struct PodCache {
    pods: Arc<RwLock<HashMap<PodKey, PodInfo>>>,
}

impl PodCache {
    pub fn new() -> Self {
        Self {
            pods: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_pod(&self, pod: PodInfo) -> Result<()> {
        let key = format!("{}/{}", pod.namespace, pod.name);
        let mut pods = self.pods.write().await;

        if pods.contains_key(&key) {
            return Err(anyhow!("Pod already exists: {}", key));
        }

        pods.insert(key, pod);
        Ok(())
    }

    pub async fn update_pod(&self, pod: PodInfo) -> Result<()> {
        let key = format!("{}/{}", pod.namespace, pod.name);
        let mut pods = self.pods.write().await;

        if !pods.contains_key(&key) {
            return Err(anyhow!("Pod not found: {}", key));
        }

        pods.insert(key, pod);
        Ok(())
    }

    pub async fn delete_pod(&self, namespace: &str, name: &str) -> Result<()> {
        let key = format!("{}/{}", namespace, name);
        let mut pods = self.pods.write().await;

        pods.remove(&key)
            .ok_or_else(|| anyhow!("Pod not found: {}", key))?;
        Ok(())
    }

    pub async fn get_pod(&self, namespace: &str, name: &str) -> Option<PodInfo> {
        let key = format!("{}/{}", namespace, name);
        let pods = self.pods.read().await;
        pods.get(&key).cloned()
    }

    pub async fn list_pods(&self) -> Vec<PodInfo> {
        let pods = self.pods.read().await;
        pods.values().cloned().collect()
    }

    pub async fn list_pods_in_namespace(&self, namespace: &str) -> Vec<PodInfo> {
        let pods = self.pods.read().await;
        pods.values()
            .filter(|p| p.namespace == namespace)
            .cloned()
            .collect()
    }

    pub async fn find_pods_with_labels(
        &self,
        namespace: &str,
        selector: &HashMap<String, String>,
    ) -> Vec<PodInfo> {
        let pods = self.pods.read().await;
        pods.values()
            .filter(|p| p.namespace == namespace && selector_matches(&p.labels, selector))
            .cloned()
            .collect()
    }

    pub async fn pod_count(&self) -> usize {
        self.pods.read().await.len()
    }

    pub async fn clear(&self) {
        self.pods.write().await.clear();
    }
}

impl Default for PodCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Backend Mapping Engine
// ============================================================================

/// Main backend mapping engine.
pub struct BackendMappingEngine {
    pod_cache: PodCache,
    pools: Arc<RwLock<HashMap<String, ServiceBackendPool>>>,
}

impl BackendMappingEngine {
    /// Creates a new backend mapping engine.
    pub fn new() -> Self {
        Self {
            pod_cache: PodCache::new(),
            pools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns the pod cache used for backend discovery.
    pub fn pod_cache(&self) -> &PodCache {
        &self.pod_cache
    }

    /// Discovers service backends from matching pods.
    pub async fn discover_backends(
        &self,
        namespace: &str,
        service_selector: &HashMap<String, String>,
        target_port: u16,
        _protocol: &str,
    ) -> Result<Vec<DiscoveredBackend>> {
        let pods = self
            .pod_cache
            .find_pods_with_labels(namespace, service_selector)
            .await;

        let backends: Vec<DiscoveredBackend> = pods
            .iter()
            .filter_map(|pod| {
                if pod.status != PodStatus::Running {
                    return None;
                }

                let pod_ip = pod.pod_ip?;
                if !pod.containers.iter().all(|container| container.ready) {
                    return None;
                }

                Some(DiscoveredBackend {
                    pod_ip,
                    port: target_port,
                    pod_name: pod.name.clone(),
                    namespace: pod.namespace.clone(),
                    node_name: pod.node_name.clone(),
                    healthy: true,
                })
            })
            .collect();

        let selector_target = service_selector
            .values()
            .next()
            .map(String::as_str)
            .unwrap_or("*");
        debug!(
            "Discovered {} backends for {}/{} (selector: {:?})",
            backends.len(),
            namespace,
            selector_target,
            service_selector
        );

        Ok(backends)
    }

    /// Updates the discovered backend pool for a service.
    pub async fn update_backend_pool(
        &self,
        service_key: String,
        backends: Vec<DiscoveredBackend>,
    ) -> Result<()> {
        let mut pools = self.pools.write().await;
        let pool = pools
            .entry(service_key.clone())
            .or_insert_with(|| ServiceBackendPool::new(service_key.clone()));
        pool.update_backends(backends);
        Ok(())
    }

    /// Returns the discovered backend pool for a service.
    pub async fn get_pool(&self, service_key: &str) -> Option<ServiceBackendPool> {
        let pools = self.pools.read().await;
        pools.get(service_key).cloned()
    }

    /// Returns healthy discovered backends for a service.
    pub async fn get_healthy_backends(&self, service_key: &str) -> Vec<DiscoveredBackend> {
        let pools = self.pools.read().await;
        pools
            .get(service_key)
            .map(|pool| {
                pool.get_healthy()
                    .iter()
                    .map(|backend| (*backend).clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns all discovered backends for a service.
    pub async fn get_backends(&self, service_key: &str) -> Vec<DiscoveredBackend> {
        let pools = self.pools.read().await;
        pools
            .get(service_key)
            .map(|pool| pool.backends.clone())
            .unwrap_or_default()
    }

    /// Deletes a discovered backend pool.
    pub async fn delete_pool(&self, service_key: &str) -> Result<()> {
        let mut pools = self.pools.write().await;
        pools
            .remove(service_key)
            .ok_or_else(|| anyhow!("Pool not found: {}", service_key))?;
        Ok(())
    }

    /// Lists all discovered backend pools.
    pub async fn list_pools(&self) -> Vec<ServiceBackendPool> {
        let pools = self.pools.read().await;
        pools.values().cloned().collect()
    }

    /// Returns the number of tracked backend pools.
    pub async fn pool_count(&self) -> usize {
        self.pools.read().await.len()
    }
}

impl Default for BackendMappingEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Check if pod labels match service selector
fn selector_matches(
    pod_labels: &HashMap<String, String>,
    selector: &HashMap<String, String>,
) -> bool {
    selector
        .iter()
        .all(|(key, value)| pod_labels.get(key).map(|v| v == value).unwrap_or(false))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn test_pod_cache_add_get() {
        let cache = PodCache::new();
        let pod = PodInfo {
            namespace: "default".to_string(),
            name: "test-pod".to_string(),
            pod_ip: "10.0.0.1".parse().ok(),
            node_name: Some("node-1".to_string()),
            labels: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            status: PodStatus::Running,
            containers: vec![ContainerStatus {
                name: "nginx".to_string(),
                ready: true,
                restart_count: 0,
            }],
        };

        cache.add_pod(pod.clone()).await.unwrap();
        assert_eq!(cache.pod_count().await, 1);

        let retrieved = cache.get_pod("default", "test-pod").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-pod");
    }

    #[tokio::test]
    async fn test_pod_cache_update() {
        let cache = PodCache::new();
        let mut pod = PodInfo {
            namespace: "default".to_string(),
            name: "test-pod".to_string(),
            pod_ip: "10.0.0.1".parse().ok(),
            node_name: Some("node-1".to_string()),
            labels: Default::default(),
            status: PodStatus::Running,
            containers: vec![],
        };

        cache.add_pod(pod.clone()).await.unwrap();

        pod.status = PodStatus::Failed;
        cache.update_pod(pod.clone()).await.unwrap();

        let retrieved = cache.get_pod("default", "test-pod").await.unwrap();
        assert_eq!(retrieved.status, PodStatus::Failed);
    }

    #[tokio::test]
    async fn test_pod_cache_delete() {
        let cache = PodCache::new();
        let pod = PodInfo {
            namespace: "default".to_string(),
            name: "test-pod".to_string(),
            pod_ip: None,
            node_name: None,
            labels: Default::default(),
            status: PodStatus::Running,
            containers: vec![],
        };

        cache.add_pod(pod).await.unwrap();
        cache.delete_pod("default", "test-pod").await.unwrap();
        assert_eq!(cache.pod_count().await, 0);
    }

    #[tokio::test]
    async fn test_pod_cache_find_by_labels() {
        let cache = PodCache::new();

        let pod1 = PodInfo {
            namespace: "default".to_string(),
            name: "nginx-1".to_string(),
            pod_ip: "10.0.0.1".parse().ok(),
            node_name: None,
            labels: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            status: PodStatus::Running,
            containers: vec![],
        };

        let pod2 = PodInfo {
            namespace: "default".to_string(),
            name: "postgres-1".to_string(),
            pod_ip: "10.0.0.2".parse().ok(),
            node_name: None,
            labels: [("app".to_string(), "postgres".to_string())]
                .iter()
                .cloned()
                .collect(),
            status: PodStatus::Running,
            containers: vec![],
        };

        cache.add_pod(pod1).await.unwrap();
        cache.add_pod(pod2).await.unwrap();

        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let found = cache.find_pods_with_labels("default", &selector).await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "nginx-1");
    }

    #[tokio::test]
    async fn test_backend_mapping_discover() {
        let engine = BackendMappingEngine::new();

        // Add pod to cache
        let pod = PodInfo {
            namespace: "default".to_string(),
            name: "nginx-1".to_string(),
            pod_ip: "10.0.0.1".parse().ok(),
            node_name: Some("node-1".to_string()),
            labels: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            status: PodStatus::Running,
            containers: vec![ContainerStatus {
                name: "nginx".to_string(),
                ready: true,
                restart_count: 0,
            }],
        };

        engine.pod_cache().add_pod(pod).await.unwrap();

        // Discover backends
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let backends = engine
            .discover_backends("default", &selector, 8080, "TCP")
            .await
            .unwrap();

        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].pod_name, "nginx-1");
        assert!(backends[0].healthy);
    }

    #[tokio::test]
    async fn test_backend_mapping_no_ready_containers() {
        let engine = BackendMappingEngine::new();

        // Add pod with not-ready containers
        let pod = PodInfo {
            namespace: "default".to_string(),
            name: "nginx-1".to_string(),
            pod_ip: "10.0.0.1".parse().ok(),
            node_name: None,
            labels: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            status: PodStatus::Running,
            containers: vec![ContainerStatus {
                name: "nginx".to_string(),
                ready: false,
                restart_count: 0,
            }],
        };

        engine.pod_cache().add_pod(pod).await.unwrap();

        // Discover backends - should be empty
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let backends = engine
            .discover_backends("default", &selector, 8080, "TCP")
            .await
            .unwrap();

        assert_eq!(backends.len(), 0);
    }

    #[tokio::test]
    async fn test_backend_pool_update() {
        let engine = BackendMappingEngine::new();

        let backends = vec![DiscoveredBackend {
            pod_ip: "10.0.0.1".parse().unwrap(),
            port: 8080,
            pod_name: "nginx-1".to_string(),
            namespace: "default".to_string(),
            node_name: Some("node-1".to_string()),
            healthy: true,
        }];

        engine
            .update_backend_pool("default/nginx".to_string(), backends)
            .await
            .unwrap();

        let pool = engine.get_pool("default/nginx").await;
        assert!(pool.is_some());
        assert_eq!(pool.unwrap().backends.len(), 1);
    }

    #[tokio::test]
    async fn test_backend_pool_healthy_backends() {
        let engine = BackendMappingEngine::new();

        let backends = vec![
            DiscoveredBackend {
                pod_ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
                pod_name: "nginx-1".to_string(),
                namespace: "default".to_string(),
                node_name: None,
                healthy: true,
            },
            DiscoveredBackend {
                pod_ip: "10.0.0.2".parse().unwrap(),
                port: 8080,
                pod_name: "nginx-2".to_string(),
                namespace: "default".to_string(),
                node_name: None,
                healthy: false,
            },
        ];

        engine
            .update_backend_pool("default/nginx".to_string(), backends)
            .await
            .unwrap();

        let healthy = engine.get_healthy_backends("default/nginx").await;
        assert_eq!(healthy.len(), 1);
    }

    #[test]
    fn test_selector_matches() {
        let pod_labels = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();

        assert!(selector_matches(&pod_labels, &selector));
    }

    #[test]
    fn test_selector_no_match() {
        let pod_labels = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let selector = [("app".to_string(), "postgres".to_string())]
            .iter()
            .cloned()
            .collect();

        assert!(!selector_matches(&pod_labels, &selector));
    }

    #[test]
    fn test_backend_pool_add_remove() {
        let mut pool = BackendPool::new(SelectionAlgorithm::RoundRobin);
        let id1 = pool.add(addr("10.0.0.1:8080"));
        let id2 = pool.add(addr("10.0.0.2:8080"));
        assert_eq!(pool.len(), 2);
        pool.remove(&id1);
        assert_eq!(pool.len(), 1);
        assert!(pool.get(&id2).is_some());
    }

    #[test]
    fn test_backend_pool_state_transition() {
        let mut pool = BackendPool::new(SelectionAlgorithm::RoundRobin);
        let id = pool.add(addr("10.0.0.1:80"));
        assert_eq!(pool.active_backends().len(), 1);
        pool.set_state(&id, BackendState::Quarantined);
        assert_eq!(pool.active_backends().len(), 0);
        pool.set_state(&id, BackendState::Active);
        assert_eq!(pool.active_backends().len(), 1);
    }

    #[test]
    fn test_round_robin_selector() {
        let backends: Vec<_> = (1u32..=3)
            .map(|i| Backend::new(BackendID::new(i), addr(&format!("10.0.0.{i}:80"))))
            .collect();
        let sel = RoundRobinSelector::new(backends);
        assert_eq!(sel.active_count(), 3);
        for _ in 0..9 {
            assert!(sel.select().is_some());
        }
    }

    #[test]
    fn test_source_hash_selector_stability() {
        let backends: Vec<_> = (1u32..=3)
            .map(|i| Backend::new(BackendID::new(i), addr(&format!("10.0.0.{i}:80"))))
            .collect();
        let sel = SourceHashSelector::new(backends);
        let src = addr("192.168.1.100:12345");
        let b1 = sel.select(&src).unwrap().id;
        let b2 = sel.select(&src).unwrap().id;
        assert_eq!(b1, b2);
    }

    #[test]
    fn test_pool_diff() {
        let mut old_pool = BackendPool::new(SelectionAlgorithm::RoundRobin);
        old_pool.add(addr("10.0.0.1:80"));
        old_pool.add(addr("10.0.0.2:80"));

        let mut new_pool = BackendPool::new(SelectionAlgorithm::RoundRobin);
        new_pool.add(addr("10.0.0.2:80"));
        new_pool.add(addr("10.0.0.3:80"));

        let (added, removed) = old_pool.diff(&new_pool);
        assert_eq!(added.len(), 1);
        assert_eq!(removed.len(), 1);
    }

    #[test]
    fn test_backend_display() {
        let id = BackendID::new(42);
        assert_eq!(id.to_string(), "backend-42");
        assert_eq!(BackendState::Active.to_string(), "active");
    }

    #[test]
    fn test_empty_pool_select() {
        let sel = RoundRobinSelector::new(vec![]);
        assert!(sel.select().is_none());
    }
}
