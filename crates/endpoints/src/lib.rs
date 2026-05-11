//! Endpoint Lifecycle - Manages pod endpoints and IP allocation
//!
//! Implements Issue #50 (P2.2): Endpoint Lifecycle Management
//!
//! This component:
//! - Tracks pod lifecycle events
//! - Allocates and manages pod IP addresses
//! - Maintains endpoint metadata
//! - Integrates with policy subsystem
//! - Monitors endpoint health

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Result};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

// ============================================================================
// Data Structures
// ============================================================================

/// Pod endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub pod_id: String,
    pub namespace: String,
    pub pod_name: String,
    pub ipv4: Option<IpAddr>,
    pub ipv6: Option<IpAddr>,
    pub labels: HashMap<String, String>,
    pub health: HealthStatus,
}

impl Endpoint {
    pub fn new(
        id: String,
        pod_id: String,
        namespace: String,
        pod_name: String,
        labels: HashMap<String, String>,
    ) -> Self {
        Self {
            id,
            pod_id,
            namespace,
            pod_name,
            ipv4: None,
            ipv6: None,
            labels,
            health: HealthStatus::Unknown,
        }
    }

    pub fn with_ipv4(mut self, ip: IpAddr) -> Self {
        self.ipv4 = Some(ip);
        self
    }

    pub fn with_ipv6(mut self, ip: IpAddr) -> Self {
        self.ipv6 = Some(ip);
        self
    }
}

/// Endpoint health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
    Unknown,
}

// ============================================================================
// Endpoint Cache
// ============================================================================

/// In-memory cache of endpoints
pub struct EndpointCache {
    endpoints: Arc<RwLock<HashMap<String, Endpoint>>>,
}

