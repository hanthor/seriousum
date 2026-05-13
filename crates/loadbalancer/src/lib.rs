#![allow(clippy::upper_case_acronyms)]
//! Core load-balancer value types ported from Cilium's `pkg/loadbalancer`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use thiserror::Error;

/// Result type for pure load-balancer operations.
pub type Result<T> = std::result::Result<T, LoadBalancerError>;

/// Errors returned by pure load-balancer helpers.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoadBalancerError {
    /// The requested backend does not exist in the service.
    #[error("backend not found: {0}")]
    BackendNotFound(BackendID),

    /// The requested state transition is not allowed.
    #[error("invalid backend state transition from {from} to {to}")]
    InvalidBackendStateTransition {
        /// Current backend state.
        from: BackendState,
        /// Requested backend state.
        to: BackendState,
    },
}

/// Service identifier allocated for a frontend.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct ServiceID(pub u32);

impl ServiceID {
    /// The zero service identifier.
    pub const ZERO: Self = Self(0);
}

impl fmt::Display for ServiceID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Fully-qualified service name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceName {
    /// Kubernetes namespace.
    pub namespace: String,
    /// Kubernetes service name.
    pub name: String,
    /// Optional cluster name for clustermesh services.
    pub cluster: Option<String>,
}

impl ServiceName {
    /// Creates a service name in the local cluster.
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            cluster: None,
        }
    }

    /// Associates the service with a specific cluster.
    pub fn with_cluster(mut self, cluster: impl Into<String>) -> Self {
        self.cluster = Some(cluster.into());
        self
    }

    /// Returns whether two service names are deeply equal.
    pub fn deep_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl fmt::Display for ServiceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(cluster) = &self.cluster {
            write!(f, "{cluster}/{}/{}", self.namespace, self.name)
        } else {
            write!(f, "{}/{}", self.namespace, self.name)
        }
    }
}

/// Supported L4 protocols for service and backend addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum L4Type {
    /// No protocol was specified.
    None,
    /// Wildcard protocol.
    Any,
    /// TCP.
    TCP,
    /// UDP.
    UDP,
    /// SCTP.
    SCTP,
}

impl fmt::Display for L4Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::None => "NONE",
            Self::Any => "ANY",
            Self::TCP => "TCP",
            Self::UDP => "UDP",
            Self::SCTP => "SCTP",
        };
        f.write_str(value)
    }
}

/// Layer 4 address consisting of protocol and port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct L4Addr {
    /// Layer 4 protocol.
    pub protocol: L4Type,
    /// Transport port.
    pub port: u16,
}

impl L4Addr {
    /// Creates a new L4 address.
    pub const fn new(protocol: L4Type, port: u16) -> Self {
        Self { protocol, port }
    }

    /// Returns whether two L4 addresses are deeply equal.
    pub fn deep_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl fmt::Display for L4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.port, self.protocol)
    }
}

/// L3/L4 address used by frontends and backends.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct L3n4Addr {
    /// IP address.
    pub ip: IpAddr,
    /// Layer 4 address.
    pub l4_addr: L4Addr,
}

impl L3n4Addr {
    /// Creates a new L3/L4 address.
    pub const fn new(ip: IpAddr, port: u16, protocol: L4Type) -> Self {
        Self {
            ip,
            l4_addr: L4Addr::new(protocol, port),
        }
    }

    /// Returns the transport port.
    pub const fn port(&self) -> u16 {
        self.l4_addr.port
    }

    /// Returns the L4 protocol.
    pub const fn protocol(&self) -> L4Type {
        self.l4_addr.protocol
    }

    /// Returns whether the address is IPv6.
    pub fn is_ipv6(&self) -> bool {
        self.ip.is_ipv6()
    }

    /// Returns whether two addresses are deeply equal.
    pub fn deep_equals(&self, other: &Self) -> bool {
        self == other
    }

    /// Formats the address in Cilium's `ip:port/proto` style.
    pub fn string_with_protocol(&self) -> String {
        if self.is_ipv6() {
            format!("[{}]:{}/{}", self.ip, self.port(), self.protocol())
        } else {
            format!("{}:{}/{}", self.ip, self.port(), self.protocol())
        }
    }
}

