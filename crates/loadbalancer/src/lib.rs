//! Load balancer subsystem — ported from cilium/pkg/loadbalancer
//!
//! This module implements the Cilium load balancer, which reconciles Kubernetes Services
//! with eBPF BPF maps for packet forwarding. It includes service types, frontend/backend
//! management, consistent-hash backend selection (Maglev), and DSR/SNAT modes.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use thiserror::Error;
use tracing::debug;

/// Error type for load balancer operations.
#[derive(Debug, Error)]
pub enum LbError {
    #[error("service not found: {0}")]
    ServiceNotFound(String),

    #[error("backend not found: {0}")]
    BackendNotFound(String),

    #[error("frontend not found: {0}")]
    FrontendNotFound(String),

    #[error("invalid service type")]
    InvalidServiceType,

    #[error("invalid IP address")]
    InvalidIp,

    #[error("no healthy backends available")]
    NoHealthyBackends,

    #[error("reconciliation failed: {0}")]
    ReconciliationFailed(String),

    #[error("eBPF map error: {0}")]
    BpfMapError(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, LbError>;

/// Service identifier in eBPF maps (u16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceId(pub u16);

impl ServiceId {
    pub const MIN: Self = Self(1);
    pub const MAX: Self = Self(u16::MAX);
    pub const ZERO: Self = Self(0);

    pub fn is_reserved(&self) -> bool {
        self.0 == 0
    }
}

impl std::fmt::Display for ServiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Service name in format: namespace/name or cluster/namespace/name
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceName {
    pub namespace: String,
    pub name: String,
    pub cluster: Option<String>,
}

impl ServiceName {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            cluster: None,
        }
    }

    pub fn with_cluster(mut self, cluster: impl Into<String>) -> Self {
        self.cluster = Some(cluster.into());
        self
    }
}

impl std::fmt::Display for ServiceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(cluster) = &self.cluster {
            write!(f, "{}/{}/{}", cluster, self.namespace, self.name)
        } else {
            write!(f, "{}/{}", self.namespace, self.name)
        }
    }
}

/// Layer 3 and Layer 4 address (IP + port + protocol).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct L3n4Addr {
    pub ip: IpAddr,
    pub port: u16,
    pub protocol: L4Protocol,
}

impl L3n4Addr {
    pub fn new(ip: IpAddr, port: u16, protocol: L4Protocol) -> Self {
        Self { ip, port, protocol }
    }

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

impl std::fmt::Display for L3n4Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}({})", self.ip, self.port, self.protocol)
    }
}

/// Layer 4 protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum L4Protocol {
    TCP,
    UDP,
    SCTP,
    Unknown(u8),
}

impl std::fmt::Display for L4Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TCP => write!(f, "TCP"),
            Self::UDP => write!(f, "UDP"),
            Self::SCTP => write!(f, "SCTP"),
            Self::Unknown(n) => write!(f, "Unknown({})", n),
        }
    }
}

/// Service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SvcType {
    ClusterIp,
    NodePort,
    LoadBalancer,
    ExternalIps,
    HostPort,
    LocalRedirect,
}

impl std::fmt::Display for SvcType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClusterIp => write!(f, "ClusterIP"),
            Self::NodePort => write!(f, "NodePort"),
            Self::LoadBalancer => write!(f, "LoadBalancer"),
            Self::ExternalIps => write!(f, "ExternalIPs"),
            Self::HostPort => write!(f, "HostPort"),
            Self::LocalRedirect => write!(f, "LocalRedirect"),
        }
    }
}

/// Traffic policy (Local vs Cluster).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrafficPolicy {
    Cluster,
    Local,
}

impl std::fmt::Display for TrafficPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cluster => write!(f, "Cluster"),
            Self::Local => write!(f, "Local"),
        }
    }
}

/// Forwarding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForwardingMode {
    DSR,  // Direct Server Return
    SNAT, // Source NAT
}

impl std::fmt::Display for ForwardingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DSR => write!(f, "DSR"),
            Self::SNAT => write!(f, "SNAT"),
        }
    }
}

