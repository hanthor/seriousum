//! Load Balancer - Selects backends for service requests
//!
//! Implements Issue #47 (P1.4): Load Balancing Algorithm
//!
//! This component:
//! - Takes service requests (client IP, service ID, protocol)
//! - Applies load balancing algorithms to select backends
//! - Tracks session affinity for client IP-based persistence
//! - Integrates with eBPF maps for backend/affinity storage

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Port, Result};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Default component name for load balancer scaffolds.
pub const COMPONENT: &str = "seriousum-loadbalancer";

/// Load balancing strategy for the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BalancingMode {
    /// Translate to a node-local backend.
    Nat,
    /// Preserve the original destination.
    Dsr,
}

/// Backend target for a virtual service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backend {
    /// Backend address.
    pub address: IpAddr,

    /// Backend port.
    pub port: Port,

    /// Backend selection weight.
    pub weight: u16,
}

impl Backend {
    /// Creates a backend target.
    #[must_use]
    pub fn new(address: IpAddr, port: Port) -> Self {
        Self {
            address,
            port,
            weight: 1,
        }
    }
}

/// Virtual service model for the load balancer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceModel {
    /// Service name.
    pub name: String,

    /// Frontend address.
    pub frontend: IpAddr,

    /// Frontend port.
    pub port: Port,

    /// Balancing strategy.
    pub mode: BalancingMode,

    /// Backends serving the service.
    pub backends: Vec<Backend>,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl ServiceModel {
    /// Creates a new service model.
    #[must_use]
    pub fn new(name: impl Into<String>, frontend: IpAddr, port: Port) -> Self {
        Self {
            name: name.into(),
            frontend,
            port,
            mode: BalancingMode::Nat,
            backends: vec![Backend::new(
                IpAddr::from([127, 0, 0, 1]),
                Port::cilium_operator(),
            )],
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "service scaffold",
            IpAddr::from([10, 0, 0, 10]),
            Port::cilium_agent(),
        )
    }

    /// Updates the balancing mode.
    #[must_use]
    pub fn with_mode(mut self, mode: BalancingMode) -> Self {
        self.mode = mode;
        self
    }

    /// Adds a backend to the service.
    #[must_use]
    pub fn with_backend(mut self, backend: Backend) -> Self {
        self.backends.push(backend);
        self
    }

    /// Returns a socket address-like summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} {}:{} backends={}",
            self.name,
            self.frontend,
            self.port,
            self.backends.len()
        )
    }

    /// Returns the first backend or an error if none exist.
    pub fn primary_backend(&self) -> Result<&Backend> {
        self.backends
            .first()
            .ok_or_else(|| Error::Loadbalancer(String::from("service has no backends")))
    }

    /// Validates the service model.
    pub fn validate(&self) -> Result<()> {
        self.primary_backend()?;

        if self.backends.iter().any(|backend| backend.weight == 0) {
            return Err(Error::Loadbalancer(String::from(
                "backend weight must be non-zero",
            )));
        }

        Ok(())
    }
}

impl Default for ServiceModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable service report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceReport {
    /// Component name.
    pub component: String,

    /// Load balancer model.
    pub service: ServiceModel,

    /// Whether the service is ready to route traffic.
    pub ready: bool,
}

impl ServiceReport {
    /// Builds a report from a service model.
    #[must_use]
    pub fn new(service: ServiceModel) -> Self {
        let ready = !service.backends.is_empty();
        Self {
            component: COMPONENT.to_owned(),
            service,
            ready,
        }
    }
}

/// Returns the standard load balancer scaffold report.
#[must_use]
pub fn scaffold() -> ServiceReport {
    ServiceReport::new(ServiceModel::scaffold())
}

// ============================================================================
// Load Balancing Algorithms
// ============================================================================

/// Load balancing algorithm types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LBAlgorithm {
    /// Round-robin: rotate through backends
    RoundRobin,
    /// Least-connections: select backend with fewest active connections
    LeastConnections,
    /// Consistent hash: client IP → backend (stable across changes)
    ConsistentHash,
    /// Random: random backend selection
    Random,
}

