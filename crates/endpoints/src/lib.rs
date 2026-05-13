#![allow(clippy::cast_precision_loss)]
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
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
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
// Endpoint Registry
// ============================================================================

/// Ways to look up an endpoint in the manager.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EndpointLookupKey {
    /// By numeric ID.
    ID(u16),
    /// By IPv4 address.
    IPv4(Ipv4Addr),
    /// By IPv6 address.
    IPv6(Ipv6Addr),
    /// By container ID.
    ContainerID(String),
    /// By pod namespace and name.
    Pod {
        /// Kubernetes namespace.
        namespace: String,
        /// Kubernetes pod name.
        name: String,
    },
}

/// Snapshot of an endpoint's essential info held by the manager.
#[derive(Debug, Clone)]
pub struct EndpointEntry {
    /// Numeric endpoint identifier.
    pub id: u16,
    /// IPv4 address assigned to the endpoint.
    pub ipv4: Option<Ipv4Addr>,
    /// IPv6 address assigned to the endpoint.
    pub ipv6: Option<Ipv6Addr>,
    /// Container runtime identifier.
    pub container_id: Option<String>,
    /// Pod name associated with the endpoint.
    pub pod_name: Option<String>,
    /// Pod namespace associated with the endpoint.
    pub pod_namespace: Option<String>,
    /// Security identity associated with the endpoint.
    pub identity_id: u32,
    /// Endpoint labels in `source:key=value` form.
    pub labels: HashMap<String, String>,
}

impl EndpointEntry {
    /// Creates a new endpoint entry with empty optional metadata.
    pub fn new(id: u16, identity_id: u32) -> Self {
        Self {
            id,
            identity_id,
            ipv4: None,
            ipv6: None,
            container_id: None,
            pod_name: None,
            pod_namespace: None,
            labels: Default::default(),
        }
    }

    /// Returns every lookup key currently exposed by this entry.
    pub fn lookup_keys(&self) -> Vec<EndpointLookupKey> {
        let mut keys = vec![EndpointLookupKey::ID(self.id)];
        if let Some(ip) = self.ipv4 {
            keys.push(EndpointLookupKey::IPv4(ip));
        }
        if let Some(ip) = self.ipv6 {
            keys.push(EndpointLookupKey::IPv6(ip));
        }
        if let Some(cid) = &self.container_id {
            keys.push(EndpointLookupKey::ContainerID(cid.clone()));
        }
        if let (Some(ns), Some(name)) = (&self.pod_namespace, &self.pod_name) {
            keys.push(EndpointLookupKey::Pod {
                namespace: ns.clone(),
                name: name.clone(),
            });
        }
        keys
    }
}

/// Error types for the endpoint registry manager.
#[derive(Debug, thiserror::Error)]
pub enum EndpointManagerError {
    /// Returned when a lookup key does not resolve to a registered endpoint.
    #[error("endpoint not found: {0:?}")]
    NotFound(EndpointLookupKey),
    /// Returned when an endpoint ID is already present in the registry.
    #[error("endpoint ID {0} already exists")]
    AlreadyExists(u16),
    /// Returned when the local endpoint ID space is fully consumed.
    #[error("endpoint ID space exhausted")]
    IDExhausted,
}

/// Registry of all endpoints on this node.
///
/// Provides O(1) lookup by ID, IP, container ID, or pod name.
pub struct EndpointManager {
    /// Primary store: ID -> entry.
    by_id: HashMap<u16, EndpointEntry>,
    /// Secondary index: IPv4 -> ID.
    by_ipv4: HashMap<Ipv4Addr, u16>,
    /// Secondary index: IPv6 -> ID.
    by_ipv6: HashMap<Ipv6Addr, u16>,
    /// Secondary index: container ID -> ID.
    by_container_id: HashMap<String, u16>,
    /// Secondary index: (namespace, pod name) -> ID.
    by_pod: HashMap<(String, String), u16>,
    /// Next endpoint ID candidate for automatic allocation.
    next_id: u16,
}

impl EndpointManager {
    /// Creates an empty endpoint registry.
    pub fn new() -> Self {
        Self {
            by_id: Default::default(),
            by_ipv4: Default::default(),
            by_ipv6: Default::default(),
            by_container_id: Default::default(),
            by_pod: Default::default(),
            next_id: 1,
        }
    }

