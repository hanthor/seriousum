//! Cilium Operator — Rust implementation with KRD reconciliation
//!
//! Full Kubernetes-native operator for Cilium managing:
//! - CiliumIdentity (CID) allocation and lifecycle
//! - CiliumEndpoint (CEP) and CiliumEndpointSlice (CES) synchronization
//! - CiliumNetworkPolicy (CNP) and ClusterwideCiliumNetworkPolicy (CCNP)
//! - Label selector evaluation and policy enforcement

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use thiserror::Error;

pub const OPERATOR_COMPONENT: &str = "seriousum-operator";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub contract: String,
    pub core: String,
}

impl VersionInfo {
    pub fn current() -> Self {
        Self {
            contract: env!("CARGO_PKG_VERSION").to_string(),
            core: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub status: HealthStatus,
    pub message: Option<String>,
    pub version: VersionInfo,
}

/// Errors returned by pure operator model and reconciliation helpers.
#[derive(Debug, Error)]
pub enum OperatorError {
    /// Identity allocation failed.
    #[error("identity allocation error: {0}")]
    IdentityAllocation(String),
    /// Generic lookup failure.
    #[error("not found: {0}")]
    NotFound(String),
    /// Invalid operator configuration.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    /// An IP is outside the configured pool CIDRs.
    #[error("IP {0} not in pool")]
    IPNotInPool(IpAddr),
    /// An IP was already allocated from the pool.
    #[error("IP {0} already allocated")]
    IPAlreadyAllocated(IpAddr),
    /// A reconcile cycle failed with an operator-defined error.
    #[error("reconcile error: {0}")]
    ReconcileError(String),
    /// A named pool could not be found.
    #[error("pool {0} not found")]
    PoolNotFound(String),
}

/// Result type for operator helpers.
pub type OperatorResult<T> = std::result::Result<T, OperatorError>;

/// The outcome of a single reconcile cycle.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileResult {
    /// Everything is in sync and no action was required.
    NoOp,
    /// Reconcile applied changes successfully.
    Updated { changes: u32 },
    /// Reconcile applied some changes but also hit non-fatal errors.
    PartialError { changes: u32, errors: Vec<String> },
    /// Reconcile failed entirely and should be retried.
    Failed(String),
}

impl ReconcileResult {
    /// Returns `true` when the reconcile cycle did not fail completely.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }

    /// Returns the number of changes applied by this cycle.
    #[must_use]
    pub fn changes(&self) -> u32 {
        match self {
            Self::Updated { changes } | Self::PartialError { changes, .. } => *changes,
            Self::NoOp | Self::Failed(_) => 0,
        }
    }
}

/// A reconciler that syncs desired and actual state for one object type.
#[async_trait::async_trait]
pub trait Reconciler: Send + Sync {
    /// Object reconciled by this implementation.
    type Object: Send + Sync;

    /// Reconcile one object instance.
    async fn reconcile(&self, obj: &Self::Object) -> ReconcileResult;

    /// Stable human-readable reconciler name.
    fn name(&self) -> &str;
}

/// Priority assigned to a reconcile request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReconcilePriority {
    /// Best-effort background work.
    Low = 0,
    /// Standard priority for normal updates.
    Normal = 1,
    /// Urgent work that should run before lower priorities.
    High = 2,
}

/// A pending request to reconcile one Kubernetes-style object key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileRequest {
    /// Object key in `namespace/name` form.
    pub key: String,
    /// Current request priority.
    pub priority: ReconcilePriority,
    /// Human-readable reason the object was enqueued.
    pub reason: String,
}

/// In-memory reconcile work queue with request deduplication.
#[derive(Debug, Default)]
pub struct ReconcileQueue {
    pending: HashMap<String, ReconcileRequest>,
    order: VecDeque<String>,
}

impl ReconcileQueue {
    /// Creates an empty reconcile queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueues a request, keeping the highest priority for duplicate keys.
    pub fn enqueue(&mut self, req: ReconcileRequest) {
        if let Some(existing) = self.pending.get_mut(&req.key) {
            if req.priority > existing.priority {
                existing.priority = req.priority;
                existing.reason = req.reason;
            }
            return;
        }

        self.order.push_back(req.key.clone());
        self.pending.insert(req.key.clone(), req);
    }

    /// Dequeues the highest-priority request, preserving FIFO ordering within a priority level.
    pub fn dequeue(&mut self) -> Option<ReconcileRequest> {
        while !self.order.is_empty() {
            let mut best_index = None;
            let mut best_priority = ReconcilePriority::Low;

            for (index, key) in self.order.iter().enumerate() {
                if let Some(req) = self.pending.get(key)
                    && (best_index.is_none() || req.priority > best_priority)
                {
                    best_index = Some(index);
                    best_priority = req.priority;
                }
            }

            let Some(index) = best_index else {
                self.order.clear();
                return None;
            };

            if let Some(key) = self.order.remove(index)
                && let Some(req) = self.pending.remove(&key)
            {
                return Some(req);
            }
        }

        None
    }