/// Load balancing decision
#[derive(Debug, Clone)]
pub struct LBDecision {
    pub backend: Backend,
    pub algorithm_used: LBAlgorithm,
    pub session_affinity_used: bool,
}

/// Round-robin state tracking
#[derive(Debug, Clone)]
struct RoundRobinState {
    current_index: usize,
}

impl RoundRobinState {
    fn new() -> Self {
        Self { current_index: 0 }
    }

    fn next(&mut self, backend_count: usize) -> usize {
        if backend_count == 0 {
            return 0;
        }
        let idx = self.current_index % backend_count;
        self.current_index = (self.current_index + 1) % backend_count;
        idx
    }
}

/// Main load balancer component
pub struct LoadBalancer {
    algorithm: LBAlgorithm,
    rr_state: Arc<RwLock<RoundRobinState>>,
    affinity_map: Arc<RwLock<HashMap<String, usize>>>, // client_key -> backend_idx
}

impl LoadBalancer {
    pub fn new(algorithm: LBAlgorithm) -> Self {
        Self {
            algorithm,
            rr_state: Arc::new(RwLock::new(RoundRobinState::new())),
            affinity_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a load balancer with round-robin
    pub fn round_robin() -> Self {
        Self::new(LBAlgorithm::RoundRobin)
    }

    /// Create a load balancer with least-connections
    pub fn least_connections() -> Self {
        Self::new(LBAlgorithm::LeastConnections)
    }

    /// Create a load balancer with consistent hashing
    pub fn consistent_hash() -> Self {
        Self::new(LBAlgorithm::ConsistentHash)
    }

    /// Select a backend for the given request
    pub async fn select_backend(
        &self,
        backends: &[Backend],
        client_ip: IpAddr,
        session_affinity: bool,
    ) -> Result<LBDecision> {
        if backends.is_empty() {
            return Err(Error::Loadbalancer("no backends available".to_string()));
        }

        let client_key = format!("{}", client_ip);

        // Check for existing affinity
        if session_affinity {
            let affinity = self.affinity_map.read().await;
            if let Some(&backend_idx) = affinity.get(&client_key) {
                if backend_idx < backends.len() {
                    debug!(
                        "Using affinity for {} -> backend {}",
                        client_ip, backend_idx
                    );
                    return Ok(LBDecision {
                        backend: backends[backend_idx].clone(),
                        algorithm_used: self.algorithm,
                        session_affinity_used: true,
                    });
                }
            }
        }

        // Select backend using algorithm
        let backend_idx = match self.algorithm {
            LBAlgorithm::RoundRobin => self.select_round_robin(backends).await,
            LBAlgorithm::LeastConnections => self.select_least_connections(backends),
            LBAlgorithm::ConsistentHash => self.select_consistent_hash(backends, client_ip),
            LBAlgorithm::Random => self.select_random(backends),
        };

        let backend = backends[backend_idx].clone();

        // Store affinity if enabled
        if session_affinity {
            let mut affinity = self.affinity_map.write().await;
            affinity.insert(client_key, backend_idx);
        }

        Ok(LBDecision {
            backend,
            algorithm_used: self.algorithm,
            session_affinity_used: session_affinity,
        })
    }

    async fn select_round_robin(&self, backends: &[Backend]) -> usize {
        let mut state = self.rr_state.write().await;
        state.next(backends.len())
    }

    fn select_least_connections(&self, backends: &[Backend]) -> usize {
        backends
            .iter()
            .enumerate()
            .min_by_key(|(_, _b)| 0u32) // For now, equal weight since we don't have conn tracking
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn select_consistent_hash(&self, backends: &[Backend], client_ip: IpAddr) -> usize {
        // Simple hash-based selection
        let hash = self.hash_client_ip(client_ip);
        hash % backends.len()
    }

    fn select_random(&self, backends: &[Backend]) -> usize {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hash, Hasher};

        let mut hasher = RandomState::new().build_hasher();
        std::process::id().hash(&mut hasher);
        (hasher.finish() % backends.len() as u64) as usize
    }

    fn hash_client_ip(&self, ip: IpAddr) -> usize {
        match ip {
            IpAddr::V4(addr) => {
                let octets = addr.octets();
                ((octets[0] as usize) << 24)
                    | ((octets[1] as usize) << 16)
                    | ((octets[2] as usize) << 8)
                    | (octets[3] as usize)
            }
            IpAddr::V6(addr) => {
                let segments = addr.segments();
                segments.iter().map(|&s| s as usize).sum()
            }
        }
    }

    /// Clear all stored affinities
    pub async fn clear_affinities(&self) {
        self.affinity_map.write().await.clear();
    }

    /// Get affinity count
    pub async fn affinity_count(&self) -> usize {
        self.affinity_map.read().await.len()
    }
}

impl Default for LoadBalancer {
    fn default() -> Self {
        Self::round_robin()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // Original scaffold tests
    #[test]
    fn scaffold_report_has_a_backend() {
        let report = scaffold();

        assert!(report.ready);
        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.service.version, VersionInfo::current());
        assert_eq!(
            report
                .service
                .primary_backend()
                .expect("backend exists")
                .port,
            Port::cilium_operator()
        );
    }

    #[test]
    fn validate_rejects_zero_weight_backend() {
        let service =
            ServiceModel::new("broken", IpAddr::from([10, 0, 0, 20]), Port::cilium_agent())
                .with_backend(Backend {
                    address: IpAddr::from([10, 0, 0, 21]),
                    port: Port::cilium_operator(),
                    weight: 0,
                });

        let error = service.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Loadbalancer(_)));
    }

