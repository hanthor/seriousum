//! Pure data model types for observing Kubernetes services and endpoints.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::debug;

/// Result type used by the service observer crate.
pub type ObserverResult<T> = std::result::Result<T, ServiceObserverError>;

/// Kubernetes service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceType {
    /// A cluster-internal virtual IP service.
    ClusterIP,
    /// A service exposed on a node port.
    NodePort,
    /// A service exposed via an external load balancer.
    LoadBalancer,
    /// A DNS-based alias to an external name.
    ExternalName,
    /// A service without a cluster IP.
    Headless,
}

/// Service protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Protocol {
    /// TCP protocol.
    TCP,
    /// UDP protocol.
    UDP,
    /// SCTP protocol.
    SCTP,
}

/// A service port mapping.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServicePort {
    /// Stable port name within the service.
    pub name: String,
    /// Transport protocol for the port.
    pub protocol: Protocol,
    /// Frontend service port.
    pub port: u16,
    /// Backend target port.
    pub target_port: u16,
    /// Optional node port for NodePort or LoadBalancer services.
    pub node_port: Option<u16>,
}

/// Unique key for a Kubernetes service.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceKey {
    /// Service namespace.
    pub namespace: String,
    /// Service name.
    pub name: String,
}

impl ServiceKey {
    /// Creates a new service key.
    pub fn new(ns: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: ns.into(),
            name: name.into(),
        }
    }
}

impl Display for ServiceKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.namespace, self.name)
    }
}

/// A Kubernetes service represented as pure data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    /// Unique service key.
    pub key: ServiceKey,
    /// Kubernetes service type.
    pub service_type: ServiceType,
    /// Primary cluster IP, if allocated.
    pub cluster_ip: Option<IpAddr>,
    /// Additional externally reachable IPs.
    pub external_ips: Vec<IpAddr>,
    /// Load balancer ingress IPs.
    pub load_balancer_ips: Vec<IpAddr>,
    /// Service ports.
    pub ports: Vec<ServicePort>,
    /// Service labels.
    pub labels: HashMap<String, String>,
    /// Pod selector labels.
    pub selector: HashMap<String, String>,
    /// Whether client IP session affinity is enabled.
    pub session_affinity: bool,
    /// Whether external traffic policy is set to Local.
    pub external_traffic_policy_local: bool,
}

impl Service {
    /// Creates a new service with default empty fields.
    pub fn new(key: ServiceKey, service_type: ServiceType) -> Self {
        Self {
            key,
            service_type,
            cluster_ip: None,
            external_ips: Vec::new(),
            load_balancer_ips: Vec::new(),
            ports: Vec::new(),
            labels: HashMap::new(),
            selector: HashMap::new(),
            session_affinity: false,
            external_traffic_policy_local: false,
        }
    }

    /// Returns true when the service is headless.
    pub fn is_headless(&self) -> bool {
        self.service_type == ServiceType::Headless
    }

    /// Returns the service port with the given name.
    pub fn port_for_name(&self, name: &str) -> Option<&ServicePort> {
        self.ports.iter().find(|port| port.name == name)
    }
}

/// A single backend endpoint address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointAddress {
    /// Backend IP address.
    pub ip: IpAddr,
    /// Optional node hosting the endpoint.
    pub node_name: Option<String>,
    /// Optional pod name.
    pub pod_name: Option<String>,
    /// Optional pod namespace.
    pub pod_namespace: Option<String>,
}

/// A group of endpoints for a specific port set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointSubset {
    /// Ready endpoint addresses.
    pub addresses: Vec<EndpointAddress>,
    /// Not-ready endpoint addresses.
    pub not_ready_addresses: Vec<EndpointAddress>,
    /// Ports shared by the subset.
    pub ports: Vec<ServicePort>,
}

/// A Kubernetes Endpoints object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoints {
    /// Service key associated with the endpoints.
    pub key: ServiceKey,
    /// Endpoint subsets.
    pub subsets: Vec<EndpointSubset>,
}

impl Endpoints {
    /// Creates a new empty endpoints object.
    pub fn new(key: ServiceKey) -> Self {
        Self {
            key,
            subsets: Vec::new(),
        }
    }