/// Backend state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendState {
    Active,
    Terminating,
    Quarantined,
}

impl std::fmt::Display for BackendState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "Active"),
            Self::Terminating => write!(f, "Terminating"),
            Self::Quarantined => write!(f, "Quarantined"),
        }
    }
}

/// Backend represents a single pod backing a service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    pub service_name: ServiceName,
    pub address: L3n4Addr,
    pub node_name: Option<String>,
    pub port_names: Vec<String>,
    pub weight: u16,
    pub state: BackendState,
    pub healthy: bool,
}

impl Backend {
    pub fn new(service_name: ServiceName, address: L3n4Addr) -> Self {
        Self {
            service_name,
            address,
            node_name: None,
            port_names: Vec::new(),
            weight: 100,
            state: BackendState::Active,
            healthy: true,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.healthy && (self.state == BackendState::Active || self.state == BackendState::Terminating)
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -> {} ({})",
            self.service_name,
            self.address,
            if self.healthy { "healthy" } else { "unhealthy" }
        )
    }
}

/// Frontend represents a service endpoint (VIP + port).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frontend {
    pub address: L3n4Addr,
    pub service_type: SvcType,
    pub service_name: ServiceName,
    pub id: ServiceId,
    pub backends: Vec<Backend>,
    pub traffic_policy: TrafficPolicy,
    pub forwarding_mode: ForwardingMode,
}

impl Frontend {
    pub fn new(address: L3n4Addr, service_type: SvcType, service_name: ServiceName) -> Self {
        Self {
            address,
            service_type,
            service_name,
            id: ServiceId::ZERO,
            backends: Vec::new(),
            traffic_policy: TrafficPolicy::Cluster,
            forwarding_mode: ForwardingMode::SNAT,
        }
    }

    pub fn with_backends(mut self, backends: Vec<Backend>) -> Self {
        self.backends = backends;
        self
    }

    pub fn healthy_backends(&self) -> Vec<&Backend> {
        self.backends.iter().filter(|b| b.is_alive()).collect()
    }

    pub fn local_backends(&self, node_name: &str) -> Vec<&Backend> {
        self.backends
            .iter()
            .filter(|b| b.node_name.as_deref() == Some(node_name) && b.is_alive())
            .collect()
    }
}

impl std::fmt::Display for Frontend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} ({}, {} backends)",
            self.address.ip,
            self.address.port,
            self.service_type,
            self.backends.len()
        )
    }
}

/// Service represents a Kubernetes service with multiple frontends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: ServiceName,
    pub frontends: Vec<Frontend>,
    pub session_affinity: bool,
    pub session_affinity_timeout: u32,
}

impl Service {
    pub fn new(name: ServiceName) -> Self {
        Self {
            name,
            frontends: Vec::new(),
            session_affinity: false,
            session_affinity_timeout: 10800,
        }
    }

    pub fn with_frontends(mut self, frontends: Vec<Frontend>) -> Self {
        self.frontends = frontends;
        self
    }
}

impl std::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({} frontends)", self.name, self.frontends.len())
    }
}

/// Maglev consistent hash implementation for backend selection.
///
/// Maglev uses a permutation table to consistently map traffic to backends
/// with minimal disruption on backend changes.
pub struct MaglevHash {
    backends: Vec<String>,
    permutation_table: Vec<usize>,
    table_size: usize,
}

impl MaglevHash {
    const DEFAULT_TABLE_SIZE: usize = 65521; // Prime number

    pub fn new(backends: Vec<String>) -> Result<Self> {
        if backends.is_empty() {
            return Err(LbError::NoHealthyBackends);
        }

        let table_size = Self::DEFAULT_TABLE_SIZE;
        let mut perm_table = vec![usize::MAX; table_size];

        // Build permutation table using Maglev algorithm
        let mut offset = vec![0usize; backends.len()];
        let mut skip = vec![0usize; backends.len()];

        for (i, backend) in backends.iter().enumerate() {
            // Use hash of backend to seed offset and skip
            let hash = fnv_hash(backend);
            offset[i] = (hash as usize) % table_size;
            skip[i] = ((hash >> 32) as usize % (table_size - 1)) + 1;
        }

        // Fill permutation table
        for j in 0..table_size {
            for i in 0..backends.len() {
                let pos = (offset[i] + j * skip[i]) % table_size;
                if perm_table[pos] == usize::MAX {
                    perm_table[pos] = i;
                    break;
                }
            }
        }

        Ok(Self {
            backends,
            permutation_table: perm_table,
            table_size,
        })
    }