impl fmt::Display for L3n4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string_with_protocol())
    }
}

/// L3/L4 address paired with a numeric identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct L3n4AddrID {
    /// Address portion.
    pub addr: L3n4Addr,
    /// Numeric identifier.
    pub id: u32,
}

impl L3n4AddrID {
    /// Creates a new identified address.
    pub fn new(addr: L3n4Addr, id: impl Into<u32>) -> Self {
        Self {
            addr,
            id: id.into(),
        }
    }

    /// Returns whether two identified addresses are deeply equal.
    pub fn deep_equals(&self, other: &Self) -> bool {
        self == other
    }

    /// Returns the identifier as a service ID.
    pub const fn service_id(&self) -> ServiceID {
        ServiceID(self.id)
    }
}

impl fmt::Display for L3n4AddrID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.addr, self.id)
    }
}

/// Backend identifier.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct BackendID(pub u32);

impl fmt::Display for BackendID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for ServiceID {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<ServiceID> for u32 {
    fn from(value: ServiceID) -> Self {
        value.0
    }
}

impl From<u32> for BackendID {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<BackendID> for u32 {
    fn from(value: BackendID) -> Self {
        value.0
    }
}

/// Backend state used for reconciliation and traffic eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendState {
    /// Eligible for load-balancing.
    Active,
    /// Gracefully terminating and only used as fallback.
    Terminating,
    /// Temporarily quarantined and not selected.
    Quarantined,
    /// Manually held out of rotation.
    Maintenance,
}

impl BackendState {
    /// Returns whether a transition to `next` is allowed.
    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Active | Self::Quarantined, Self::Terminating)
                    | (Self::Active, Self::Quarantined | Self::Maintenance)
                    | (Self::Quarantined | Self::Maintenance, Self::Active)
            )
    }
}

impl fmt::Display for BackendState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Active => "active",
            Self::Terminating => "terminating",
            Self::Quarantined => "quarantined",
            Self::Maintenance => "maintenance",
        };
        f.write_str(value)
    }
}

/// Backend entry consisting of an address, numeric ID, and current state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Backend {
    /// Address and numeric identifier.
    pub address: L3n4AddrID,
    /// Current backend state.
    pub state: BackendState,
}

impl Backend {
    /// Creates a new active backend.
    pub fn new(id: BackendID, addr: L3n4Addr) -> Self {
        Self::with_state(id, addr, BackendState::Active)
    }

    /// Creates a backend with an explicit state.
    pub fn with_state(id: BackendID, addr: L3n4Addr, state: BackendState) -> Self {
        Self {
            address: L3n4AddrID::new(addr, id),
            state,
        }
    }

    /// Returns the backend identifier.
    pub const fn id(&self) -> BackendID {
        BackendID(self.address.id)
    }

    /// Returns whether this backend is eligible for traffic.
    pub fn is_active(&self) -> bool {
        self.state == BackendState::Active
    }

    /// Updates the backend state if the transition is valid.
    pub fn transition_to(&mut self, next: BackendState) -> Result<()> {
        if !self.state.can_transition_to(next) {
            return Err(LoadBalancerError::InvalidBackendStateTransition {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        Ok(())
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.address.addr, self.state)
    }
}

/// Cilium service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SVCType {
    /// No service type.
    None,
    /// HostPort service.
    HostPort,
    /// ClusterIP service.
    ClusterIP,
    /// NodePort service.
    NodePort,
    /// ExternalIPs service.
    ExternalIPs,
    /// LoadBalancer service.
    LoadBalancer,
    /// Local redirect service.
    LocalRedirect,
}

impl fmt::Display for SVCType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::None => "NONE",
            Self::HostPort => "HostPort",
            Self::ClusterIP => "ClusterIP",
            Self::NodePort => "NodePort",
            Self::ExternalIPs => "ExternalIPs",
            Self::LoadBalancer => "LoadBalancer",
            Self::LocalRedirect => "LocalRedirect",
        };
        f.write_str(value)
    }
}

