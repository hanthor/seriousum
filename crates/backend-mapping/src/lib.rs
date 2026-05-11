//! Backend Mapping Engine - connects services to their endpoints
//!
//! Implements Issue #46 (P1.3)
//!
//! This component integrates:
//! - Track 1 (ServiceObserver): Receives service change notifications
//! - Track 2 (eBPF Maps): Populates backend pools
//! - Pod cache: Discovers endpoints matching service selectors
//! - Health tracking: Monitors pod readiness

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
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

/// Backend endpoint for service
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct Backend {
    pub pod_ip: IpAddr,
    pub port: u16,
    pub pod_name: String,
    pub namespace: String,
    pub node_name: Option<String>,
    pub healthy: bool,
}

impl Backend {
    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.pod_name)
    }
}

/// Backend pool for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendPool {
    pub service_key: String,
    pub backends: Vec<Backend>,
    pub healthy_count: usize,
    pub generation: u64,
}

impl BackendPool {
    pub fn new(service_key: String) -> Self {
        Self {
            service_key,
            backends: Vec::new(),
            healthy_count: 0,
            generation: 0,
        }
    }

    pub fn get_healthy(&self) -> Vec<&Backend> {
        self.backends.iter().filter(|b| b.healthy).collect()
    }

    pub fn update_backends(&mut self, backends: Vec<Backend>) {
        self.backends = backends;
        self.healthy_count = self.backends.iter().filter(|b| b.healthy).count();
        self.generation = self.generation.wrapping_add(1);
    }
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
            .filter(|p| {
                p.namespace == namespace && selector_matches(&p.labels, selector)
            })
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

/// Main backend mapping engine
pub struct BackendMappingEngine {
    pod_cache: PodCache,
    pools: Arc<RwLock<HashMap<String, BackendPool>>>,
}

impl BackendMappingEngine {
    pub fn new() -> Self {
        Self {
            pod_cache: PodCache::new(),
            pools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn pod_cache(&self) -> &PodCache {
        &self.pod_cache
    }

    /// Discover backends for a service
    pub async fn discover_backends(
        &self,
        namespace: &str,
        service_selector: &HashMap<String, String>,
        target_port: u16,
        _protocol: &str,
    ) -> Result<Vec<Backend>> {
        // Find pods matching selector
        let pods = self
            .pod_cache
            .find_pods_with_labels(namespace, service_selector)
            .await;

        let backends: Vec<Backend> = pods
            .iter()
            .filter_map(|pod| {
                // Only include running pods with valid IPs
                if pod.status != PodStatus::Running || pod.pod_ip.is_none() {
                    return None;
                }

                // Check if containers are ready
                let all_ready = pod.containers.iter().all(|c| c.ready);
                if !all_ready {
                    return None;
                }

                Some(Backend {
                    pod_ip: pod.pod_ip.unwrap(),
                    port: target_port,
                    pod_name: pod.name.clone(),
                    namespace: pod.namespace.clone(),
                    node_name: pod.node_name.clone(),
                    healthy: true,
                })
            })
            .collect();

        debug!(
            "Discovered {} backends for {}/{} (selector: {:?})",
            backends.len(),
            namespace,
            service_selector
                .values()
                .next()
                .unwrap_or(&"*".to_string()),
            service_selector
        );

        Ok(backends)
    }

    /// Update backend pool for a service
    pub async fn update_backend_pool(
        &self,
        service_key: String,
        backends: Vec<Backend>,
    ) -> Result<()> {
        let mut pools = self.pools.write().await;
        let pool = pools
            .entry(service_key.clone())
            .or_insert_with(|| BackendPool::new(service_key.clone()));
        pool.update_backends(backends);
        Ok(())
    }

    /// Get backend pool for a service
    pub async fn get_pool(&self, service_key: &str) -> Option<BackendPool> {
        let pools = self.pools.read().await;
        pools.get(service_key).cloned()
    }

    /// Get healthy backends for a service
    pub async fn get_healthy_backends(&self, service_key: &str) -> Vec<Backend> {
        let pools = self.pools.read().await;
        pools
            .get(service_key)
            .map(|pool| pool.get_healthy().iter().map(|b| (*b).clone()).collect())
            .unwrap_or_default()
    }

    /// Get all backends for a service
    pub async fn get_backends(&self, service_key: &str) -> Vec<Backend> {
        let pools = self.pools.read().await;
        pools
            .get(service_key)
            .map(|pool| pool.backends.clone())
            .unwrap_or_default()
    }

    /// Delete backend pool
    pub async fn delete_pool(&self, service_key: &str) -> Result<()> {
        let mut pools = self.pools.write().await;
        pools
            .remove(service_key)
            .ok_or_else(|| anyhow!("Pool not found: {}", service_key))?;
        Ok(())
    }

    /// List all backend pools
    pub async fn list_pools(&self) -> Vec<BackendPool> {
        let pools = self.pools.read().await;
        pools.values().cloned().collect()
    }

    /// Get pool count
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
    selector.iter().all(|(key, value)| {
        pod_labels.get(key).map(|v| v == value).unwrap_or(false)
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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

        let backends = vec![Backend {
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
            Backend {
                pod_ip: "10.0.0.1".parse().unwrap(),
                port: 8080,
                pod_name: "nginx-1".to_string(),
                namespace: "default".to_string(),
                node_name: None,
                healthy: true,
            },
            Backend {
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
}