    /// Collects all ready backend socket addresses for a named port.
    pub fn ready_backends_for_port(&self, port_name: &str) -> Vec<SocketAddr> {
        let mut out = Vec::new();
        for subset in &self.subsets {
            if let Some(port) = subset.ports.iter().find(|port| port.name == port_name) {
                for address in &subset.addresses {
                    out.push(SocketAddr::new(address.ip, port.target_port));
                }
            }
        }
        out
    }

    /// Returns the total number of ready addresses across all subsets.
    pub fn ready_address_count(&self) -> usize {
        self.subsets
            .iter()
            .map(|subset| subset.addresses.len())
            .sum()
    }
}

/// An event describing a service or endpoints change.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    /// A service was added.
    ServiceAdded(Service),
    /// A service was updated.
    ServiceUpdated { old: Service, new: Service },
    /// A service was deleted.
    ServiceDeleted(ServiceKey),
    /// Endpoints were added.
    EndpointsAdded(Endpoints),
    /// Endpoints were updated.
    EndpointsUpdated { old: Endpoints, new: Endpoints },
    /// Endpoints were deleted.
    EndpointsDeleted(ServiceKey),
}

impl ServiceEvent {
    /// Returns the service key associated with the event.
    pub fn service_key(&self) -> &ServiceKey {
        match self {
            Self::ServiceAdded(service) | Self::ServiceUpdated { new: service, .. } => &service.key,
            Self::ServiceDeleted(key) => key,
            Self::EndpointsAdded(endpoints) | Self::EndpointsUpdated { new: endpoints, .. } => {
                &endpoints.key
            }
            Self::EndpointsDeleted(key) => key,
        }
    }
}

/// In-memory store for services and endpoints with event emission.
#[derive(Debug)]
pub struct ServiceStore {
    services: HashMap<ServiceKey, Service>,
    endpoints: HashMap<ServiceKey, Endpoints>,
    tx: broadcast::Sender<ServiceEvent>,
}

impl ServiceStore {
    /// Creates a new empty store.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            services: HashMap::new(),
            endpoints: HashMap::new(),
            tx,
        }
    }

    /// Subscribes to service events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.tx.subscribe()
    }

    /// Adds a service and emits an add event.
    pub fn add_service(&mut self, service: Service) {
        let key = service.key.clone();
        self.services.insert(key.clone(), service.clone());
        debug!(service = %key, "adding service");
        let _ = self.tx.send(ServiceEvent::ServiceAdded(service));
    }

    /// Updates an existing service and emits an update event.
    pub fn update_service(&mut self, new: Service) {
        if let Some(old) = self.services.insert(new.key.clone(), new.clone()) {
            debug!(service = %new.key, "updating service");
            let _ = self.tx.send(ServiceEvent::ServiceUpdated { old, new });
        }
    }

    /// Deletes a service and emits a delete event.
    pub fn delete_service(&mut self, key: &ServiceKey) {
        if self.services.remove(key).is_some() {
            debug!(service = %key, "deleting service");
            let _ = self.tx.send(ServiceEvent::ServiceDeleted(key.clone()));
        }
    }

    /// Adds endpoints and emits an add event.
    pub fn add_endpoints(&mut self, endpoints: Endpoints) {
        let key = endpoints.key.clone();
        self.endpoints.insert(key.clone(), endpoints.clone());
        debug!(service = %key, "adding endpoints");
        let _ = self.tx.send(ServiceEvent::EndpointsAdded(endpoints));
    }

    /// Updates existing endpoints and emits an update event.
    pub fn update_endpoints(&mut self, new: Endpoints) {
        if let Some(old) = self.endpoints.insert(new.key.clone(), new.clone()) {
            debug!(service = %new.key, "updating endpoints");
            let _ = self.tx.send(ServiceEvent::EndpointsUpdated { old, new });
        }
    }

    /// Deletes endpoints and emits a delete event.
    pub fn delete_endpoints(&mut self, key: &ServiceKey) {
        if self.endpoints.remove(key).is_some() {
            debug!(service = %key, "deleting endpoints");
            let _ = self.tx.send(ServiceEvent::EndpointsDeleted(key.clone()));
        }
    }

    /// Returns a service by key.
    pub fn get_service(&self, key: &ServiceKey) -> Option<&Service> {
        self.services.get(key)
    }

    /// Returns endpoints by key.
    pub fn get_endpoints(&self, key: &ServiceKey) -> Option<&Endpoints> {
        self.endpoints.get(key)
    }

    /// Returns all services currently in the store.
    pub fn list_services(&self) -> Vec<Service> {
        self.services.values().cloned().collect()
    }

    /// Returns all endpoints currently in the store.
    pub fn list_endpoints(&self) -> Vec<Endpoints> {
        self.endpoints.values().cloned().collect()
    }

    /// Finds services in a namespace.
    pub fn services_for_namespace(&self, namespace: &str) -> Vec<Service> {
        self.services
            .values()
            .filter(|service| service.key.namespace == namespace)
            .cloned()
            .collect()
    }

    /// Finds services whose selectors contain all labels in the provided selector.
    pub fn find_services_by_selector(&self, selector: &HashMap<String, String>) -> Vec<Service> {
        self.services
            .values()
            .filter(|service| selector_matches(&service.selector, selector))
            .cloned()
            .collect()
    }

    /// Returns the service that owns the given cluster IP.
    pub fn get_service_by_ip(&self, ip: &IpAddr) -> Option<&Service> {
        self.services
            .values()
            .find(|service| service.cluster_ip.as_ref() == Some(ip))
    }

    /// Returns the number of services currently stored.
    pub fn service_count(&self) -> usize {
        self.services.len()
    }

    /// Returns the number of endpoint objects currently stored.
    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
    }
}

