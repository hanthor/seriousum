//! Service Observer - watches Kubernetes services and tracks lifecycle
//!
//! Implements Issue #44 (P1.1): Service Observer component
//!
//! Key components:
//! - Service watcher (K8s event listener)
//! - Service cache (in-memory store with fast queries)
//! - Event dispatcher (notifies on add/update/delete)
//! - Integration with backend mapper

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

// ============================================================================
// Core Types
// ============================================================================

/// Service information (mirrors K8s Service spec)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub namespace: String,
    pub name: String,
    pub service_type: ServiceType,
    pub cluster_ip: Option<IpAddr>,
    pub selector: HashMap<String, String>,
    pub ports: Vec<ServicePort>,
    pub session_affinity: SessionAffinity,
    pub load_balancer_ip: Option<IpAddr>,
    pub external_ips: Vec<IpAddr>,
}

/// Kubernetes service type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceType {
    ClusterIP,
    NodePort,
    LoadBalancer,
    ExternalName,
}

/// Service port definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePort {
    pub name: Option<String>,
    pub protocol: String, // "TCP", "UDP"
    pub port: u16,        // Service port
    pub target_port: u16, // Pod port
    pub node_port: Option<u16>,
}

/// Session affinity for client IP tracking
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SessionAffinity {
    None,
    ClientIP { timeout_seconds: u32 },
}

impl Default for SessionAffinity {
    fn default() -> Self {
        Self::None
    }
}

/// Cache key: "namespace/name"
pub type ServiceKey = String;

impl ServiceInfo {
    pub fn key(&self) -> ServiceKey {
        format!("{}/{}", self.namespace, self.name)
    }
}

// ============================================================================
// Service Cache
// ============================================================================

/// In-memory cache of services with fast lookup
#[derive(Debug, Clone)]
pub struct ServiceCache {
    services: Arc<RwLock<HashMap<ServiceKey, ServiceInfo>>>,
    version: Arc<RwLock<u64>>, // Incremental version for watch support
}

impl ServiceCache {
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            version: Arc::new(RwLock::new(0)),
        }
    }

    /// Add a service to cache
    pub async fn add_service(&self, svc: ServiceInfo) -> Result<()> {
        let key = svc.key();
        let mut services = self.services.write().await;

        if services.contains_key(&key) {
            return Err(anyhow!("Service already exists: {}", key));
        }

        services.insert(key.clone(), svc.clone());
        
        // Bump version
        let mut version = self.version.write().await;
        *version = version.wrapping_add(1);

        debug!("Added service: {}", key);
        Ok(())
    }

    /// Update a service in cache
    pub async fn update_service(&self, svc: ServiceInfo) -> Result<()> {
        let key = svc.key();
        let mut services = self.services.write().await;

        if !services.contains_key(&key) {
            return Err(anyhow!("Service not found: {}", key));
        }

        services.insert(key.clone(), svc.clone());

        // Bump version
        let mut version = self.version.write().await;
        *version = version.wrapping_add(1);

        debug!("Updated service: {}", key);
        Ok(())
    }

    /// Delete a service from cache
    pub async fn delete_service(&self, key: &ServiceKey) -> Result<()> {
        let mut services = self.services.write().await;

        if !services.remove(key).is_some() {
            return Err(anyhow!("Service not found: {}", key));
        }

        // Bump version
        let mut version = self.version.write().await;
        *version = version.wrapping_add(1);

        debug!("Deleted service: {}", key);
        Ok(())
    }

    /// Get service by key
    pub async fn get_service(&self, key: &ServiceKey) -> Option<ServiceInfo> {
        let services = self.services.read().await;
        services.get(key).cloned()
    }

    /// Get service by namespace and name
    pub async fn get_service_by_name(
        &self,
        namespace: &str,
        name: &str,
    ) -> Option<ServiceInfo> {
        let key = format!("{}/{}", namespace, name);
        self.get_service(&key).await
    }

    /// Get service by cluster IP
    pub async fn get_service_by_ip(&self, ip: &IpAddr) -> Option<ServiceInfo> {
        let services = self.services.read().await;
        services
            .values()
            .find(|svc| svc.cluster_ip == Some(*ip))
            .cloned()
    }

    /// List all services
    pub async fn list_services(&self) -> Vec<ServiceInfo> {
        let services = self.services.read().await;
        services.values().cloned().collect()
    }

    /// Find services matching selector
    pub async fn find_services_by_selector(
        &self,
        selector: &HashMap<String, String>,
    ) -> Vec<ServiceInfo> {
        let services = self.services.read().await;
        services
            .values()
            .filter(|svc| selector_matches(&svc.selector, selector))
            .cloned()
            .collect()
    }

    /// List services in namespace
    pub async fn services_for_namespace(&self, namespace: &str) -> Vec<ServiceInfo> {
        let services = self.services.read().await;
        services
            .values()
            .filter(|svc| svc.namespace == namespace)
            .cloned()
            .collect()
    }

    /// Get current cache version
    pub async fn version(&self) -> u64 {
        *self.version.read().await
    }

    /// Get service count
    pub async fn service_count(&self) -> usize {
        self.services.read().await.len()
    }

    /// Clear all services (mainly for testing)
    pub async fn clear(&self) {
        self.services.write().await.clear();
        let mut version = self.version.write().await;
        *version = version.wrapping_add(1);
    }
}