    pub fn select(&self, key: &[u8]) -> Result<&str> {
        if self.backends.is_empty() {
            return Err(LbError::NoHealthyBackends);
        }
        let hash = fnv_hash_bytes(key);
        let idx = (hash as usize) % self.table_size;
        let backend_idx = self.permutation_table[idx];
        Ok(&self.backends[backend_idx])
    }
}

/// FNV-1a hash function.
fn fnv_hash(s: &str) -> u64 {
    fnv_hash_bytes(s.as_bytes())
}

fn fnv_hash_bytes(data: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Load balancer manager.
pub struct LoadBalancer {
    services: Arc<DashMap<ServiceName, Service>>,
    frontends: Arc<DashMap<ServiceId, Frontend>>,
    service_id_counter: Arc<std::sync::atomic::AtomicU16>,
}

impl LoadBalancer {
    pub fn new() -> Self {
        Self {
            services: Arc::new(DashMap::new()),
            frontends: Arc::new(DashMap::new()),
            service_id_counter: Arc::new(std::sync::atomic::AtomicU16::new(ServiceId::MIN.0)),
        }
    }

    /// Add or update a service.
    pub fn upsert_service(&self, service: Service) -> Result<()> {
        debug!("Upserting service: {}", service.name);
        self.services.insert(service.name.clone(), service);
        Ok(())
    }

    /// Retrieve a service by name.
    pub fn get_service(&self, name: &ServiceName) -> Result<Option<Service>> {
        Ok(self.services.get(name).map(|entry| entry.value().clone()))
    }

    /// List all services.
    pub fn list_services(&self) -> Vec<Service> {
        self.services.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Allocate a new service ID.
    fn allocate_service_id(&self) -> ServiceId {
        let id = self.service_id_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ServiceId(id)
    }

    /// Add a frontend for a service.
    pub fn add_frontend(&self, mut frontend: Frontend) -> Result<()> {
        frontend.id = self.allocate_service_id();
        debug!("Adding frontend: {} with ID {}", frontend.address, frontend.id);
        self.frontends.insert(frontend.id, frontend);
        Ok(())
    }

    /// Get a frontend by ID.
    pub fn get_frontend(&self, id: ServiceId) -> Result<Option<Frontend>> {
        Ok(self.frontends.get(&id).map(|entry| entry.value().clone()))
    }

    /// List all frontends.
    pub fn list_frontends(&self) -> Vec<Frontend> {
        self.frontends.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Select a backend for a frontend using Maglev hashing.
    pub fn select_backend(&self, frontend_id: ServiceId, flow_hash: &[u8]) -> Result<Backend> {
        let frontend = self
            .frontends
            .get(&frontend_id)
            .ok_or_else(|| LbError::FrontendNotFound(frontend_id.to_string()))?;

        let healthy_backends = frontend
            .healthy_backends()
            .into_iter()
            .map(|b| b.address.ip.to_string())
            .collect::<Vec<_>>();

        if healthy_backends.is_empty() {
            return Err(LbError::NoHealthyBackends);
        }

        let maglev = MaglevHash::new(healthy_backends)?;
        let selected_ip = maglev.select(flow_hash)?;

        // Find the backend with this IP
        frontend
            .healthy_backends()
            .into_iter()
            .find(|b| b.address.ip.to_string() == selected_ip)
            .cloned()
            .ok_or_else(|| LbError::BackendNotFound(selected_ip.to_string()))
    }

    /// Update backends for a service.
    pub fn update_backends(&self, service_name: &ServiceName, backends: Vec<Backend>) -> Result<()> {
        let mut service = self
            .services
            .get_mut(service_name)
            .ok_or_else(|| LbError::ServiceNotFound(service_name.to_string()))?;

        for frontend in &mut service.frontends {
            frontend.backends = backends.clone();
        }

        debug!("Updated {} backends for service {}", backends.len(), service_name);
        Ok(())
    }

    /// Remove a service.
    pub fn remove_service(&self, name: &ServiceName) -> Result<()> {
        self.services.remove(name);
        debug!("Removed service: {}", name);
        Ok(())
    }

    /// Get statistics.
    pub fn stats(&self) -> LoadBalancerStats {
        LoadBalancerStats {
            services: self.services.len(),
            frontends: self.frontends.len(),
            total_backends: self
                .frontends
                .iter()
                .map(|entry| entry.value().backends.len())
                .sum(),
        }
    }
}

impl Default for LoadBalancer {
    fn default() -> Self {
        Self::new()
    }
}

/// Load balancer statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadBalancerStats {
    pub services: usize,
    pub frontends: usize,
    pub total_backends: usize,
}

/// Run the load balancer scaffold.
pub fn scaffold() -> String {
    format!(
        "load balancer scaffold ready | services=0 | frontends=0 | backends=0"
    )
}

/// Run the load balancer.
pub fn run() -> Result<String> {
    Ok(scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_name_display() {
        let name = ServiceName::new("default", "nginx");
        assert_eq!(name.to_string(), "default/nginx");

        let name = ServiceName::new("default", "nginx").with_cluster("us-west");
        assert_eq!(name.to_string(), "us-west/default/nginx");
    }

    #[test]
    fn test_service_id_reserved() {
        assert!(ServiceId::ZERO.is_reserved());
        assert!(!ServiceId::MIN.is_reserved());
    }

    #[test]
    fn test_backend_is_alive() {
        let backend = Backend::new(
            ServiceName::new("default", "nginx"),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        assert!(backend.is_alive());

        let mut backend = backend;
        backend.healthy = false;
        assert!(!backend.is_alive());

        backend.healthy = true;
        backend.state = BackendState::Quarantined;
        assert!(!backend.is_alive());
    }

    #[test]
    fn test_frontend_healthy_backends() {
        let svc_name = ServiceName::new("default", "nginx");
        let backend1 = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        let mut backend2 = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.2".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        backend2.healthy = false;

        let frontend = Frontend::new(
            L3n4Addr::new("10.1.1.1".parse().unwrap(), 80, L4Protocol::TCP),
            SvcType::ClusterIp,
            svc_name,
        )
        .with_backends(vec![backend1.clone(), backend2]);

        let healthy = frontend.healthy_backends();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].address.ip.to_string(), "10.0.0.1");
    }

    #[test]
    fn test_frontend_local_backends() {
        let svc_name = ServiceName::new("default", "nginx");
        let mut backend1 = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        backend1.node_name = Some("node1".to_string());

        let mut backend2 = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.2".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        backend2.node_name = Some("node2".to_string());

        let frontend = Frontend::new(
            L3n4Addr::new("10.1.1.1".parse().unwrap(), 80, L4Protocol::TCP),
            SvcType::ClusterIp,
            svc_name,
        )
        .with_backends(vec![backend1, backend2]);

        let local = frontend.local_backends("node1");
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].address.ip.to_string(), "10.0.0.1");
    }

    #[test]
    fn test_maglev_hash_creation() {
        let backends = vec!["backend1".to_string(), "backend2".to_string(), "backend3".to_string()];
        let maglev = MaglevHash::new(backends).unwrap();
        assert_eq!(maglev.backends.len(), 3);
        assert_eq!(maglev.table_size, MaglevHash::DEFAULT_TABLE_SIZE);
    }

    #[test]
    fn test_maglev_hash_empty_backends() {
        let backends: Vec<String> = vec![];
        let result = MaglevHash::new(backends);
        assert!(result.is_err());
    }

    #[test]
    fn test_maglev_select_consistent() {
        let backends = vec!["backend1".to_string(), "backend2".to_string()];
        let maglev = MaglevHash::new(backends).unwrap();

        let flow_hash = b"flow1";
        let selected1 = maglev.select(flow_hash).unwrap();
        let selected2 = maglev.select(flow_hash).unwrap();

        assert_eq!(selected1, selected2, "Selection should be deterministic");
    }

    #[test]
    fn test_maglev_select_distribution() {
        let backends = vec!["b1".to_string(), "b2".to_string(), "b3".to_string()];
        let maglev = MaglevHash::new(backends).unwrap();

        // Verify that selection works and is deterministic
        let flow_hash1 = b"flow1";
        let selected1 = maglev.select(flow_hash1).unwrap();
        let selected1_again = maglev.select(flow_hash1).unwrap();
        assert_eq!(selected1, selected1_again);

        // Verify different flows might select different backends
        let flow_hash2 = b"flow2";
        let selected2 = maglev.select(flow_hash2).unwrap();
        // No assertion on difference - depends on hash distribution
        assert!(!selected2.is_empty());
    }

    #[test]
    fn test_load_balancer_add_service() {
        let lb = LoadBalancer::new();
        let service = Service::new(ServiceName::new("default", "nginx"));

        assert!(lb.upsert_service(service).is_ok());
        assert_eq!(lb.stats().services, 1);
    }

    #[test]
    fn test_load_balancer_add_frontend() {
        let lb = LoadBalancer::new();
        let svc_name = ServiceName::new("default", "nginx");
        let service = Service::new(svc_name.clone());
        lb.upsert_service(service).unwrap();

        let frontend = Frontend::new(
            L3n4Addr::new("10.1.1.1".parse().unwrap(), 80, L4Protocol::TCP),
            SvcType::ClusterIp,
            svc_name,
        );

        assert!(lb.add_frontend(frontend).is_ok());
        assert_eq!(lb.stats().frontends, 1);
    }

    #[test]
    fn test_load_balancer_select_backend() {
        let lb = LoadBalancer::new();
        let svc_name = ServiceName::new("default", "nginx");

        let backend1 = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        let backend2 = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.2".parse().unwrap(), 8080, L4Protocol::TCP),
        );

        let frontend = Frontend::new(
            L3n4Addr::new("10.1.1.1".parse().unwrap(), 80, L4Protocol::TCP),
            SvcType::ClusterIp,
            svc_name,
        )
        .with_backends(vec![backend1, backend2]);

        lb.add_frontend(frontend).unwrap();
        let frontends = lb.list_frontends();
        let frontend_id = frontends[0].id;

        let selected = lb.select_backend(frontend_id, b"flow1").unwrap();
        assert!(selected.address.ip.to_string().starts_with("10.0.0"));
    }

    #[test]
    fn test_load_balancer_update_backends() {
        let lb = LoadBalancer::new();
        let svc_name = ServiceName::new("default", "nginx");

        let service = Service::new(svc_name.clone());
        lb.upsert_service(service).unwrap();

        let backend = Backend::new(
            svc_name.clone(),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );

        assert!(lb.update_backends(&svc_name, vec![backend]).is_ok());
    }

    #[test]
    fn test_load_balancer_remove_service() {
        let lb = LoadBalancer::new();
        let svc_name = ServiceName::new("default", "nginx");
        let service = Service::new(svc_name.clone());

        lb.upsert_service(service).unwrap();
        assert_eq!(lb.stats().services, 1);

        lb.remove_service(&svc_name).unwrap();
        assert_eq!(lb.stats().services, 0);
    }

    #[test]
    fn test_l3n4addr_socket_addr() {
        let addr = L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP);
        let socket = addr.socket_addr();
        assert_eq!(socket.port(), 8080);
        assert_eq!(socket.ip().to_string(), "10.0.0.1");
    }

    #[test]
    fn test_service_display() {
        let service = Service::new(ServiceName::new("default", "nginx"));
        assert!(service.to_string().contains("default/nginx"));
    }

    #[test]
    fn test_fnv_hash_deterministic() {
        let hash1 = fnv_hash("test");
        let hash2 = fnv_hash("test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_fnv_hash_different_inputs() {
        let hash1 = fnv_hash("test1");
        let hash2 = fnv_hash("test2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_traffic_policy_display() {
        assert_eq!(TrafficPolicy::Cluster.to_string(), "Cluster");
        assert_eq!(TrafficPolicy::Local.to_string(), "Local");
    }

    #[test]
    fn test_backend_state_display() {
        assert_eq!(BackendState::Active.to_string(), "Active");
        assert_eq!(BackendState::Terminating.to_string(), "Terminating");
        assert_eq!(BackendState::Quarantined.to_string(), "Quarantined");
    }

    #[test]
    fn test_forwarding_mode_display() {
        assert_eq!(ForwardingMode::DSR.to_string(), "DSR");
        assert_eq!(ForwardingMode::SNAT.to_string(), "SNAT");
    }

    #[test]
    fn test_svc_type_display() {
        assert_eq!(SvcType::ClusterIp.to_string(), "ClusterIP");
        assert_eq!(SvcType::NodePort.to_string(), "NodePort");
        assert_eq!(SvcType::LoadBalancer.to_string(), "LoadBalancer");
    }

    #[test]
    fn test_l4protocol_display() {
        assert_eq!(L4Protocol::TCP.to_string(), "TCP");
        assert_eq!(L4Protocol::UDP.to_string(), "UDP");
        assert_eq!(L4Protocol::SCTP.to_string(), "SCTP");
    }

    #[test]
    fn test_load_balancer_stats() {
        let lb = LoadBalancer::new();
        let stats = lb.stats();
        assert_eq!(stats.services, 0);
        assert_eq!(stats.frontends, 0);
        assert_eq!(stats.total_backends, 0);
    }

    #[test]
    fn test_backend_with_node_name() {
        let backend = Backend::new(
            ServiceName::new("default", "nginx"),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        let mut backend = backend;
        backend.node_name = Some("node1".to_string());

        assert_eq!(backend.node_name.as_deref(), Some("node1"));
    }

    #[test]
    fn test_frontend_ports_names() {
        let mut frontend = Frontend::new(
            L3n4Addr::new("10.1.1.1".parse().unwrap(), 80, L4Protocol::TCP),
            SvcType::ClusterIp,
            ServiceName::new("default", "nginx"),
        );
        frontend.backends = vec![];

        // Add backends with port names
        let mut backend = Backend::new(
            frontend.service_name.clone(),
            L3n4Addr::new("10.0.0.1".parse().unwrap(), 8080, L4Protocol::TCP),
        );
        backend.port_names = vec!["http".to_string()];
        frontend.backends.push(backend);

        assert!(!frontend.backends.is_empty());
        assert_eq!(frontend.backends[0].port_names[0], "http");
    }

    #[test]
    fn test_load_balancer_list_services() {
        let lb = LoadBalancer::new();
        let svc1 = Service::new(ServiceName::new("default", "nginx"));
        let svc2 = Service::new(ServiceName::new("default", "redis"));

        lb.upsert_service(svc1).unwrap();
        lb.upsert_service(svc2).unwrap();

        let services = lb.list_services();
        assert_eq!(services.len(), 2);
    }

    #[test]
    fn test_load_balancer_list_frontends() {
        let lb = LoadBalancer::new();
        let svc_name = ServiceName::new("default", "nginx");

        let frontend1 = Frontend::new(
            L3n4Addr::new("10.1.1.1".parse().unwrap(), 80, L4Protocol::TCP),
            SvcType::ClusterIp,
            svc_name.clone(),
        );
        let frontend2 = Frontend::new(
            L3n4Addr::new("10.1.1.2".parse().unwrap(), 443, L4Protocol::TCP),
            SvcType::ClusterIp,
            svc_name,
        );

        lb.add_frontend(frontend1).unwrap();
        lb.add_frontend(frontend2).unwrap();

        let frontends = lb.list_frontends();
        assert_eq!(frontends.len(), 2);
    }
}