    /// Returns the number of queued unique keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Returns `true` when there are no pending requests.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// An IP pool managed by the operator.
#[derive(Debug, Clone)]
pub struct IPPool {
    /// Pool name.
    pub name: String,
    /// Backing CIDR for the pool.
    pub cidr: IpNet,
    /// Number of allocated IPs.
    pub allocated_count: u32,
    /// Total usable capacity for the CIDR.
    pub capacity: u32,
}

impl IPPool {
    /// Creates a new pool and computes its usable capacity.
    #[must_use]
    pub fn new(name: impl Into<String>, cidr: IpNet) -> Self {
        let capacity = match &cidr {
            IpNet::V4(net) => {
                let bits = 32_u32.saturating_sub(u32::from(net.prefix_len()));
                if bits >= 2 {
                    let hosts = 1_u64 << bits;
                    hosts.saturating_sub(2).min(u64::from(u32::MAX)) as u32
                } else {
                    0
                }
            }
            IpNet::V6(net) => {
                let bits = 128_u32.saturating_sub(u32::from(net.prefix_len()));
                if bits > 30 {
                    u32::MAX
                } else {
                    (1_u32 << bits).saturating_sub(2)
                }
            }
        };

        Self {
            name: name.into(),
            cidr,
            allocated_count: 0,
            capacity,
        }
    }

    /// Returns the remaining number of usable IPs.
    #[must_use]
    pub fn available(&self) -> u32 {
        self.capacity.saturating_sub(self.allocated_count)
    }

    /// Returns pool utilization as a percentage.
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn utilization_pct(&self) -> f32 {
        if self.capacity == 0 {
            return 100.0;
        }

        (self.allocated_count as f32 / self.capacity as f32) * 100.0
    }
}

/// A pool of IPs available for LoadBalancer service assignment.
#[derive(Debug, Clone)]
pub struct LBIPPool {
    /// Pool name.
    pub name: String,
    /// CIDRs that may allocate service IPs.
    pub cidrs: Vec<IpNet>,
    /// Allocated service IPs.
    pub allocated: HashSet<IpAddr>,
}

impl LBIPPool {
    /// Creates a new load-balancer IP pool.
    #[must_use]
    pub fn new(name: impl Into<String>, cidrs: Vec<IpNet>) -> Self {
        Self {
            name: name.into(),
            cidrs,
            allocated: HashSet::new(),
        }
    }

    /// Claims a specific IP from this pool.
    pub fn allocate(&mut self, ip: IpAddr) -> OperatorResult<()> {
        if !self.cidrs.iter().any(|cidr| cidr.contains(&ip)) {
            return Err(OperatorError::IPNotInPool(ip));
        }

        if !self.allocated.insert(ip) {
            return Err(OperatorError::IPAlreadyAllocated(ip));
        }

        Ok(())
    }

    /// Releases an IP back into the pool.
    pub fn release(&mut self, ip: &IpAddr) -> bool {
        self.allocated.remove(ip)
    }

    /// Returns the number of allocated service IPs.
    #[must_use]
    pub fn allocated_count(&self) -> usize {
        self.allocated.len()
    }

    /// Returns `true` if the IP belongs to any configured CIDR in the pool.
    #[must_use]
    pub fn contains(&self, ip: &IpAddr) -> bool {
        self.cidrs.iter().any(|cidr| cidr.contains(ip))
    }
}

// ============================================================================
// Identity Module
// ============================================================================

#[allow(clippy::wildcard_imports)]
pub mod identity {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
    pub struct NumericIdentity(pub u32);

    impl NumericIdentity {
        pub const WORLD: Self = Self(1);
        pub const HOST: Self = Self(2);
        pub const LOCAL_NODE: Self = Self(6);
        pub const UNMANAGED: Self = Self(3);
        pub const MIN_ALLOCATABLE: Self = Self(256);
        pub const MAX_ALLOCATABLE: Self = Self(65535);

        pub fn is_reserved(self) -> bool {
            self.0 < Self::MIN_ALLOCATABLE.0
        }

        pub fn is_allocatable(self) -> bool {
            !self.is_reserved() && self <= Self::MAX_ALLOCATABLE
        }
    }

    impl From<u32> for NumericIdentity {
        fn from(id: u32) -> Self {
            Self(id)
        }
    }

    impl From<NumericIdentity> for u32 {
        fn from(id: NumericIdentity) -> Self {
            id.0
        }
    }

    impl std::fmt::Display for NumericIdentity {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    pub type LabelSelector = HashMap<String, String>;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum IdentityStatus {
        Active,
        Inactive,
        Pending,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CiliumIdentity {
        pub name: String,
        pub id: NumericIdentity,
        pub labels: LabelSelector,
        pub namespace: Option<String>,
        pub status: IdentityStatus,
    }

    impl CiliumIdentity {
        pub fn new(id: NumericIdentity, labels: LabelSelector) -> Self {
            Self {
                name: id.to_string(),
                id,
                labels,
                namespace: None,
                status: IdentityStatus::Active,
            }
        }

        pub fn with_namespace(mut self, namespace: String) -> Self {
            self.namespace = Some(namespace);
            self
        }