impl Default for ServiceStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Event handler trait for compatibility with the earlier scaffold.
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Handles a service added event.
    async fn on_service_added(&self, _service: &Service) -> ObserverResult<()> {
        Ok(())
    }

    /// Handles a service updated event.
    async fn on_service_updated(&self, _service: &Service) -> ObserverResult<()> {
        Ok(())
    }

    /// Handles a service deleted event.
    async fn on_service_deleted(&self, _key: &ServiceKey) -> ObserverResult<()> {
        Ok(())
    }
}

/// Lightweight observer wrapper around the in-memory service store.
pub struct ServiceObserver {
    store: Arc<RwLock<ServiceStore>>,
    event_handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
    running: Arc<RwLock<bool>>,
}

impl ServiceObserver {
    /// Creates a new service observer.
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(ServiceStore::new())),
            event_handlers: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Marks the observer as running.
    pub async fn start(&self) -> ObserverResult<()> {
        let mut running = self.running.write().await;
        *running = true;
        debug!("starting service observer");
        Ok(())
    }

    /// Marks the observer as stopped.
    pub async fn stop(&self) -> ObserverResult<()> {
        let mut running = self.running.write().await;
        *running = false;
        debug!("stopping service observer");
        Ok(())
    }

    /// Returns whether the observer is running.
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Registers an event handler.
    pub async fn register_handler(&self, handler: Arc<dyn EventHandler>) {
        self.event_handlers.write().await.push(handler);
    }

    /// Removes all registered handlers.
    pub async fn clear_handlers(&self) {
        self.event_handlers.write().await.clear();
    }

    /// Returns the current event receiver.
    pub async fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.store.read().await.subscribe()
    }

    /// Returns a service by namespace and name.
    pub async fn get_service(&self, namespace: &str, name: &str) -> Option<Service> {
        let key = ServiceKey::new(namespace, name);
        self.store.read().await.get_service(&key).cloned()
    }

    /// Lists all services.
    pub async fn list_services(&self) -> Vec<Service> {
        self.store.read().await.list_services()
    }

    /// Finds services whose selectors match the provided selector.
    pub async fn find_services(&self, selector: &HashMap<String, String>) -> Vec<Service> {
        self.store.read().await.find_services_by_selector(selector)
    }

    /// Returns the number of services in the store.
    pub async fn service_count(&self) -> usize {
        self.store.read().await.service_count()
    }

    /// Adds a service and dispatches add events to registered handlers.
    pub async fn add_service_internal(&self, service: Service) -> ObserverResult<()> {
        self.store.write().await.add_service(service.clone());
        let handlers = self.event_handlers.read().await.clone();
        for handler in handlers {
            if let Err(error) = handler.on_service_added(&service).await {
                debug!(?error, service = %service.key, "service added handler failed");
            }
        }
        Ok(())
    }

    /// Updates a service and dispatches update events to registered handlers.
    pub async fn update_service_internal(&self, service: Service) -> ObserverResult<()> {
        self.store.write().await.update_service(service.clone());
        let handlers = self.event_handlers.read().await.clone();
        for handler in handlers {
            if let Err(error) = handler.on_service_updated(&service).await {
                debug!(?error, service = %service.key, "service updated handler failed");
            }
        }
        Ok(())
    }

    /// Deletes a service and dispatches delete events to registered handlers.
    pub async fn delete_service_internal(&self, key: &ServiceKey) -> ObserverResult<()> {
        self.store.write().await.delete_service(key);
        let handlers = self.event_handlers.read().await.clone();
        for handler in handlers {
            if let Err(error) = handler.on_service_deleted(key).await {
                debug!(?error, service = %key, "service deleted handler failed");
            }
        }
        Ok(())
    }

    /// Returns the service for the given cluster IP.
    pub async fn get_service_by_ip(&self, ip: &IpAddr) -> Option<Service> {
        self.store.read().await.get_service_by_ip(ip).cloned()
    }

    /// Returns all services within a namespace.
    pub async fn services_for_namespace(&self, namespace: &str) -> Vec<Service> {
        self.store.read().await.services_for_namespace(namespace)
    }
}