    /// Allocates an ID when needed and registers an endpoint entry.
    pub fn add(
        &mut self,
        mut entry: EndpointEntry,
    ) -> std::result::Result<u16, EndpointManagerError> {
        if entry.id == 0 {
            entry.id = self.allocate_id()?;
        } else if self.by_id.contains_key(&entry.id) {
            return Err(EndpointManagerError::AlreadyExists(entry.id));
        }

        let id = entry.id;
        if let Some(ip) = entry.ipv4 {
            self.by_ipv4.insert(ip, id);
        }
        if let Some(ip) = entry.ipv6 {
            self.by_ipv6.insert(ip, id);
        }
        if let Some(cid) = &entry.container_id {
            self.by_container_id.insert(cid.clone(), id);
        }
        if let (Some(ns), Some(name)) = (&entry.pod_namespace, &entry.pod_name) {
            self.by_pod.insert((ns.clone(), name.clone()), id);
        }
        self.by_id.insert(id, entry);
        debug!(endpoint_id = id, "registered endpoint entry");
        Ok(id)
    }

    /// Removes an endpoint and all of its secondary index entries.
    pub fn remove(&mut self, id: u16) -> Option<EndpointEntry> {
        let entry = self.by_id.remove(&id)?;
        if let Some(ip) = entry.ipv4 {
            self.by_ipv4.remove(&ip);
        }
        if let Some(ip) = entry.ipv6 {
            self.by_ipv6.remove(&ip);
        }
        if let Some(cid) = &entry.container_id {
            self.by_container_id.remove(cid);
        }
        if let (Some(ns), Some(name)) = (&entry.pod_namespace, &entry.pod_name) {
            self.by_pod.remove(&(ns.clone(), name.clone()));
        }
        debug!(endpoint_id = id, "removed endpoint entry");
        Some(entry)
    }

    /// Looks up an endpoint by any supported key.
    pub fn lookup(&self, key: &EndpointLookupKey) -> Option<&EndpointEntry> {
        let id = match key {
            EndpointLookupKey::ID(id) => *id,
            EndpointLookupKey::IPv4(ip) => *self.by_ipv4.get(ip)?,
            EndpointLookupKey::IPv6(ip) => *self.by_ipv6.get(ip)?,
            EndpointLookupKey::ContainerID(cid) => *self.by_container_id.get(cid)?,
            EndpointLookupKey::Pod { namespace, name } => {
                *self.by_pod.get(&(namespace.clone(), name.clone()))?
            }
        };
        self.by_id.get(&id)
    }

    /// Returns an endpoint by numeric ID.
    pub fn get(&self, id: u16) -> Option<&EndpointEntry> {
        self.by_id.get(&id)
    }

    /// Returns a mutable endpoint by numeric ID.
    pub fn get_mut(&mut self, id: u16) -> Option<&mut EndpointEntry> {
        self.by_id.get_mut(&id)
    }

    /// Returns an iterator over all registered endpoints.
    pub fn all(&self) -> impl Iterator<Item = &EndpointEntry> {
        self.by_id.values()
    }

    /// Returns the number of registered endpoints.
    pub fn count(&self) -> usize {
        self.by_id.len()
    }

    /// Returns endpoints that share the provided identity.
    pub fn by_identity(&self, identity_id: u32) -> Vec<&EndpointEntry> {
        self.by_id
            .values()
            .filter(|entry| entry.identity_id == identity_id)
            .collect()
    }

    fn allocate_id(&mut self) -> std::result::Result<u16, EndpointManagerError> {
        for _ in 0..u16::MAX {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if !self.by_id.contains_key(&id) {
                return Ok(id);
            }
        }
        Err(EndpointManagerError::IDExhausted)
    }
}

impl Default for EndpointManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Events emitted by the endpoint manager.
#[derive(Debug, Clone)]
pub enum EndpointManagerEvent {
    /// An endpoint was added to the registry.
    EndpointAdded(u16),
    /// An endpoint was removed from the registry.
    EndpointRemoved(u16),
    /// An endpoint's identity changed.
    IdentityUpdated {
        /// Endpoint whose identity changed.
        endpoint_id: u16,
        /// Previous identity value.
        old_identity: u32,
        /// New identity value.
        new_identity: u32,
    },
}

// ============================================================================
// Pod Lifecycle Manager
// ============================================================================

/// Manages endpoint lifecycle.
pub struct PodLifecycleManager {
    cache: Arc<EndpointCache>,
    ipam: Arc<IPAMManager>,
}

impl PodLifecycleManager {
    /// Creates a lifecycle manager backed by the provided cache and IPAM allocator.
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
        let endpoint_id = format!("{namespace}/{pod_name}");