    // Load balancing algorithm tests
    #[tokio::test]
    async fn test_round_robin_selection() {
        let lb = LoadBalancer::round_robin();
        let backends = vec![
            Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 2]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 3]), Port::cilium_agent()),
        ];

        let client_ip = IpAddr::from([192, 168, 1, 100]);

        // First call should select 0
        let decision1 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        assert_eq!(decision1.backend.address, backends[0].address);

        // Second call should select 1
        let decision2 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        assert_eq!(decision2.backend.address, backends[1].address);

        // Third call should select 2
        let decision3 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        assert_eq!(decision3.backend.address, backends[2].address);

        // Fourth call should wrap around to 0
        let decision4 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        assert_eq!(decision4.backend.address, backends[0].address);
    }

    #[tokio::test]
    async fn test_session_affinity() {
        let lb = LoadBalancer::round_robin();
        let backends = vec![
            Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 2]), Port::cilium_agent()),
        ];

        let client_ip = IpAddr::from([192, 168, 1, 100]);

        // First request with affinity
        let decision1 = lb
            .select_backend(&backends, client_ip, true)
            .await
            .unwrap();
        let selected_backend = decision1.backend.address;

        // Subsequent requests should use same backend
        for _ in 0..3 {
            let decision = lb
                .select_backend(&backends, client_ip, true)
                .await
                .unwrap();
            assert_eq!(
                decision.backend.address, selected_backend,
                "Affinity should return same backend"
            );
            assert!(decision.session_affinity_used);
        }
    }

    #[tokio::test]
    async fn test_consistent_hash() {
        let lb = LoadBalancer::consistent_hash();
        let backends = vec![
            Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 2]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 3]), Port::cilium_agent()),
        ];

        let client_ip = IpAddr::from([192, 168, 1, 100]);

        // Same client should always get same backend
        let decision1 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        let decision2 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        let decision3 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();

        assert_eq!(decision1.backend.address, decision2.backend.address);
        assert_eq!(decision2.backend.address, decision3.backend.address);
        assert_eq!(decision1.algorithm_used, LBAlgorithm::ConsistentHash);
    }

    #[tokio::test]
    async fn test_random_algorithm() {
        let lb = LoadBalancer::new(LBAlgorithm::Random);
        let backends = vec![
            Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 2]), Port::cilium_agent()),
        ];

        let client_ip = IpAddr::from([192, 168, 1, 100]);
        let decision = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();

        assert_eq!(decision.algorithm_used, LBAlgorithm::Random);
        assert!(backends.iter().any(|b| b.address == decision.backend.address));
    }

    #[tokio::test]
    async fn test_no_backends_error() {
        let lb = LoadBalancer::round_robin();
        let backends: Vec<Backend> = vec![];

        let result = lb
            .select_backend(&backends, IpAddr::from([192, 168, 1, 100]), false)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_affinity_map_operations() {
        let lb = LoadBalancer::round_robin();
        assert_eq!(lb.affinity_count().await, 0);

        let backends = vec![Backend::new(
            IpAddr::from([10, 0, 0, 1]),
            Port::cilium_agent(),
        )];

        let client1 = IpAddr::from([192, 168, 1, 100]);
        let client2 = IpAddr::from([192, 168, 1, 101]);

        // Add affinity for two clients
        let _ = lb.select_backend(&backends, client1, true).await.unwrap();
        let _ = lb.select_backend(&backends, client2, true).await.unwrap();

        assert_eq!(lb.affinity_count().await, 2);

        // Clear affinities
        lb.clear_affinities().await;
        assert_eq!(lb.affinity_count().await, 0);
    }

    #[tokio::test]
    async fn test_least_connections() {
        let lb = LoadBalancer::least_connections();
        let backends = vec![
            Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 2]), Port::cilium_agent()),
        ];

        let decision = lb
            .select_backend(&backends, IpAddr::from([192, 168, 1, 100]), false)
            .await
            .unwrap();

        assert_eq!(decision.algorithm_used, LBAlgorithm::LeastConnections);
    }

    #[test]
    fn test_round_robin_state() {
        let mut state = RoundRobinState::new();

        assert_eq!(state.next(3), 0);
        assert_eq!(state.next(3), 1);
        assert_eq!(state.next(3), 2);
        assert_eq!(state.next(3), 0); // Wraps around
        assert_eq!(state.next(3), 1);
    }

    #[test]
    fn test_backend_creation() {
        let backend = Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent());
        assert_eq!(backend.weight, 1);
        assert_eq!(backend.address, IpAddr::from([10, 0, 0, 1]));
    }

    #[test]
    fn test_service_model_backend_addition() {
        let mut service =
            ServiceModel::new("test", IpAddr::from([10, 0, 0, 10]), Port::cilium_agent());

        assert_eq!(service.backends.len(), 1); // Default backend

        service = service.with_backend(Backend::new(
            IpAddr::from([10, 0, 0, 20]),
            Port::cilium_agent(),
        ));

        assert_eq!(service.backends.len(), 2);
    }

    #[test]
    fn test_lb_algorithm_default() {
        let lb = LoadBalancer::default();
        // Should be round-robin by default
        assert_eq!(lb.algorithm, LBAlgorithm::RoundRobin);
    }

    #[tokio::test]
    async fn test_affinity_disabled() {
        let lb = LoadBalancer::round_robin();
        let backends = vec![
            Backend::new(IpAddr::from([10, 0, 0, 1]), Port::cilium_agent()),
            Backend::new(IpAddr::from([10, 0, 0, 2]), Port::cilium_agent()),
        ];

        let client_ip = IpAddr::from([192, 168, 1, 100]);

        // Disable affinity - should round-robin
        let decision1 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();
        let decision2 = lb
            .select_backend(&backends, client_ip, false)
            .await
            .unwrap();

        // Different backends without affinity
        assert_eq!(decision1.backend.address, backends[0].address);
        assert_eq!(decision2.backend.address, backends[1].address);
        assert!(!decision1.session_affinity_used);
        assert!(!decision2.session_affinity_used);
    }
}