impl Default for ServiceObserver {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors produced by the service observer crate.
#[derive(Debug, thiserror::Error)]
pub enum ServiceObserverError {
    /// Requested service or endpoints object was not found.
    #[error("service not found: {0}")]
    NotFound(String),
    /// Broadcast watch channel was closed.
    #[error("watch channel closed")]
    ChannelClosed,
}

/// Returns true when all selector entries are present in the labels map.
fn selector_matches(
    service_selector: &HashMap<String, String>,
    labels: &HashMap<String, String>,
) -> bool {
    service_selector
        .iter()
        .all(|(key, value)| labels.get(key).is_some_and(|candidate| candidate == value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_service(name: &str) -> Service {
        let mut service = Service::new(ServiceKey::new("default", name), ServiceType::ClusterIP);
        service.cluster_ip = Some("10.0.0.1".parse().unwrap());
        service.selector = [("app".to_string(), name.to_string())]
            .into_iter()
            .collect();
        service.labels = [("app".to_string(), name.to_string())]
            .into_iter()
            .collect();
        service.ports.push(ServicePort {
            name: "http".into(),
            protocol: Protocol::TCP,
            port: 80,
            target_port: 8080,
            node_port: None,
        });
        service
    }

    fn sample_endpoints(name: &str) -> Endpoints {
        let mut endpoints = Endpoints::new(ServiceKey::new("default", name));
        endpoints.subsets.push(EndpointSubset {
            addresses: vec![EndpointAddress {
                ip: "10.0.0.1".parse().unwrap(),
                node_name: Some("node-a".into()),
                pod_name: Some(format!("{name}-pod")),
                pod_namespace: Some("default".into()),
            }],
            not_ready_addresses: vec![EndpointAddress {
                ip: "10.0.0.99".parse().unwrap(),
                node_name: None,
                pod_name: None,
                pod_namespace: None,
            }],
            ports: vec![ServicePort {
                name: "http".into(),
                protocol: Protocol::TCP,
                port: 80,
                target_port: 8080,
                node_port: None,
            }],
        });
        endpoints
    }

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
        let service = sample_service("nginx");
        observer
            .add_service_internal(service.clone())
            .await
            .unwrap();
        assert_eq!(observer.service_count().await, 1);
        let retrieved = observer.get_service("default", "nginx").await;
        assert_eq!(retrieved.unwrap().key.name, service.key.name);
    }

    #[tokio::test]
    async fn test_update_service() {
        let observer = ServiceObserver::new();
        let mut service = sample_service("nginx");
        observer
            .add_service_internal(service.clone())
            .await
            .unwrap();
        service.ports[0].port = 8080;
        observer
            .update_service_internal(service.clone())
            .await
            .unwrap();
        let retrieved = observer.get_service("default", "nginx").await.unwrap();
        assert_eq!(retrieved.ports[0].port, 8080);
    }

    #[tokio::test]
    async fn test_delete_service() {
        let observer = ServiceObserver::new();
        let service = sample_service("nginx");
        let key = service.key.clone();
        observer.add_service_internal(service).await.unwrap();
        assert_eq!(observer.service_count().await, 1);
        observer.delete_service_internal(&key).await.unwrap();
        assert_eq!(observer.service_count().await, 0);
    }

    #[tokio::test]
    async fn test_list_services() {
        let observer = ServiceObserver::new();
        for name in ["svc-0", "svc-1", "svc-2"] {
            observer
                .add_service_internal(sample_service(name))
                .await
                .unwrap();
        }
        assert_eq!(observer.list_services().await.len(), 3);
    }

    #[tokio::test]
    async fn test_find_by_selector() {
        let observer = ServiceObserver::new();
        observer
            .add_service_internal(sample_service("nginx"))
            .await
            .unwrap();
        let selector = [("app".to_string(), "nginx".to_string())]
            .into_iter()
            .collect();
        let found = observer.find_services(&selector).await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key.name, "nginx");
    }

    #[tokio::test]
    async fn test_selector_no_match() {
        let observer = ServiceObserver::new();
        observer
            .add_service_internal(sample_service("nginx"))
            .await
            .unwrap();
        let selector = [("app".to_string(), "postgres".to_string())]
            .into_iter()
            .collect();
        assert!(observer.find_services(&selector).await.is_empty());
    }

    #[tokio::test]
    async fn test_get_by_ip() {
        let observer = ServiceObserver::new();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        observer
            .add_service_internal(sample_service("nginx"))
            .await
            .unwrap();
        let found = observer.get_service_by_ip(&ip).await;
        assert_eq!(found.unwrap().key.name, "nginx");
    }

    #[tokio::test]
    async fn test_event_handler() {
        struct TestHandler {
            added_count: Arc<RwLock<usize>>,
        }

        #[async_trait]
        impl EventHandler for TestHandler {
            async fn on_service_added(&self, _service: &Service) -> ObserverResult<()> {
                let mut count = self.added_count.write().await;
                *count += 1;
                Ok(())
            }
        }

        let observer = ServiceObserver::new();
        let handler = Arc::new(TestHandler {
            added_count: Arc::new(RwLock::new(0)),
        });
        observer.register_handler(handler.clone()).await;
        observer
            .add_service_internal(sample_service("test"))
            .await
            .unwrap();
        assert_eq!(*handler.added_count.read().await, 1);
    }

    #[tokio::test]
    async fn test_services_by_namespace() {
        let observer = ServiceObserver::new();
        observer
            .add_service_internal(sample_service("svc1"))
            .await
            .unwrap();
        let mut other_namespace = sample_service("svc2");
        other_namespace.key.namespace = "kube-system".into();
        observer
            .add_service_internal(other_namespace)
            .await
            .unwrap();
        let default_services = observer.services_for_namespace("default").await;
        assert_eq!(default_services.len(), 1);
        assert_eq!(default_services[0].key.name, "svc1");
    }

    #[test]
    fn test_selector_matches() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .into_iter()
            .collect();
        let labels = [("app".to_string(), "nginx".to_string())]
            .into_iter()
            .collect();
        assert!(selector_matches(&selector, &labels));
    }