impl Default for ServiceCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Watch Events
// ============================================================================

/// Watch event types
#[derive(Debug, Clone)]
pub enum WatchEvent {
    Added(ServiceInfo),
    Modified(ServiceInfo),
    Deleted(ServiceKey),
}

/// Event handler trait for watch notifications
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn on_service_added(&self, svc: &ServiceInfo) -> Result<()>;
    async fn on_service_updated(&self, svc: &ServiceInfo) -> Result<()>;
    async fn on_service_deleted(&self, key: &ServiceKey) -> Result<()>;
}

// ============================================================================
// Service Observer
// ============================================================================

/// Main service observer component
pub struct ServiceObserver {
    cache: ServiceCache,
    event_handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
    running: Arc<RwLock<bool>>,
}

impl ServiceObserver {
    pub fn new() -> Self {
        Self {
            cache: ServiceCache::new(),
            event_handlers: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Start watching services
    pub async fn start(&self) -> Result<()> {
        let mut running = self.running.write().await;
        if *running {
            return Err(anyhow!("Observer already running"));
        }

        info!("Starting service observer");
        *running = true;

        // TODO: Connect to K8s API and start watch
        // For now, just mark as running for testing

        Ok(())
    }

    /// Stop watching services
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.running.write().await;
        *running = false;

        info!("Stopped service observer");
        Ok(())
    }

    /// Check if observer is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Add a service event handler
    pub async fn register_handler(&self, handler: Arc<dyn EventHandler>) {
        let mut handlers = self.event_handlers.write().await;
        handlers.push(handler);
        debug!("Registered event handler (total: {})", handlers.len());
    }

    /// Remove all handlers
    pub async fn clear_handlers(&self) {
        self.event_handlers.write().await.clear();
    }

    /// Get service by namespace and name
    pub async fn get_service(&self, namespace: &str, name: &str) -> Option<ServiceInfo> {
        self.cache.get_service_by_name(namespace, name).await
    }

    /// List all services
    pub async fn list_services(&self) -> Vec<ServiceInfo> {
        self.cache.list_services().await
    }

    /// Find services matching selector
    pub async fn find_services(
        &self,
        selector: &HashMap<String, String>,
    ) -> Vec<ServiceInfo> {
        self.cache.find_services_by_selector(selector).await
    }

    /// Get service count
    pub async fn service_count(&self) -> usize {
        self.cache.service_count().await
    }

    // ========================================================================
    // Internal methods (public for testing, but not part of public API)
    // ========================================================================

    /// Internal: Add service and dispatch event
    #[allow(dead_code)]
    pub(crate) async fn add_service_internal(&self, svc: ServiceInfo) -> Result<()> {
        self.cache.add_service(svc.clone()).await?;

        // Dispatch event
        let handlers = self.event_handlers.read().await;
        for handler in handlers.iter() {
            if let Err(e) = handler.on_service_added(&svc).await {
                error!("Event handler error in on_service_added: {}", e);
            }
        }

        Ok(())
    }

    /// Internal: Update service and dispatch event
    #[allow(dead_code)]
    pub(crate) async fn update_service_internal(&self, svc: ServiceInfo) -> Result<()> {
        self.cache.update_service(svc.clone()).await?;

        // Dispatch event
        let handlers = self.event_handlers.read().await;
        for handler in handlers.iter() {
            if let Err(e) = handler.on_service_updated(&svc).await {
                error!("Event handler error in on_service_updated: {}", e);
            }
        }

        Ok(())
    }

    /// Internal: Delete service and dispatch event
    #[allow(dead_code)]
    pub(crate) async fn delete_service_internal(&self, key: &ServiceKey) -> Result<()> {
        self.cache.delete_service(key).await?;

        // Dispatch event
        let handlers = self.event_handlers.read().await;
        for handler in handlers.iter() {
            if let Err(e) = handler.on_service_deleted(key).await {
                error!("Event handler error in on_service_deleted: {}", e);
            }
        }

        Ok(())
    }
}

impl Default for ServiceObserver {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Check if service selector matches given labels
fn selector_matches(
    service_selector: &HashMap<String, String>,
    labels: &HashMap<String, String>,
) -> bool {
    // All selector labels must match in the given labels
    service_selector.iter().all(|(key, value)| {
        labels.get(key).map(|v| v == value).unwrap_or(false)
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_observer_creation() {
        let observer = ServiceObserver::new();
        assert!(!observer.is_running().await);
        assert_eq!(observer.service_count().await, 0);
    }

    #[tokio::test]
    async fn test_observer_start_stop() {
        let observer = ServiceObserver::new();
        observer.start().await.unwrap();
        assert!(observer.is_running().await);

        observer.stop().await.unwrap();
        assert!(!observer.is_running().await);
    }

    #[tokio::test]
    async fn test_add_service() {
        let observer = ServiceObserver::new();
        let svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "nginx".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: "10.0.0.1".parse().ok(),
            selector: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            ports: vec![ServicePort {
                name: Some("http".to_string()),
                protocol: "TCP".to_string(),
                port: 80,
                target_port: 8080,
                node_port: None,
            }],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer
            .add_service_internal(svc.clone())
            .await
            .unwrap();

        assert_eq!(observer.service_count().await, 1);

        let retrieved = observer.get_service("default", "nginx").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "nginx");
    }

    #[tokio::test]
    async fn test_update_service() {
        let observer = ServiceObserver::new();
        let mut svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "nginx".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: "10.0.0.1".parse().ok(),
            selector: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            ports: vec![ServicePort {
                name: Some("http".to_string()),
                protocol: "TCP".to_string(),
                port: 80,
                target_port: 8080,
                node_port: None,
            }],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer
            .add_service_internal(svc.clone())
            .await
            .unwrap();

        // Update port
        svc.ports[0].port = 8080;
        observer
            .update_service_internal(svc.clone())
            .await
            .unwrap();

        let retrieved = observer.get_service("default", "nginx").await.unwrap();
        assert_eq!(retrieved.ports[0].port, 8080);
    }

    #[tokio::test]
    async fn test_delete_service() {
        let observer = ServiceObserver::new();
        let svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "nginx".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: "10.0.0.1".parse().ok(),
            selector: Default::default(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer
            .add_service_internal(svc.clone())
            .await
            .unwrap();
        assert_eq!(observer.service_count().await, 1);

        observer
            .delete_service_internal(&svc.key())
            .await
            .unwrap();
        assert_eq!(observer.service_count().await, 0);
    }

    #[tokio::test]
    async fn test_list_services() {
        let observer = ServiceObserver::new();

        for i in 0..3 {
            let svc = ServiceInfo {
                namespace: "default".to_string(),
                name: format!("svc-{}", i),
                service_type: ServiceType::ClusterIP,
                cluster_ip: format!("10.0.0.{}", i + 1).parse().ok(),
                selector: Default::default(),
                ports: vec![],
                session_affinity: SessionAffinity::None,
                load_balancer_ip: None,
                external_ips: vec![],
            };
            observer.add_service_internal(svc).await.unwrap();
        }

        let services = observer.list_services().await;
        assert_eq!(services.len(), 3);
    }

    #[tokio::test]
    async fn test_find_by_selector() {
        let observer = ServiceObserver::new();

        let svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "nginx".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: "10.0.0.1".parse().ok(),
            selector: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer.add_service_internal(svc).await.unwrap();

        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let found = observer.find_services(&selector).await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "nginx");
    }

    #[tokio::test]
    async fn test_selector_no_match() {
        let observer = ServiceObserver::new();

        let svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "nginx".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: "10.0.0.1".parse().ok(),
            selector: [("app".to_string(), "nginx".to_string())]
                .iter()
                .cloned()
                .collect(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer.add_service_internal(svc).await.unwrap();

        let selector = [("app".to_string(), "postgres".to_string())]
            .iter()
            .cloned()
            .collect();
        let found = observer.find_services(&selector).await;
        assert_eq!(found.len(), 0);
    }

    #[tokio::test]
    async fn test_get_by_ip() {
        let observer = ServiceObserver::new();
        let ip = "10.0.0.1".parse().unwrap();

        let svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "nginx".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: Some(ip),
            selector: Default::default(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer.add_service_internal(svc).await.unwrap();

        let found = observer
            .cache
            .get_service_by_ip(&ip)
            .await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "nginx");
    }

    #[tokio::test]
    async fn test_event_handler() {
        struct TestHandler {
            added_count: Arc<RwLock<usize>>,
        }

        #[async_trait::async_trait]
        impl EventHandler for TestHandler {
            async fn on_service_added(&self, _svc: &ServiceInfo) -> Result<()> {
                let mut count = self.added_count.write().await;
                *count += 1;
                Ok(())
            }

            async fn on_service_updated(&self, _svc: &ServiceInfo) -> Result<()> {
                Ok(())
            }

            async fn on_service_deleted(&self, _key: &ServiceKey) -> Result<()> {
                Ok(())
            }
        }

        let observer = ServiceObserver::new();
        let handler = Arc::new(TestHandler {
            added_count: Arc::new(RwLock::new(0)),
        });

        observer.register_handler(handler.clone()).await;

        let svc = ServiceInfo {
            namespace: "default".to_string(),
            name: "test".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: None,
            selector: Default::default(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer.add_service_internal(svc).await.unwrap();

        let count = *handler.added_count.read().await;
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_services_by_namespace() {
        let observer = ServiceObserver::new();

        let svc1 = ServiceInfo {
            namespace: "default".to_string(),
            name: "svc1".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: None,
            selector: Default::default(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        let svc2 = ServiceInfo {
            namespace: "kube-system".to_string(),
            name: "svc2".to_string(),
            service_type: ServiceType::ClusterIP,
            cluster_ip: None,
            selector: Default::default(),
            ports: vec![],
            session_affinity: SessionAffinity::None,
            load_balancer_ip: None,
            external_ips: vec![],
        };

        observer.add_service_internal(svc1).await.unwrap();
        observer.add_service_internal(svc2).await.unwrap();

        let default_svcs = observer.cache.services_for_namespace("default").await;
        assert_eq!(default_svcs.len(), 1);
        assert_eq!(default_svcs[0].name, "svc1");
    }

    #[test]
    fn test_selector_matches() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let labels = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();

        assert!(selector_matches(&selector, &labels));
    }

    #[test]
    fn test_selector_partial_match() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let labels = [
            ("app".to_string(), "nginx".to_string()),
            ("version".to_string(), "v1".to_string()),
        ]
        .iter()
        .cloned()
        .collect();

        assert!(selector_matches(&selector, &labels));
    }

    #[test]
    fn test_selector_no_match_value() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let labels = [("app".to_string(), "postgres".to_string())]
            .iter()
            .cloned()
            .collect();

        assert!(!selector_matches(&selector, &labels));
    }

    #[test]
    fn test_selector_no_match_key() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .iter()
            .cloned()
            .collect();
        let labels = [("version".to_string(), "v1".to_string())]
            .iter()
            .cloned()
            .collect();

        assert!(!selector_matches(&selector, &labels));
    }
}