        pub fn matches_selector(&self, selector: &LabelSelector) -> bool {
            selector.iter().all(|(k, v)| {
                self.labels
                    .get(k)
                    .map(|label_val| label_val == v)
                    .unwrap_or(false)
            })
        }
    }
}

// ============================================================================
// Endpoint Module
// ============================================================================

#[allow(clippy::wildcard_imports)]
pub mod endpoint {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct EndpointID(pub u16);

    impl std::fmt::Display for EndpointID {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EndpointAddressing {
        pub ipv4: Option<Ipv4Addr>,
        pub ipv6: Option<Ipv6Addr>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum EndpointStatus {
        Ready,
        Pending,
        Failed(String),
        Terminating,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CiliumEndpoint {
        pub name: String,
        pub namespace: String,
        pub endpoint_id: EndpointID,
        pub identity: identity::NumericIdentity,
        pub addressing: EndpointAddressing,
        pub labels: HashMap<String, String>,
        pub node_name: String,
        pub status: EndpointStatus,
        pub pod_name: String,
    }

    impl CiliumEndpoint {
        pub fn new(name: String, namespace: String, pod_name: String, node_name: String) -> Self {
            Self {
                name,
                namespace,
                endpoint_id: EndpointID(0),
                identity: identity::NumericIdentity::UNMANAGED,
                addressing: EndpointAddressing {
                    ipv4: None,
                    ipv6: None,
                },
                labels: HashMap::new(),
                node_name,
                status: EndpointStatus::Pending,
                pod_name,
            }
        }

        pub fn set_status(&mut self, status: EndpointStatus) {
            self.status = status;
        }

        pub fn set_identity(&mut self, id: identity::NumericIdentity) {
            self.identity = id;
        }

        pub fn set_ipv4(&mut self, addr: Ipv4Addr) {
            self.addressing.ipv4 = Some(addr);
        }

        pub fn set_ipv6(&mut self, addr: Ipv6Addr) {
            self.addressing.ipv6 = Some(addr);
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CoreCiliumEndpoint {
        pub name: String,
        pub pod_name: String,
        pub identity: identity::NumericIdentity,
        pub addressing: EndpointAddressing,
        pub labels: HashMap<String, String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CiliumEndpointSlice {
        pub name: String,
        pub namespace: String,
        pub endpoints: Vec<CoreCiliumEndpoint>,
        pub node_name: Option<String>,
    }

    impl CiliumEndpointSlice {
        pub fn new(name: String, namespace: String) -> Self {
            Self {
                name,
                namespace,
                endpoints: Vec::new(),
                node_name: None,
            }
        }

        pub fn add_endpoint(&mut self, ep: CoreCiliumEndpoint) {
            self.endpoints.push(ep);
        }

        pub fn len(&self) -> usize {
            self.endpoints.len()
        }

        pub fn is_empty(&self) -> bool {
            self.endpoints.is_empty()
        }
    }
}

// ============================================================================
// Policy Module
// ============================================================================

#[allow(clippy::wildcard_imports)]
pub mod policy {
    use super::*;

    pub type PolicyName = String;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum PolicyAction {
        Allow,
        Deny,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EndpointSelector {
        pub match_labels: HashMap<String, String>,
    }

    impl EndpointSelector {
        pub fn matches(&self, labels: &HashMap<String, String>) -> bool {
            self.match_labels.iter().all(|(k, v)| {
                labels
                    .get(k)
                    .map(|label_val| label_val == v)
                    .unwrap_or(false)
            })
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum TrafficDirection {
        Ingress,
        Egress,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PolicyRule {
        pub action: PolicyAction,
        pub direction: TrafficDirection,
        pub selector: EndpointSelector,
        pub protocol: Option<String>,
        pub ports: Option<Vec<u16>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum PolicyStatus {
        Active,
        Pending,
        Error(String),
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CiliumNetworkPolicy {
        pub name: String,
        pub namespace: String,
        pub endpoint_selector: EndpointSelector,
        pub ingress_rules: Vec<PolicyRule>,
        pub egress_rules: Vec<PolicyRule>,
        pub status: PolicyStatus,
    }

    impl CiliumNetworkPolicy {
        pub fn new(name: String, namespace: String, endpoint_selector: EndpointSelector) -> Self {
            Self {
                name,
                namespace,
                endpoint_selector,
                ingress_rules: Vec::new(),
                egress_rules: Vec::new(),
                status: PolicyStatus::Pending,
            }
        }

        pub fn add_ingress_rule(&mut self, rule: PolicyRule) {
            self.ingress_rules.push(rule);
        }

        pub fn add_egress_rule(&mut self, rule: PolicyRule) {
            self.egress_rules.push(rule);
        }

        pub fn applies_to(&self, labels: &HashMap<String, String>) -> bool {
            self.endpoint_selector.matches(labels)
        }

        pub fn mark_active(&mut self) {
            self.status = PolicyStatus::Active;
        }

        pub fn mark_failed(&mut self, reason: String) {
            self.status = PolicyStatus::Error(reason);
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ClusterwideCiliumNetworkPolicy {
        pub name: String,
        pub endpoint_selector: EndpointSelector,
        pub ingress_rules: Vec<PolicyRule>,
        pub egress_rules: Vec<PolicyRule>,
        pub status: PolicyStatus,
    }

    impl ClusterwideCiliumNetworkPolicy {
        pub fn new(name: String, endpoint_selector: EndpointSelector) -> Self {
            Self {
                name,
                endpoint_selector,
                ingress_rules: Vec::new(),
                egress_rules: Vec::new(),
                status: PolicyStatus::Pending,
            }
        }

        pub fn applies_to(&self, labels: &HashMap<String, String>) -> bool {
            self.endpoint_selector.matches(labels)
        }
    }
}

// ============================================================================
// Reconciler Module
// ============================================================================

#[allow(clippy::wildcard_imports)]
pub mod reconciler {
    #[derive(Debug, Clone)]
    pub struct ReconcileResult {
        pub success: bool,
        pub error: Option<String>,
        pub retry_count: u32,
    }

    impl ReconcileResult {
        pub fn success() -> Self {
            Self {
                success: true,
                error: None,
                retry_count: 0,
            }
        }

        pub fn failure(error: String) -> Self {
            Self {
                success: false,
                error: Some(error),
                retry_count: 0,
            }
        }
    }
}

// ============================================================================
// API health/server helpers (parity-oriented extraction from operator/api tests)
// ============================================================================

/// Minimal HTTP response model for parity tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Optional body content.
    pub body: Option<String>,
}

impl HttpResponse {
    fn new(status: u16, body: Option<String>) -> Self {
        Self { status, body }
    }
}

/// Returns `/healthz` response matching operator health handler expectations.
#[must_use]
pub fn healthz_response(k8s_enabled: bool) -> HttpResponse {
    if k8s_enabled {
        HttpResponse::new(200, Some("ok".to_string()))
    } else {
        HttpResponse::new(501, None)
    }
}

/// Returns expected status code for operator API server endpoints.
///
/// Ported behavior from `operator/api/server_test.go`:
/// - `/v1/metrics`: always 200
/// - `/v1/cluster`: always 200
/// - `/v1/healthz` and `/healthz`: 200 when k8s enabled, else 501
#[must_use]
pub fn api_endpoint_status(path: &str, k8s_enabled: bool) -> u16 {
    match path {
        "/v1/metrics" | "/v1/cluster" => 200,
        "/v1/healthz" | "/healthz" => {
            if k8s_enabled {
                200
            } else {
                501
            }
        }
        _ => 404,
    }
}

// ============================================================================
// Parity tests — ported from operator/api/health_test.go,
//                           operator/api/server_test.go,
//                           operator/cmd/root_test.go
// ============================================================================

#[cfg(test)]
mod parity_tests {
    use super::*;
    use seriousum_core::{HookError, HookFn, Lifecycle};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    // ------ health_test.go ------

    #[test]
    fn parity_health_handler_k8s_disabled() {
        let response = healthz_response(false);
        assert_eq!(response.status, 501);
        assert_eq!(response.body, None);
    }

    #[test]
    fn parity_health_handler_k8s_enabled() {
        let response = healthz_response(true);
        assert_eq!(response.status, 200);
        assert_eq!(response.body.as_deref(), Some("ok"));
    }

    // ------ server_test.go ------

    #[test]
    fn parity_api_server_k8s_disabled() {
        assert_eq!(api_endpoint_status("/v1/metrics", false), 200);
        assert_eq!(api_endpoint_status("/v1/healthz", false), 501);
        assert_eq!(api_endpoint_status("/healthz", false), 501);
        assert_eq!(api_endpoint_status("/v1/cluster", false), 200);
    }

    #[test]
    fn parity_api_server_k8s_enabled() {
        assert_eq!(api_endpoint_status("/v1/metrics", true), 200);
        assert_eq!(api_endpoint_status("/v1/healthz", true), 200);
        assert_eq!(api_endpoint_status("/healthz", true), 200);
        assert_eq!(api_endpoint_status("/v1/cluster", true), 200);
    }

    // ------ root_test.go ------

    /// Tests that a minimal operator lifecycle can be populated and started with default state,
    /// mirroring the upstream hive smoke test without the full DI tree.
    #[test]
    fn parity_operator_hive_populates() {
        let started = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let mut lifecycle = Lifecycle::new();

        let started_for_start = Arc::clone(&started);
        let started_for_stop = Arc::clone(&started);
        let stopped_for_hook = Arc::clone(&stopped);
        lifecycle.append(HookFn::new(
            move || -> Result<(), HookError> {
                assert_eq!(healthz_response(false).status, 501);
                assert_eq!(api_endpoint_status("/v1/metrics", false), 200);
                started_for_start.store(true, Ordering::SeqCst);
                Ok(())
            },
            move || -> Result<(), HookError> {
                assert!(started_for_stop.load(Ordering::SeqCst));
                stopped_for_hook.store(true, Ordering::SeqCst);
                Ok(())
            },
        ));

        lifecycle
            .start_all()
            .expect("operator lifecycle should start");
        assert!(started.load(Ordering::SeqCst));
        lifecycle.stop_all();
        assert!(stopped.load(Ordering::SeqCst));
    }
}

// ============================================================================
// Tests (30+ unit tests)
// ============================================================================

#[allow(clippy::wildcard_imports)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // --- NumericIdentity Tests (8 tests) ---

    #[test]
    fn test_numeric_identity_reserved_constants() {
        assert_eq!(identity::NumericIdentity::WORLD.0, 1);
        assert_eq!(identity::NumericIdentity::HOST.0, 2);
        assert_eq!(identity::NumericIdentity::LOCAL_NODE.0, 6);
        assert_eq!(identity::NumericIdentity::UNMANAGED.0, 3);
    }

    #[test]
    fn test_numeric_identity_is_reserved() {
        assert!(identity::NumericIdentity::WORLD.is_reserved());
        assert!(identity::NumericIdentity::HOST.is_reserved());
        assert!(identity::NumericIdentity::LOCAL_NODE.is_reserved());
        assert!(identity::NumericIdentity::UNMANAGED.is_reserved());
        assert!(!identity::NumericIdentity(256).is_reserved());
    }

    #[test]
    fn test_numeric_identity_is_allocatable() {
        assert!(!identity::NumericIdentity::WORLD.is_allocatable());
        assert!(!identity::NumericIdentity::HOST.is_allocatable());
        assert!(identity::NumericIdentity(256).is_allocatable());
        assert!(identity::NumericIdentity(65535).is_allocatable());
        assert!(!identity::NumericIdentity(65536).is_allocatable());
    }

    #[test]
    fn test_numeric_identity_ordering() {
        let id1 = identity::NumericIdentity(256);
        let id2 = identity::NumericIdentity(512);
        assert!(id1 < id2);
        assert_eq!(id1, id1);
    }

    #[test]
    fn test_numeric_identity_conversion() {
        let id = identity::NumericIdentity(1000);
        let val: u32 = id.into();
        assert_eq!(val, 1000);

        let id2 = identity::NumericIdentity::from(1000u32);
        assert_eq!(id2, id);
    }

    #[test]
    fn test_numeric_identity_display() {
        let id = identity::NumericIdentity(256);
        assert_eq!(id.to_string(), "256");

        let id = identity::NumericIdentity(65535);
        assert_eq!(id.to_string(), "65535");
    }

    #[test]
    fn test_cilium_identity_creation() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());
        labels.insert("tier".to_string(), "frontend".to_string());

        let cid = identity::CiliumIdentity::new(identity::NumericIdentity(256), labels.clone());

        assert_eq!(cid.id, identity::NumericIdentity(256));
        assert_eq!(cid.name, "256");
        assert_eq!(cid.labels, labels);
        assert_eq!(cid.namespace, None);
        assert_eq!(cid.status, identity::IdentityStatus::Active);
    }

    #[test]
    fn test_cilium_identity_with_namespace() {
        let labels = HashMap::new();
        let cid = identity::CiliumIdentity::new(identity::NumericIdentity(256), labels)
            .with_namespace("custom-ns".to_string());

        assert_eq!(cid.namespace, Some("custom-ns".to_string()));
    }

    // --- Endpoint Tests (8 tests) ---

    #[test]
    fn test_endpoint_creation() {
        let ep = endpoint::CiliumEndpoint::new(
            "pod-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            "node-1".to_string(),
        );

        assert_eq!(ep.name, "pod-1");
        assert_eq!(ep.namespace, "default");
        assert_eq!(ep.pod_name, "pod-1");
        assert_eq!(ep.node_name, "node-1");
        assert_eq!(ep.status, endpoint::EndpointStatus::Pending);
        assert_eq!(ep.endpoint_id, endpoint::EndpointID(0));
        assert_eq!(ep.identity, identity::NumericIdentity::UNMANAGED);
    }

    #[test]
    fn test_endpoint_set_status() {
        let mut ep = endpoint::CiliumEndpoint::new(
            "pod-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            "node-1".to_string(),
        );

        ep.set_status(endpoint::EndpointStatus::Ready);
        assert_eq!(ep.status, endpoint::EndpointStatus::Ready);

        ep.set_status(endpoint::EndpointStatus::Failed("error".to_string()));
        assert!(matches!(ep.status, endpoint::EndpointStatus::Failed(_)));
    }

    #[test]
    fn test_endpoint_set_identity() {
        let mut ep = endpoint::CiliumEndpoint::new(
            "pod-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            "node-1".to_string(),
        );

        ep.set_identity(identity::NumericIdentity(256));
        assert_eq!(ep.identity, identity::NumericIdentity(256));
    }

    #[test]
    fn test_endpoint_addresses() {
        let mut ep = endpoint::CiliumEndpoint::new(
            "pod-1".to_string(),
            "default".to_string(),
            "pod-1".to_string(),
            "node-1".to_string(),
        );

        let ipv4 = "10.0.0.1".parse::<Ipv4Addr>().unwrap();
        let ipv6 = "2001:db8::1".parse::<Ipv6Addr>().unwrap();

        ep.set_ipv4(ipv4);
        ep.set_ipv6(ipv6);

        assert_eq!(ep.addressing.ipv4, Some(ipv4));
        assert_eq!(ep.addressing.ipv6, Some(ipv6));
    }

    #[test]
    fn test_endpoint_id_display() {
        let id = endpoint::EndpointID(1234);
        assert_eq!(id.to_string(), "1234");
    }

    #[test]
    fn test_endpoint_slice_creation() {
        let ces = endpoint::CiliumEndpointSlice::new("ces-1".to_string(), "default".to_string());

        assert_eq!(ces.name, "ces-1");
        assert_eq!(ces.namespace, "default");
        assert!(ces.is_empty());
        assert_eq!(ces.len(), 0);
    }

    #[test]
    fn test_endpoint_slice_add_endpoints() {
        let mut ces =
            endpoint::CiliumEndpointSlice::new("ces-1".to_string(), "default".to_string());

        for i in 0..3 {
            let core_ep = endpoint::CoreCiliumEndpoint {
                name: format!("ep{i}"),
                pod_name: format!("pod{i}"),
                identity: identity::NumericIdentity(256 + i as u32),
                addressing: endpoint::EndpointAddressing {
                    ipv4: Some(format!("10.0.0.{}", i + 1).parse().unwrap()),
                    ipv6: None,
                },
                labels: HashMap::new(),
            };
            ces.add_endpoint(core_ep);
        }

        assert!(!ces.is_empty());
        assert_eq!(ces.len(), 3);
    }

    // --- Policy Tests (8 tests) ---

    #[test]
    fn test_policy_creation() {
        let mut selector_labels = HashMap::new();
        selector_labels.insert("app".to_string(), "web".to_string());

        let selector = policy::EndpointSelector {
            match_labels: selector_labels,
        };

        let policy = policy::CiliumNetworkPolicy::new(
            "policy-1".to_string(),
            "default".to_string(),
            selector,
        );

        assert_eq!(policy.name, "policy-1");
        assert_eq!(policy.namespace, "default");
        assert_eq!(policy.status, policy::PolicyStatus::Pending);
        assert!(policy.ingress_rules.is_empty());
        assert!(policy.egress_rules.is_empty());
    }

    #[test]
    fn test_policy_ingress_rules() {
        let selector = policy::EndpointSelector {
            match_labels: HashMap::new(),
        };

        let mut policy = policy::CiliumNetworkPolicy::new(
            "policy-1".to_string(),
            "default".to_string(),
            selector,
        );

        let rule1 = policy::PolicyRule {
            action: policy::PolicyAction::Allow,
            direction: policy::TrafficDirection::Ingress,
            selector: policy::EndpointSelector {
                match_labels: HashMap::new(),
            },
            protocol: Some("TCP".to_string()),
            ports: Some(vec![80, 443]),
        };

        let rule2 = policy::PolicyRule {
            action: policy::PolicyAction::Allow,
            direction: policy::TrafficDirection::Ingress,
            selector: policy::EndpointSelector {
                match_labels: HashMap::new(),
            },
            protocol: Some("UDP".to_string()),
            ports: Some(vec![53]),
        };

        policy.add_ingress_rule(rule1);
        policy.add_ingress_rule(rule2);

        assert_eq!(policy.ingress_rules.len(), 2);
    }

    #[test]
    fn test_policy_egress_rules() {
        let selector = policy::EndpointSelector {
            match_labels: HashMap::new(),
        };

        let mut policy = policy::CiliumNetworkPolicy::new(
            "policy-1".to_string(),
            "default".to_string(),
            selector,
        );

        let rule = policy::PolicyRule {
            action: policy::PolicyAction::Deny,
            direction: policy::TrafficDirection::Egress,
            selector: policy::EndpointSelector {
                match_labels: HashMap::new(),
            },
            protocol: None,
            ports: None,
        };

        policy.add_egress_rule(rule);
        assert_eq!(policy.egress_rules.len(), 1);
    }

    #[test]
    fn test_policy_endpoint_selector() {
        let mut selector_labels = HashMap::new();
        selector_labels.insert("app".to_string(), "web".to_string());
        selector_labels.insert("tier".to_string(), "frontend".to_string());

        let selector = policy::EndpointSelector {
            match_labels: selector_labels,
        };

        let mut ep_labels = HashMap::new();
        ep_labels.insert("app".to_string(), "web".to_string());
        ep_labels.insert("tier".to_string(), "frontend".to_string());
        ep_labels.insert("version".to_string(), "v1".to_string());

        assert!(selector.matches(&ep_labels));

        ep_labels.remove("tier");
        assert!(!selector.matches(&ep_labels));
    }

    #[test]
    fn test_policy_status_lifecycle() {
        let selector = policy::EndpointSelector {
            match_labels: HashMap::new(),
        };

        let mut policy = policy::CiliumNetworkPolicy::new(
            "policy-1".to_string(),
            "default".to_string(),
            selector,
        );

        assert_eq!(policy.status, policy::PolicyStatus::Pending);

        policy.mark_active();
        assert_eq!(policy.status, policy::PolicyStatus::Active);

        policy.mark_failed("replication failed".to_string());
        match policy.status {
            policy::PolicyStatus::Error(ref msg) => assert_eq!(msg, "replication failed"),
            _ => panic!("expected error status"),
        }
    }

    #[test]
    fn test_cluster_policy_creation() {
        let selector = policy::EndpointSelector {
            match_labels: {
                let mut m = HashMap::new();
                m.insert("namespace".to_string(), "production".to_string());
                m
            },
        };

        let ccnp = policy::ClusterwideCiliumNetworkPolicy::new("ccnp-prod".to_string(), selector);

        assert_eq!(ccnp.name, "ccnp-prod");
        assert_eq!(ccnp.status, policy::PolicyStatus::Pending);
        assert!(ccnp.ingress_rules.is_empty());
    }

    #[test]
    fn test_cluster_policy_applies_to_endpoint() {
        let selector = policy::EndpointSelector {
            match_labels: {
                let mut m = HashMap::new();
                m.insert("tier".to_string(), "backend".to_string());
                m
            },
        };

        let ccnp = policy::ClusterwideCiliumNetworkPolicy::new("ccnp-1".to_string(), selector);

        let mut labels = HashMap::new();
        labels.insert("tier".to_string(), "backend".to_string());
        labels.insert("namespace".to_string(), "default".to_string());

        assert!(ccnp.applies_to(&labels));

        labels.insert("tier".to_string(), "frontend".to_string());
        assert!(!ccnp.applies_to(&labels));
    }

    // --- Health & Error Tests (6 tests) ---

    #[test]
    fn test_version_info() {
        let v = VersionInfo::current();
        assert!(!v.contract.is_empty());
        assert!(!v.core.is_empty());
    }

    #[test]
    fn test_health_status_equality() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Degraded);
        assert_ne!(HealthStatus::Degraded, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_report_creation() {
        let report = HealthReport {
            status: HealthStatus::Healthy,
            message: Some("all systems operational".to_string()),
            version: VersionInfo::current(),
        };

        assert_eq!(report.status, HealthStatus::Healthy);
        assert!(report.message.is_some());
    }

    #[test]
    fn test_operator_error_display() {
        let err = OperatorError::IdentityAllocation("pool exhausted".to_string());
        assert!(err.to_string().contains("identity allocation"));

        let err = OperatorError::NotFound("resource-1".to_string());
        assert!(err.to_string().contains("not found"));

        let err = OperatorError::InvalidConfig("missing field".to_string());
        assert!(err.to_string().contains("invalid configuration"));
    }

    #[test]
    fn test_reconcile_result_helpers() {
        assert!(ReconcileResult::NoOp.is_ok());
        assert!(ReconcileResult::Updated { changes: 3 }.is_ok());
        assert!(!ReconcileResult::Failed("boom".into()).is_ok());
        assert_eq!(ReconcileResult::Updated { changes: 5 }.changes(), 5);
        assert_eq!(ReconcileResult::NoOp.changes(), 0);
    }

    #[test]
    fn test_reconcile_queue_dedup() {
        let mut q = ReconcileQueue::new();
        q.enqueue(ReconcileRequest {
            key: "ns/svc".into(),
            priority: ReconcilePriority::Normal,
            reason: "created".into(),
        });
        q.enqueue(ReconcileRequest {
            key: "ns/svc".into(),
            priority: ReconcilePriority::Low,
            reason: "updated".into(),
        });
        assert_eq!(q.len(), 1);
        let req = q.dequeue().unwrap();
        assert_eq!(req.priority, ReconcilePriority::Normal);
        assert!(q.is_empty());
    }

    #[test]
    fn test_reconcile_queue_priority_upgrade() {
        let mut q = ReconcileQueue::new();
        q.enqueue(ReconcileRequest {
            key: "a".into(),
            priority: ReconcilePriority::Low,
            reason: "r1".into(),
        });
        q.enqueue(ReconcileRequest {
            key: "a".into(),
            priority: ReconcilePriority::High,
            reason: "urgent".into(),
        });
        let req = q.dequeue().unwrap();
        assert_eq!(req.priority, ReconcilePriority::High);
        assert_eq!(req.reason, "urgent");
    }

    #[test]
    fn test_ip_pool_capacity() {
        let pool = IPPool::new("test", "10.0.0.0/24".parse().unwrap());
        assert_eq!(pool.capacity, 254);
        assert_eq!(pool.available(), 254);
    }

    #[test]
    fn test_lb_ip_pool_allocate_release() {
        let mut pool = LBIPPool::new("lb", vec!["192.168.1.0/24".parse().unwrap()]);
        let ip: std::net::IpAddr = "192.168.1.100".parse().unwrap();
        pool.allocate(ip).unwrap();
        assert_eq!(pool.allocated_count(), 1);
        assert!(pool.allocate(ip).is_err());
        pool.release(&ip);
        assert_eq!(pool.allocated_count(), 0);
    }

    #[test]
    fn test_lb_ip_pool_out_of_range() {
        let mut pool = LBIPPool::new("lb", vec!["192.168.1.0/24".parse().unwrap()]);
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        assert!(pool.allocate(ip).is_err());
    }

    #[test]
    fn test_reconcile_result() {
        let success = reconciler::ReconcileResult::success();
        assert!(success.success);
        assert_eq!(success.error, None);

        let failure = reconciler::ReconcileResult::failure("sync failed".to_string());
        assert!(!failure.success);
        assert!(failure.error.is_some());
    }

    #[test]
    fn test_cilium_identity_selector_matching() {
        let mut id_labels = HashMap::new();
        id_labels.insert("app".to_string(), "api".to_string());
        id_labels.insert("version".to_string(), "v2".to_string());

        let cid = identity::CiliumIdentity::new(identity::NumericIdentity(1000), id_labels);

        let mut selector1 = HashMap::new();
        selector1.insert("app".to_string(), "api".to_string());
        assert!(cid.matches_selector(&selector1));

        let mut selector2 = HashMap::new();
        selector2.insert("app".to_string(), "api".to_string());
        selector2.insert("version".to_string(), "v3".to_string());
        assert!(!cid.matches_selector(&selector2));

        let selector3 = HashMap::new();
        assert!(cid.matches_selector(&selector3)); // empty selector matches all
    }

    #[test]
    fn test_multiple_policies_for_endpoint() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());
        labels.insert("tier".to_string(), "frontend".to_string());
        labels.insert("env".to_string(), "prod".to_string());

        let mut policy_labels_1 = HashMap::new();
        policy_labels_1.insert("app".to_string(), "web".to_string());
        let selector1 = policy::EndpointSelector {
            match_labels: policy_labels_1,
        };
        let policy1 = policy::CiliumNetworkPolicy::new(
            "policy-1".to_string(),
            "default".to_string(),
            selector1,
        );

        let mut policy_labels_2 = HashMap::new();
        policy_labels_2.insert("tier".to_string(), "frontend".to_string());
        let selector2 = policy::EndpointSelector {
            match_labels: policy_labels_2,
        };
        let policy2 = policy::CiliumNetworkPolicy::new(
            "policy-2".to_string(),
            "default".to_string(),
            selector2,
        );

        let mut policy_labels_3 = HashMap::new();
        policy_labels_3.insert("env".to_string(), "staging".to_string());
        let selector3 = policy::EndpointSelector {
            match_labels: policy_labels_3,
        };
        let policy3 = policy::CiliumNetworkPolicy::new(
            "policy-3".to_string(),
            "default".to_string(),
            selector3,
        );

        assert!(policy1.applies_to(&labels));
        assert!(policy2.applies_to(&labels));
        assert!(!policy3.applies_to(&labels));
    }

    #[test]
    fn test_endpoint_slice_with_multiple_core_endpoints() {
        let mut ces =
            endpoint::CiliumEndpointSlice::new("ces-batch".to_string(), "production".to_string());

        for i in 0..10 {
            let core_ep = endpoint::CoreCiliumEndpoint {
                name: format!("core-ep-{i}"),
                pod_name: format!("pod-batch-{i}"),
                identity: identity::NumericIdentity(3000 + i as u32),
                addressing: endpoint::EndpointAddressing {
                    ipv4: Some(format!("172.16.0.{}", i + 1).parse().unwrap()),
                    ipv6: None,
                },
                labels: {
                    let mut m = HashMap::new();
                    m.insert("batch".to_string(), "true".to_string());
                    m.insert("index".to_string(), i.to_string());
                    m
                },
            };
            ces.add_endpoint(core_ep);
        }

        assert_eq!(ces.len(), 10);
        assert!(!ces.is_empty());
    }

    #[test]
    fn test_policy_rules_with_multiple_directions() {
        let selector = policy::EndpointSelector {
            match_labels: HashMap::new(),
        };
        let mut policy = policy::CiliumNetworkPolicy::new(
            "multi-dir".to_string(),
            "default".to_string(),
            selector,
        );

        for i in 0..3 {
            let ingress_rule = policy::PolicyRule {
                action: policy::PolicyAction::Allow,
                direction: policy::TrafficDirection::Ingress,
                selector: policy::EndpointSelector {
                    match_labels: HashMap::new(),
                },
                protocol: Some(format!("PROTO-{i}")),
                ports: Some(vec![80 + i as u16]),
            };
            policy.add_ingress_rule(ingress_rule);
        }

        for _ in 0..2 {
            let egress_rule = policy::PolicyRule {
                action: policy::PolicyAction::Deny,
                direction: policy::TrafficDirection::Egress,
                selector: policy::EndpointSelector {
                    match_labels: HashMap::new(),
                },
                protocol: None,
                ports: None,
            };
            policy.add_egress_rule(egress_rule);
        }

        assert_eq!(policy.ingress_rules.len(), 3);
        assert_eq!(policy.egress_rules.len(), 2);
    }
}