        let mut endpoint = Endpoint::new(endpoint_id, pod_id, namespace, pod_name, labels);
        endpoint = endpoint.with_ipv4(ip);
        endpoint.health = HealthStatus::Healthy;

        self.cache.add_endpoint(endpoint.clone()).await;
        Ok(endpoint)
    }

    /// Pod deleted
    pub async fn on_pod_deleted(&self, namespace: &str, pod_name: &str) -> Result<()> {
        let endpoint_id = format!("{namespace}/{pod_name}");

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
        let healthy = endpoints
            .iter()
            .filter(|ep| ep.health == HealthStatus::Healthy)
            .count();
        let unhealthy = endpoints
            .iter()
            .filter(|ep| ep.health == HealthStatus::Unhealthy)
            .count();
        let unknown = endpoints
            .iter()
            .filter(|ep| ep.health == HealthStatus::Unknown)
            .count();

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

    fn make_entry(id: u16) -> EndpointEntry {
        let mut entry = EndpointEntry::new(id, 100);
        entry.ipv4 = Some("10.0.0.1".parse().unwrap());
        entry.container_id = Some("abc123".into());
        entry.pod_name = Some("nginx".into());
        entry.pod_namespace = Some("default".into());
        entry
    }

    #[test]
    fn test_add_and_lookup_by_id() {
        let mut manager = EndpointManager::new();
        manager.add(make_entry(1)).unwrap();
        assert!(manager.lookup(&EndpointLookupKey::ID(1)).is_some());
        assert!(manager.lookup(&EndpointLookupKey::ID(99)).is_none());
    }

    #[test]
    fn test_lookup_by_ipv4() {
        let mut manager = EndpointManager::new();
        manager.add(make_entry(1)).unwrap();
        let ip = "10.0.0.1".parse().unwrap();
        assert!(manager.lookup(&EndpointLookupKey::IPv4(ip)).is_some());
    }

    #[test]
    fn test_lookup_by_pod() {
        let mut manager = EndpointManager::new();
        manager.add(make_entry(1)).unwrap();
        let key = EndpointLookupKey::Pod {
            namespace: "default".into(),
            name: "nginx".into(),
        };
        assert!(manager.lookup(&key).is_some());
    }

    #[test]
    fn test_lookup_by_container_id() {
        let mut manager = EndpointManager::new();
        manager.add(make_entry(1)).unwrap();
        let key = EndpointLookupKey::ContainerID("abc123".into());
        assert!(manager.lookup(&key).is_some());
    }

    #[test]
    fn test_remove_cleans_indexes() {
        let mut manager = EndpointManager::new();
        let entry = make_entry(1);
        let ip = entry.ipv4.unwrap();
        manager.add(entry).unwrap();
        manager.remove(1);
        assert!(manager.lookup(&EndpointLookupKey::ID(1)).is_none());
        assert!(manager.lookup(&EndpointLookupKey::IPv4(ip)).is_none());
    }

    #[test]
    fn test_duplicate_id_rejected() {
        let mut manager = EndpointManager::new();
        manager.add(make_entry(5)).unwrap();
        let result = manager.add(make_entry(5));
        assert!(result.is_err());
    }

    #[test]
    fn test_by_identity() {
        let mut manager = EndpointManager::new();
        let mut entry_one = EndpointEntry::new(1, 200);
        entry_one.ipv4 = Some("10.0.0.1".parse().unwrap());
        let mut entry_two = EndpointEntry::new(2, 200);
        entry_two.ipv4 = Some("10.0.0.2".parse().unwrap());
        let entry_three = EndpointEntry::new(3, 999);
        manager.add(entry_one).unwrap();
        manager.add(entry_two).unwrap();
        manager.add(entry_three).unwrap();
        assert_eq!(manager.by_identity(200).len(), 2);
        assert_eq!(manager.by_identity(999).len(), 1);
        assert_eq!(manager.by_identity(0).len(), 0);
    }

    #[test]
    fn test_auto_id_allocation() {
        let mut manager = EndpointManager::new();
        let mut entry = EndpointEntry::new(0, 100);
        entry.ipv4 = Some("10.0.0.99".parse().unwrap());
        let assigned = manager.add(entry).unwrap();
        assert!(assigned > 0);
        assert!(manager.get(assigned).is_some());
    }

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
        assert_eq!(
            cache.get_endpoints_by_namespace("kube-system").await.len(),
            3
        );
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
        let manager = PodLifecycleManager::new(cache, ipam.clone());

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
        let manager = PodLifecycleManager::new(cache, ipam);

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