    #[test]
    fn test_selector_partial_match() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .into_iter()
            .collect();
        let labels = [
            ("app".to_string(), "nginx".to_string()),
            ("version".to_string(), "v1".to_string()),
        ]
        .into_iter()
        .collect();
        assert!(selector_matches(&selector, &labels));
    }

    #[test]
    fn test_selector_no_match_value() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .into_iter()
            .collect();
        let labels = [("app".to_string(), "postgres".to_string())]
            .into_iter()
            .collect();
        assert!(!selector_matches(&selector, &labels));
    }

    #[test]
    fn test_selector_no_match_key() {
        let selector = [("app".to_string(), "nginx".to_string())]
            .into_iter()
            .collect();
        let labels = [("version".to_string(), "v1".to_string())]
            .into_iter()
            .collect();
        assert!(!selector_matches(&selector, &labels));
    }

    #[test]
    fn test_service_port_lookup() {
        let mut service = Service::new(ServiceKey::new("default", "nginx"), ServiceType::ClusterIP);
        service.ports.push(ServicePort {
            name: "http".into(),
            protocol: Protocol::TCP,
            port: 80,
            target_port: 8080,
            node_port: None,
        });
        assert!(service.port_for_name("http").is_some());
        assert!(service.port_for_name("https").is_none());
    }

    #[test]
    fn test_endpoints_ready_backends() {
        let mut endpoints = Endpoints::new(ServiceKey::new("default", "nginx"));
        endpoints.subsets.push(EndpointSubset {
            addresses: vec![
                EndpointAddress {
                    ip: "10.0.0.1".parse().unwrap(),
                    node_name: None,
                    pod_name: None,
                    pod_namespace: None,
                },
                EndpointAddress {
                    ip: "10.0.0.2".parse().unwrap(),
                    node_name: None,
                    pod_name: None,
                    pod_namespace: None,
                },
            ],
            not_ready_addresses: vec![],
            ports: vec![ServicePort {
                name: "http".into(),
                protocol: Protocol::TCP,
                port: 80,
                target_port: 8080,
                node_port: None,
            }],
        });
        let backends = endpoints.ready_backends_for_port("http");
        assert_eq!(backends.len(), 2);
        assert_eq!(endpoints.ready_address_count(), 2);
    }

    #[test]
    fn test_service_store_add_delete() {
        let mut store = ServiceStore::new();
        let service = Service::new(ServiceKey::new("default", "nginx"), ServiceType::ClusterIP);
        let key = service.key.clone();
        store.add_service(service);
        assert_eq!(store.service_count(), 1);
        store.delete_service(&key);
        assert_eq!(store.service_count(), 0);
    }

    #[test]
    fn test_service_event_key() {
        let key = ServiceKey::new("default", "svc");
        let service = Service::new(key.clone(), ServiceType::LoadBalancer);
        let event = ServiceEvent::ServiceAdded(service);
        assert_eq!(event.service_key(), &key);
    }

    #[test]
    fn test_service_key_display() {
        let key = ServiceKey::new("kube-system", "kube-dns");
        assert_eq!(key.to_string(), "kube-system/kube-dns");
    }

    #[tokio::test]
    async fn test_store_subscribe_events() {
        let observer = ServiceObserver::new();
        let mut receiver = observer.subscribe().await;
        observer
            .add_service_internal(Service::new(
                ServiceKey::new("default", "web"),
                ServiceType::ClusterIP,
            ))
            .await
            .unwrap();
        let event = receiver.try_recv().unwrap();
        assert!(matches!(event, ServiceEvent::ServiceAdded(_)));
    }

    #[test]
    fn test_store_endpoints_lifecycle() {
        let mut store = ServiceStore::new();
        let endpoints = sample_endpoints("nginx");
        let key = endpoints.key.clone();
        store.add_endpoints(endpoints.clone());
        assert_eq!(store.endpoint_count(), 1);
        assert_eq!(store.get_endpoints(&key), Some(&endpoints));
        let mut updated = endpoints.clone();
        updated.subsets[0].addresses.push(EndpointAddress {
            ip: "10.0.0.2".parse().unwrap(),
            node_name: None,
            pod_name: None,
            pod_namespace: None,
        });
        store.update_endpoints(updated.clone());
        assert_eq!(store.get_endpoints(&key), Some(&updated));
        store.delete_endpoints(&key);
        assert!(store.get_endpoints(&key).is_none());
    }

    #[test]
    fn test_headless_service_helper() {
        let service = Service::new(
            ServiceKey::new("default", "headless"),
            ServiceType::Headless,
        );
        assert!(service.is_headless());
    }
}