impl EndpointCache {
    pub fn new() -> Self {
        Self {
            endpoints: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add endpoint
    pub async fn add_endpoint(&self, endpoint: Endpoint) {
        let mut endpoints = self.endpoints.write().await;
        endpoints.insert(endpoint.id.clone(), endpoint.clone());
        debug!("Added endpoint: {}", endpoint.id);
    }

    /// Remove endpoint
    pub async fn remove_endpoint(&self, id: &str) -> Option<Endpoint> {
        let mut endpoints = self.endpoints.write().await;
        let removed = endpoints.remove(id);
        if removed.is_some() {
            debug!("Removed endpoint: {}", id);
        }
        removed
    }

    /// Get endpoint by ID
    pub async fn get_endpoint(&self, id: &str) -> Option<Endpoint> {
        let endpoints = self.endpoints.read().await;
        endpoints.get(id).cloned()
    }

    /// List all endpoints
    pub async fn list_endpoints(&self) -> Vec<Endpoint> {
        let endpoints = self.endpoints.read().await;
        endpoints.values().cloned().collect()
    }

    /// Get endpoints for a namespace
    pub async fn get_endpoints_by_namespace(&self, namespace: &str) -> Vec<Endpoint> {
        let endpoints = self.endpoints.read().await;
        endpoints
            .values()
            .filter(|ep| ep.namespace == namespace)
            .cloned()
            .collect()
    }

    /// Count endpoints
    pub async fn endpoint_count(&self) -> usize {
        self.endpoints.read().await.len()
    }

    /// Get healthy endpoints
    pub async fn get_healthy_endpoints(&self) -> Vec<Endpoint> {
        let endpoints = self.endpoints.read().await;
        endpoints
            .values()
            .filter(|ep| ep.health == HealthStatus::Healthy)
            .cloned()
            .collect()
    }
}

impl Default for EndpointCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// IP Address Allocation Manager
// ============================================================================

/// IPAM configuration
#[derive(Debug, Clone)]
pub struct IPAMConfig {
    pub start_ip: Ipv4Addr,
    pub end_ip: Ipv4Addr,
}

impl Default for IPAMConfig {
    fn default() -> Self {
        Self {
            start_ip: Ipv4Addr::new(10, 0, 0, 2),
            end_ip: Ipv4Addr::new(10, 255, 255, 254),
        }
    }
}

/// Allocates IP addresses to endpoints
pub struct IPAMManager {
    config: IPAMConfig,
    allocated_ips: Arc<RwLock<std::collections::HashSet<u32>>>,
}

impl IPAMManager {
    pub fn new(config: IPAMConfig) -> Self {
        Self {
            config,
            allocated_ips: Arc::new(RwLock::new(std::collections::HashSet::new())),
        }
    }

    /// Allocate an IP address
    pub async fn allocate_ip(&self) -> Result<IpAddr> {
        let start = u32::from(self.config.start_ip);
        let end = u32::from(self.config.end_ip);

        let mut allocated = self.allocated_ips.write().await;

        for ip_int in start..=end {
            if !allocated.contains(&ip_int) {
                allocated.insert(ip_int);
                let ip = Ipv4Addr::from(ip_int);
                debug!("Allocated IP: {}", ip);
                return Ok(IpAddr::V4(ip));
            }
        }

        Err(Error::Ipam("IP pool exhausted".to_string()))
    }

    /// Release an IP address
    pub async fn release_ip(&self, ip: IpAddr) -> Result<()> {
        if let IpAddr::V4(ipv4) = ip {
            let mut allocated = self.allocated_ips.write().await;
            allocated.remove(&u32::from(ipv4));
            debug!("Released IP: {}", ip);
            Ok(())
        } else {
            Err(Error::Ipam("Invalid IP".to_string()))
        }
    }

    /// Check if IP is allocated
    pub async fn is_allocated(&self, ip: IpAddr) -> bool {
        if let IpAddr::V4(ipv4) = ip {
            let allocated = self.allocated_ips.read().await;
            allocated.contains(&u32::from(ipv4))
        } else {
            false
        }
    }

    /// Get allocation metrics
    pub async fn get_metrics(&self) -> IPAMMetrics {
        let allocated = self.allocated_ips.read().await;
        let start = u32::from(self.config.start_ip);
        let end = u32::from(self.config.end_ip);
        let total = (end - start + 1) as usize;
        let used = allocated.len();

        IPAMMetrics {
            total,
            allocated: used,
            available: total - used,
            utilization: (used as f64 / total as f64) * 100.0,
        }
    }
}

impl Default for IPAMManager {
    fn default() -> Self {
        Self::new(IPAMConfig::default())
    }
}

/// IPAM metrics
#[derive(Debug, Clone)]
pub struct IPAMMetrics {
    pub total: usize,
    pub allocated: usize,
    pub available: usize,
    pub utilization: f64,
}

// ============================================================================
// Endpoint Manager
// ============================================================================

/// Manages endpoint lifecycle
pub struct EndpointManager {
    cache: Arc<EndpointCache>,
    ipam: Arc<IPAMManager>,
}

impl EndpointManager {
    pub fn new(cache: Arc<EndpointCache>, ipam: Arc<IPAMManager>) -> Self {
        Self { cache, ipam }
    }

    /// Pod added
    pub async fn on_pod_added(
        &self,
        namespace: String,
        pod_name: String,
        pod_id: String,
        labels: HashMap<String, String>,
    ) -> Result<Endpoint> {
        let ip = self.ipam.allocate_ip().await?;
        let endpoint_id = format!("{}/{}", namespace, pod_name);

        let mut endpoint = Endpoint::new(endpoint_id, pod_id, namespace, pod_name, labels);
        endpoint = endpoint.with_ipv4(ip);
        endpoint.health = HealthStatus::Healthy;

        self.cache.add_endpoint(endpoint.clone()).await;
        Ok(endpoint)
    }

    /// Pod deleted
    pub async fn on_pod_deleted(&self, namespace: &str, pod_name: &str) -> Result<()> {
        let endpoint_id = format!("{}/{}", namespace, pod_name);

        if let Some(endpoint) = self.cache.remove_endpoint(&endpoint_id).await {
            if let Some(ip) = endpoint.ipv4 {
                self.ipam.release_ip(ip).await?;
            }
            debug!("Removed endpoint and released IP: {}", endpoint_id);
        }

        Ok(())
    }

    /// Update endpoint health
    pub async fn set_endpoint_health(&self, id: &str, health: HealthStatus) {
        if let Some(mut ep) = self.cache.get_endpoint(id).await {
            ep.health = health;
            self.cache.add_endpoint(ep).await;
        }
    }

    /// Get endpoint metrics
    pub async fn get_metrics(&self) -> EndpointMetrics {
        let endpoints = self.cache.list_endpoints().await;
        let healthy = endpoints.iter().filter(|ep| ep.health == HealthStatus::Healthy).count();
        let unhealthy = endpoints.iter().filter(|ep| ep.health == HealthStatus::Unhealthy).count();
        let unknown = endpoints.iter().filter(|ep| ep.health == HealthStatus::Unknown).count();

        EndpointMetrics {
            total: endpoints.len(),
            healthy,
            unhealthy,
            unknown,
        }
    }
}

/// Endpoint metrics
#[derive(Debug, Clone)]
pub struct EndpointMetrics {
    pub total: usize,
    pub healthy: usize,
    pub unhealthy: usize,
    pub unknown: usize,
}

// ============================================================================
// Health Tracker
// ============================================================================

/// Tracks endpoint health
pub struct HealthTracker {
    endpoint_health: Arc<RwLock<HashMap<String, HealthStatus>>>,
}

impl HealthTracker {
    pub fn new() -> Self {
        Self {
            endpoint_health: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Mark endpoint as healthy
    pub async fn set_healthy(&self, ep_id: &str) {
        let mut health = self.endpoint_health.write().await;
        health.insert(ep_id.to_string(), HealthStatus::Healthy);
        debug!("Marked healthy: {}", ep_id);
    }

    /// Mark endpoint as unhealthy
    pub async fn set_unhealthy(&self, ep_id: &str) {
        let mut health = self.endpoint_health.write().await;
        health.insert(ep_id.to_string(), HealthStatus::Unhealthy);
        debug!("Marked unhealthy: {}", ep_id);
    }

    /// Get health status
    pub async fn get_health(&self, ep_id: &str) -> HealthStatus {
        let health = self.endpoint_health.read().await;
        health.get(ep_id).copied().unwrap_or(HealthStatus::Unknown)
    }

    /// Get all healthy endpoints
    pub async fn get_healthy_endpoints(&self) -> Vec<String> {
        let health = self.endpoint_health.read().await;
        health
            .iter()
            .filter(|(_, status)| **status == HealthStatus::Healthy)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn endpoint_creation() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let endpoint = Endpoint::new(
            "default/pod-1".to_string(),
            "pod-uuid-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            labels,
        );

        assert_eq!(endpoint.id, "default/pod-1");
        assert_eq!(endpoint.health, HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn endpoint_cache_add_and_get() {
        let cache = EndpointCache::new();
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let endpoint = Endpoint::new(
            "default/pod-1".to_string(),
            "pod-uuid-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            labels,
        );

        cache.add_endpoint(endpoint.clone()).await;
        assert_eq!(cache.endpoint_count().await, 1);

        let retrieved = cache.get_endpoint("default/pod-1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "default/pod-1");
    }

    #[tokio::test]
    async fn endpoint_cache_remove() {
        let cache = EndpointCache::new();
        let endpoint = Endpoint::new(
            "default/pod-1".to_string(),
            "pod-uuid-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            HashMap::new(),
        );

        cache.add_endpoint(endpoint).await;
        assert_eq!(cache.endpoint_count().await, 1);

        cache.remove_endpoint("default/pod-1").await;
        assert_eq!(cache.endpoint_count().await, 0);
    }

    #[tokio::test]
    async fn endpoint_cache_by_namespace() {
        let cache = EndpointCache::new();

        for ns in &["default", "kube-system"] {
            for i in 0..3 {
                let endpoint = Endpoint::new(
                    format!("{}/pod-{}", ns, i),
                    format!("pod-uuid-{}", i),
                    ns.to_string(),
                    format!("pod-{}", i),
                    HashMap::new(),
                );
                cache.add_endpoint(endpoint).await;
            }
        }

        assert_eq!(cache.endpoint_count().await, 6);
        assert_eq!(cache.get_endpoints_by_namespace("default").await.len(), 3);
        assert_eq!(cache.get_endpoints_by_namespace("kube-system").await.len(), 3);
    }

    #[tokio::test]
    async fn ipam_allocate_and_release() {
        let ipam = IPAMManager::default();

        let ip1 = ipam.allocate_ip().await.unwrap();
        assert!(ipam.is_allocated(ip1).await);

        let ip2 = ipam.allocate_ip().await.unwrap();
        assert_ne!(ip1, ip2);

        ipam.release_ip(ip1).await.unwrap();
        assert!(!ipam.is_allocated(ip1).await);
    }

    #[tokio::test]
    async fn ipam_metrics() {
        let ipam = IPAMManager::default();

        let _ip1 = ipam.allocate_ip().await.unwrap();
        let _ip2 = ipam.allocate_ip().await.unwrap();

        let metrics = ipam.get_metrics().await;
        assert_eq!(metrics.allocated, 2);
        assert!(metrics.utilization > 0.0);
    }

    #[tokio::test]
    async fn endpoint_manager_pod_lifecycle() {
        let cache = Arc::new(EndpointCache::new());
        let ipam = Arc::new(IPAMManager::default());
        let manager = EndpointManager::new(cache, ipam.clone());

        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let endpoint = manager
            .on_pod_added(
                "default".to_string(),
                "pod-1".to_string(),
                "pod-uuid-1".to_string(),
                labels,
            )
            .await
            .unwrap();

        assert!(endpoint.ipv4.is_some());
        assert_eq!(endpoint.health, HealthStatus::Healthy);

        manager.on_pod_deleted("default", "pod-1").await.unwrap();
        assert!(!ipam.is_allocated(endpoint.ipv4.unwrap()).await);
    }

    #[tokio::test]
    async fn endpoint_manager_metrics() {
        let cache = Arc::new(EndpointCache::new());
        let ipam = Arc::new(IPAMManager::default());
        let manager = EndpointManager::new(cache, ipam);

        for i in 0..3 {
            let _ = manager
                .on_pod_added(
                    "default".to_string(),
                    format!("pod-{}", i),
                    format!("pod-uuid-{}", i),
                    HashMap::new(),
                )
                .await;
        }

        let metrics = manager.get_metrics().await;
        assert_eq!(metrics.total, 3);
        assert_eq!(metrics.healthy, 3);
    }

    #[tokio::test]
    async fn health_tracker_transitions() {
        let tracker = HealthTracker::new();

        tracker.set_healthy("ep-1").await;
        assert_eq!(tracker.get_health("ep-1").await, HealthStatus::Healthy);

        tracker.set_unhealthy("ep-1").await;
        assert_eq!(tracker.get_health("ep-1").await, HealthStatus::Unhealthy);
    }

    #[tokio::test]
    async fn health_tracker_healthy_list() {
        let tracker = HealthTracker::new();

        tracker.set_healthy("ep-1").await;
        tracker.set_healthy("ep-2").await;
        tracker.set_unhealthy("ep-3").await;

        let healthy = tracker.get_healthy_endpoints().await;
        assert_eq!(healthy.len(), 2);
    }
}