/// Service traffic policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SVCTrafficPolicy {
    /// No traffic policy was specified.
    None,
    /// Route to any healthy backend.
    Cluster,
    /// Route only to local backends.
    Local,
}

impl fmt::Display for SVCTrafficPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::None => "NONE",
            Self::Cluster => "Cluster",
            Self::Local => "Local",
        };
        f.write_str(value)
    }
}

/// NAT policy applied to backend traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SVCNatPolicy {
    /// No address-family translation.
    None,
    /// Translate IPv4 frontends to IPv6 backends.
    Nat46,
    /// Translate IPv6 frontends to IPv4 backends.
    Nat64,
}

impl fmt::Display for SVCNatPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::None => "NONE",
            Self::Nat46 => "Nat46",
            Self::Nat64 => "Nat64",
        };
        f.write_str(value)
    }
}

/// Service definition containing a frontend and its backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SVC {
    /// Optional Kubernetes service name.
    pub name: Option<ServiceName>,
    /// Service frontend address and identifier.
    pub frontend: L3n4AddrID,
    /// Backends serving the frontend.
    pub backends: Vec<Backend>,
    /// Service type.
    pub svc_type: SVCType,
    /// External traffic policy.
    pub ext_traffic_policy: SVCTrafficPolicy,
    /// Internal traffic policy.
    pub int_traffic_policy: SVCTrafficPolicy,
    /// NAT policy.
    pub nat_policy: SVCNatPolicy,
}

impl SVC {
    /// Creates a new service with default policies.
    pub fn new(frontend: L3n4AddrID) -> Self {
        Self {
            name: None,
            frontend,
            backends: Vec::new(),
            svc_type: SVCType::ClusterIP,
            ext_traffic_policy: SVCTrafficPolicy::Cluster,
            int_traffic_policy: SVCTrafficPolicy::Cluster,
            nat_policy: SVCNatPolicy::None,
        }
    }

    /// Returns the frontend identifier as a service ID.
    pub const fn service_id(&self) -> ServiceID {
        self.frontend.service_id()
    }

    /// Returns all backends currently in the active state.
    pub fn active_backends(&self) -> Vec<&Backend> {
        self.backends
            .iter()
            .filter(|backend| backend.is_active())
            .collect()
    }

    /// Returns whether the service already contains the backend ID.
    pub fn has_backend(&self, backend_id: BackendID) -> bool {
        self.backends
            .iter()
            .any(|backend| backend.id() == backend_id)
    }

    /// Updates the state of a backend referenced by ID.
    pub fn update_backend_state(
        &mut self,
        backend_id: BackendID,
        next_state: BackendState,
    ) -> Result<()> {
        let backend = self
            .backends
            .iter_mut()
            .find(|backend| backend.id() == backend_id)
            .ok_or(LoadBalancerError::BackendNotFound(backend_id))?;
        backend.transition_to(next_state)
    }
}

impl fmt::Display for SVC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -> {} backends ({})",
            self.frontend.addr,
            self.backends.len(),
            self.svc_type
        )
    }
}

/// Backend reconciliation delta between desired and actual state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BackendDiff {
    /// Backends present only in the desired state.
    pub added: Vec<Backend>,
    /// Backends present only in the actual state.
    pub removed: Vec<Backend>,
    /// Backends with the same ID but different content.
    pub updated: Vec<Backend>,
}

impl BackendDiff {
    /// Returns `true` when no reconciliation is required.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }
}

/// Computes the backend reconciliation delta between desired and actual state.
pub fn diff_backends(desired: &[Backend], actual: &[Backend]) -> BackendDiff {
    let desired_by_id: HashMap<BackendID, &Backend> = desired
        .iter()
        .map(|backend| (backend.id(), backend))
        .collect();
    let actual_by_id: HashMap<BackendID, &Backend> = actual
        .iter()
        .map(|backend| (backend.id(), backend))
        .collect();

    let added = desired
        .iter()
        .filter(|backend| !actual_by_id.contains_key(&backend.id()))
        .cloned()
        .collect();

    let updated = desired
        .iter()
        .filter_map(|backend| match actual_by_id.get(&backend.id()) {
            Some(actual_backend) if *actual_backend != backend => Some(backend.clone()),
            _ => None,
        })
        .collect();

    let removed = actual
        .iter()
        .filter(|backend| !desired_by_id.contains_key(&backend.id()))
        .cloned()
        .collect();

    BackendDiff {
        added,
        removed,
        updated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet, hash_map::DefaultHasher};
    use std::hash::{Hash, Hasher};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn backend(id: u32, ip: IpAddr, state: BackendState) -> Backend {
        Backend::with_state(id.into(), L3n4Addr::new(ip, 8080, L4Type::TCP), state)
    }

    #[test]
    fn l3n4addr_creation_string_and_equality() {
        let ipv4 = L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 80, L4Type::TCP);
        let ipv4_same = L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 80, L4Type::TCP);
        let ipv6 = L3n4Addr::new(
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            443,
            L4Type::UDP,
        );

        assert_eq!(ipv4.to_string(), "10.0.0.1:80/TCP");
        assert_eq!(ipv6.to_string(), "[2001:db8::1]:443/UDP");
        assert!(ipv4.deep_equals(&ipv4_same));
        assert_eq!(ipv4, ipv4_same);
        assert_ne!(ipv4, ipv6);
    }

    #[test]
    fn backend_state_transitions_follow_go_rules() {
        let mut backend = backend(
            17,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 17)),
            BackendState::Active,
        );

        assert!(backend.transition_to(BackendState::Quarantined).is_ok());
        assert_eq!(backend.state, BackendState::Quarantined);

        assert!(backend.transition_to(BackendState::Active).is_ok());
        assert_eq!(backend.state, BackendState::Active);

        assert!(backend.transition_to(BackendState::Terminating).is_ok());
        assert_eq!(backend.state, BackendState::Terminating);

        let error = backend.transition_to(BackendState::Maintenance);
        assert_eq!(
            error,
            Err(LoadBalancerError::InvalidBackendStateTransition {
                from: BackendState::Terminating,
                to: BackendState::Maintenance,
            })
        );
    }

    #[test]
    fn diff_backends_detects_add_remove_and_update() {
        let desired = vec![
            backend(
                1,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                BackendState::Active,
            ),
            backend(
                2,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                BackendState::Quarantined,
            ),
        ];
        let actual = vec![
            backend(
                2,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
                BackendState::Active,
            ),
            backend(
                3,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)),
                BackendState::Active,
            ),
        ];

        let diff = diff_backends(&desired, &actual);

        assert_eq!(diff.added, vec![desired[0].clone()]);
        assert_eq!(diff.removed, vec![actual[1].clone()]);
        assert_eq!(diff.updated, vec![desired[1].clone()]);
    }

    #[test]
    fn service_id_hash_and_equality_are_stable() {
        let first = ServiceID(42);
        let same = ServiceID(42);
        let different = ServiceID(7);

        assert_eq!(first, same);
        assert_ne!(first, different);

        let mut set = HashSet::new();
        set.insert(first);
        set.insert(same);
        set.insert(different);
        assert_eq!(set.len(), 2);

        let mut first_hasher = DefaultHasher::new();
        first.hash(&mut first_hasher);
        let mut same_hasher = DefaultHasher::new();
        same.hash(&mut same_hasher);
        assert_eq!(first_hasher.finish(), same_hasher.finish());
    }

    #[test]
    fn svc_helpers_operate_on_backend_ids() {
        let frontend = L3n4AddrID::new(
            L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 1)), 80, L4Type::TCP),
            ServiceID(100),
        );
        let mut svc = SVC::new(frontend);
        svc.backends = vec![
            backend(
                10,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 10)),
                BackendState::Active,
            ),
            backend(
                11,
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 11)),
                BackendState::Maintenance,
            ),
        ];

        assert_eq!(svc.service_id(), ServiceID(100));
        assert_eq!(svc.active_backends().len(), 1);
        assert!(svc.has_backend(BackendID(10)));
        assert!(!svc.has_backend(BackendID(99)));
        assert!(
            svc.update_backend_state(BackendID(11), BackendState::Active)
                .is_ok()
        );
        assert_eq!(svc.active_backends().len(), 2);
    }
}
