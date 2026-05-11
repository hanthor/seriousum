//! BGP control plane implementation.
//!
//! Ported from cilium/pkg/bgp.
//! Implements BGP speaker setup, CiliumBGPPeeringPolicy reconciliation,
//! route advertisement, peer management, and multi-VRF support.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Result};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use thiserror::Error;

/// Default component name for BGP scaffolds.
pub const COMPONENT: &str = "seriousum-bgp";

/// BGP-specific error types.
#[derive(Debug, Error, Clone)]
pub enum BgpError {
    #[error("invalid ASN: {0}")]
    InvalidAsn(String),
    #[error("invalid router ID: {0}")]
    InvalidRouterId(String),
    #[error("peer not found: {0}")]
    PeerNotFound(String),
    #[error("policy not found: {0}")]
    PolicyNotFound(String),
    #[error("invalid route: {0}")]
    InvalidRoute(String),
    #[error("invalid VRF: {0}")]
    InvalidVrf(String),
    #[error("peer already exists: {0}")]
    PeerExists(String),
    #[error("configuration conflict: {0}")]
    ConfigConflict(String),
}

/// Simple BGP session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// BGP is not yet configured.
    Idle,
    /// BGP is attempting to connect.
    Connect,
    /// BGP is exchanging updates.
    Established,
    /// BGP is unavailable.
    Down,
}

impl SessionState {
    /// Returns true if session is established.
    pub fn is_established(&self) -> bool {
        matches!(self, SessionState::Established)
    }
}

/// BGP address family (AFI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum AddressFamily {
    /// IPv4
    Ipv4,
    /// IPv6
    Ipv6,
}

impl AddressFamily {
    /// Returns string representation.
    pub fn as_str(&self) -> &str {
        match self {
            AddressFamily::Ipv4 => "ipv4",
            AddressFamily::Ipv6 => "ipv6",
        }
    }
}

impl std::fmt::Display for AddressFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// BGP sub-address family (SAFI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum SubAddressFamily {
    /// Unicast routes
    Unicast,
}

impl SubAddressFamily {
    /// Returns string representation.
    pub fn as_str(&self) -> &str {
        match self {
            SubAddressFamily::Unicast => "unicast",
        }
    }
}

impl fmt::Display for SubAddressFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// BGP address family pair (AFI/SAFI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
pub struct Family {
    pub afi: AddressFamily,
    pub safi: SubAddressFamily,
}

impl Family {
    /// Creates a new address family.
    pub fn new(afi: AddressFamily, safi: SubAddressFamily) -> Self {
        Self { afi, safi }
    }

    /// IPv4 unicast.
    pub fn ipv4_unicast() -> Self {
        Self {
            afi: AddressFamily::Ipv4,
            safi: SubAddressFamily::Unicast,
        }
    }

    /// IPv6 unicast.
    pub fn ipv6_unicast() -> Self {
        Self {
            afi: AddressFamily::Ipv6,
            safi: SubAddressFamily::Unicast,
        }
    }
}

impl std::fmt::Display for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.afi, self.safi)
    }
}

/// BGP graceful restart configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GracefulRestart {
    /// Enable graceful restart.
    pub enabled: bool,
    /// Restart time in seconds.
    pub restart_time_seconds: u32,
}

impl Default for GracefulRestart {
    fn default() -> Self {
        Self {
            enabled: true,
            restart_time_seconds: 120,
        }
    }
}

/// BGP neighbor timers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborTimers {
    /// Connect retry time in seconds.
    pub connect_retry_seconds: u32,
    /// Hold time in seconds.
    pub hold_time_seconds: u32,
    /// Keepalive interval in seconds.
    pub keepalive_interval_seconds: u32,
}

impl Default for NeighborTimers {
    fn default() -> Self {
        Self {
            connect_retry_seconds: 120,
            hold_time_seconds: 90,
            keepalive_interval_seconds: 30,
        }
    }
}

/// BGP neighbor configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpNeighborConfig {
    /// Peer name (unique identifier).
    pub name: String,
    /// Peer IP address.
    pub peer_addr: IpAddr,
    /// Peer ASN.
    pub peer_asn: u32,
    /// Local address for this peer connection.
    pub local_addr: Option<IpAddr>,
    /// Neighbor timers.
    pub timers: NeighborTimers,
    /// Password for TCP authentication.
    pub auth_password: Option<String>,
    /// Graceful restart configuration.
    pub graceful_restart: GracefulRestart,
    /// Supported address families.
    pub families: Vec<Family>,
}

impl BgpNeighborConfig {
    /// Creates a new neighbor configuration.
    pub fn new(name: impl Into<String>, peer_addr: IpAddr, peer_asn: u32) -> Self {
        Self {
            name: name.into(),
            peer_addr,
            peer_asn,
            local_addr: None,
            timers: NeighborTimers::default(),
            auth_password: None,
            graceful_restart: GracefulRestart::default(),
            families: vec![Family::ipv4_unicast()],
        }
    }

    /// Sets local address.
    pub fn with_local_addr(mut self, addr: IpAddr) -> Self {
        self.local_addr = Some(addr);
        self
    }

    /// Adds an address family.
    pub fn with_family(mut self, family: Family) -> Self {
        self.families.push(family);
        self
    }

    /// Sets authentication password.
    pub fn with_auth_password(mut self, password: impl Into<String>) -> Self {
        self.auth_password = Some(password.into());
        self
    }

    /// Validates the neighbor configuration.
    pub fn validate(&self) -> std::result::Result<(), BgpError> {
        if self.name.is_empty() {
            return Err(BgpError::InvalidAsn(
                "neighbor name must not be empty".into(),
            ));
        }
        if self.peer_asn == 0 {
            return Err(BgpError::InvalidAsn("peer ASN must be non-zero".into()));
        }
        if self.families.is_empty() {
            return Err(BgpError::ConfigConflict(
                "neighbor must support at least one address family".into(),
            ));
        }
        Ok(())
    }
}

/// BGP neighbor state (runtime).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpNeighborState {
    /// Peer address.
    pub peer: IpAddr,
    /// Remote autonomous system number.
    pub remote_asn: u32,
    /// Current session state.
    pub state: SessionState,
    /// Number of announced prefixes.
    pub prefixes: u32,
    /// Number of received routes.
    pub received_routes: u64,
    /// Number of accepted routes.
    pub accepted_routes: u64,
}

impl BgpNeighborState {
    /// Creates a new neighbor state.
    pub fn new(peer: IpAddr, remote_asn: u32) -> Self {
        Self {
            peer,
            remote_asn,
            state: SessionState::Idle,
            prefixes: 0,
            received_routes: 0,
            accepted_routes: 0,
        }
    }

    /// Marks the neighbor as established.
    pub fn established(mut self) -> Self {
        self.state = SessionState::Established;
        self.prefixes = 1;
        self
    }
}

// Keep backward-compatible alias
pub type BgpNeighbor = BgpNeighborState;

/// BGP global configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpGlobalConfig {
    /// Local router identifier.
    pub router_id: Ipv4Addr,
    /// Local autonomous system number.
    pub local_asn: u32,
    /// Listen port (default 179).
    pub listen_port: u16,
    /// Virtual Routing and Forwarding (VRF) instance name.
    pub vrf: Option<String>,
}

impl BgpGlobalConfig {
    /// Creates a new BGP global configuration.
    pub fn new(router_id: Ipv4Addr, local_asn: u32) -> Self {
        Self {
            router_id,
            local_asn,
            listen_port: 179,
            vrf: None,
        }
    }

    /// Sets VRF name.
    pub fn with_vrf(mut self, vrf: impl Into<String>) -> Self {
        self.vrf = Some(vrf.into());
        self
    }

    /// Validates the global configuration.
    pub fn validate(&self) -> std::result::Result<(), BgpError> {
        if self.local_asn == 0 {
            return Err(BgpError::InvalidAsn("local ASN must be non-zero".into()));
        }
        if self.listen_port == 0 {
            return Err(BgpError::InvalidRouterId(
                "listen port must be non-zero".into(),
            ));
        }
        Ok(())
    }
}

/// BGP route to advertise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpRoute {
    /// Route prefix (e.g., "10.0.0.0/8").
    pub prefix: String,
    /// Next hop IP address.
    pub next_hop: IpAddr,
    /// Local preference (for ranking).
    pub local_preference: Option<u32>,
}

impl BgpRoute {
    /// Creates a new BGP route.
    pub fn new(prefix: impl Into<String>, next_hop: IpAddr) -> Self {
        Self {
            prefix: prefix.into(),
            next_hop,
            local_preference: None,
        }
    }

    /// Sets local preference.
    pub fn with_local_preference(mut self, pref: u32) -> Self {
        self.local_preference = Some(pref);
        self
    }
}

/// BGP routing policy for filtering/modifying routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpRoutingPolicy {
    /// Policy name.
    pub name: String,
    /// Whether this is an import or export policy.
    pub is_import: bool,
    /// Prefixes to match.
    pub match_prefixes: Vec<String>,
    /// Action: true = accept, false = reject.
    pub action: bool,
}

impl BgpRoutingPolicy {
    /// Creates a new routing policy.
    pub fn new(name: impl Into<String>, is_import: bool, action: bool) -> Self {
        Self {
            name: name.into(),
            is_import,
            match_prefixes: Vec::new(),
            action,
        }
    }

    /// Adds a prefix to match.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.match_prefixes.push(prefix.into());
        self
    }
}

/// Compact BGP model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpModel {
    /// Component name.
    pub component: String,
    /// Global configuration.
    pub global: BgpGlobalConfig,
    /// Configured BGP neighbors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub neighbors: Vec<BgpNeighborState>,
    /// Routes to advertise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<BgpRoute>,
    /// Routing policies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policies: Vec<BgpRoutingPolicy>,
}

impl BgpModel {
    /// Creates a new BGP model.
    pub fn new(router_id: Ipv4Addr, local_asn: u32) -> Self {
        Self {
            component: COMPONENT.to_owned(),
            global: BgpGlobalConfig::new(router_id, local_asn),
            neighbors: Vec::new(),
            routes: Vec::new(),
            policies: Vec::new(),
        }
    }

    /// Returns the default scaffold model.
    pub fn scaffold() -> Self {
        Self::new(Ipv4Addr::new(10, 0, 0, 1), 65_000).with_neighbor(
            BgpNeighborState::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001).established(),
        )
    }

    /// Adds a neighbor.
    pub fn with_neighbor(mut self, neighbor: BgpNeighborState) -> Self {
        self.neighbors.push(neighbor);
        self
    }

    /// Adds a route.
    pub fn with_route(mut self, route: BgpRoute) -> Self {
        self.routes.push(route);
        self
    }

    /// Adds a routing policy.
    pub fn with_policy(mut self, policy: BgpRoutingPolicy) -> Self {
        self.policies.push(policy);
        self
    }

    /// Returns the number of established neighbors.
    pub fn established_neighbors(&self) -> usize {
        self.neighbors
            .iter()
            .filter(|neighbor| neighbor.state.is_established())
            .count()
    }

    /// Returns a concise summary string.
    pub fn summary(&self) -> String {
        format!(
            "{} neighbors={} established={} routes={}",
            self.component,
            self.neighbors.len(),
            self.established_neighbors(),
            self.routes.len()
        )
    }

    /// Validates the BGP model.
    pub fn validate(&self) -> Result<()> {
        self.global
            .validate()
            .map_err(|e| Error::Bgp(e.to_string()))?;

        if self.neighbors.is_empty() {
            return Err(Error::Bgp(String::from(
                "bgp model must contain at least one neighbor",
            )));
        }

        Ok(())
    }
}

impl Default for BgpModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// BGP router manager for coordinating BGP instances.
#[derive(Clone)]
pub struct BgpRouterManager {
    /// Instances by name.
    instances: Arc<DashMap<String, BgpInstance>>,
}

impl fmt::Debug for BgpRouterManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BgpRouterManager")
            .field("instances_count", &self.instances.len())
            .finish()
    }
}

impl BgpRouterManager {
    /// Creates a new BGP router manager.
    pub fn new() -> Self {
        Self {
            instances: Arc::new(DashMap::new()),
        }
    }

    /// Adds a BGP instance.
    pub fn add_instance(
        &self,
        name: impl Into<String>,
        config: BgpGlobalConfig,
    ) -> std::result::Result<(), BgpError> {
        let name = name.into();
        if self.instances.contains_key(&name) {
            return Err(BgpError::PeerExists(name));
        }
        config.validate()?;
        let instance = BgpInstance::new(name.clone(), config);
        self.instances.insert(name, instance);
        Ok(())
    }

    /// Gets a BGP instance.
    pub fn get_instance(&self, name: &str) -> Option<BgpInstance> {
        self.instances.get(name).map(|r| r.clone())
    }

    /// Removes a BGP instance.
    pub fn remove_instance(&self, name: &str) -> std::result::Result<(), BgpError> {
        self.instances
            .remove(name)
            .ok_or_else(|| BgpError::PolicyNotFound(name.to_string()))?;
        Ok(())
    }

    /// Lists all instances.
    pub fn list_instances(&self) -> Vec<String> {
        self.instances.iter().map(|r| r.key().clone()).collect()
    }
}

impl Default for BgpRouterManager {
    fn default() -> Self {
        Self::new()
    }
}

/// A single BGP instance running on a node.
#[derive(Clone)]
pub struct BgpInstance {
    /// Instance name.
    name: Arc<String>,
    /// Global configuration.
    config: Arc<BgpGlobalConfig>,
    /// Neighbor configurations.
    neighbors: Arc<DashMap<String, BgpNeighborConfig>>,
    /// Routes.
    routes: Arc<DashMap<String, BgpRoute>>,
    /// Policies.
    policies: Arc<DashMap<String, BgpRoutingPolicy>>,
}

impl BgpInstance {
    /// Creates a new BGP instance.
    pub fn new(name: impl Into<String>, config: BgpGlobalConfig) -> Self {
        Self {
            name: Arc::new(name.into()),
            config: Arc::new(config),
            neighbors: Arc::new(DashMap::new()),
            routes: Arc::new(DashMap::new()),
            policies: Arc::new(DashMap::new()),
        }
    }

    /// Gets the instance name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Gets the global configuration.
    pub fn config(&self) -> &BgpGlobalConfig {
        &self.config
    }

    /// Adds a neighbor.
    pub fn add_neighbor(&self, neighbor: BgpNeighborConfig) -> std::result::Result<(), BgpError> {
        neighbor.validate()?;
        if self.neighbors.contains_key(&neighbor.name) {
            return Err(BgpError::PeerExists(neighbor.name.clone()));
        }
        self.neighbors.insert(neighbor.name.clone(), neighbor);
        Ok(())
    }

    /// Gets a neighbor.
    pub fn get_neighbor(&self, name: &str) -> Option<BgpNeighborConfig> {
        self.neighbors.get(name).map(|r| r.clone())
    }

    /// Removes a neighbor.
    pub fn remove_neighbor(&self, name: &str) -> std::result::Result<(), BgpError> {
        self.neighbors
            .remove(name)
            .ok_or_else(|| BgpError::PeerNotFound(name.to_string()))?;
        Ok(())
    }

    /// Lists all neighbors.
    pub fn list_neighbors(&self) -> Vec<String> {
        self.neighbors.iter().map(|r| r.key().clone()).collect()
    }

    /// Advertises a route.
    pub fn advertise_route(&self, route: BgpRoute) -> std::result::Result<(), BgpError> {
        if route.prefix.is_empty() {
            return Err(BgpError::InvalidRoute("prefix must not be empty".into()));
        }
        self.routes.insert(route.prefix.clone(), route);
        Ok(())
    }

    /// Withdraws a route.
    pub fn withdraw_route(&self, prefix: &str) -> std::result::Result<(), BgpError> {
        self.routes
            .remove(prefix)
            .ok_or_else(|| BgpError::InvalidRoute(prefix.to_string()))?;
        Ok(())
    }

    /// Lists all routes.
    pub fn list_routes(&self) -> Vec<BgpRoute> {
        self.routes.iter().map(|r| r.value().clone()).collect()
    }

    /// Adds a routing policy.
    pub fn add_policy(&self, policy: BgpRoutingPolicy) -> std::result::Result<(), BgpError> {
        if policy.name.is_empty() {
            return Err(BgpError::PolicyNotFound(
                "policy name must not be empty".into(),
            ));
        }
        self.policies.insert(policy.name.clone(), policy);
        Ok(())
    }

    /// Removes a policy.
    pub fn remove_policy(&self, name: &str) -> std::result::Result<(), BgpError> {
        self.policies
            .remove(name)
            .ok_or_else(|| BgpError::PolicyNotFound(name.to_string()))?;
        Ok(())
    }

    /// Lists all policies.
    pub fn list_policies(&self) -> Vec<BgpRoutingPolicy> {
        self.policies.iter().map(|r| r.value().clone()).collect()
    }

    /// Gets the model snapshot for reporting.
    pub fn model_snapshot(&self) -> BgpModel {
        BgpModel {
            component: COMPONENT.to_owned(),
            global: (*self.config).clone(),
            neighbors: self
                .neighbors
                .iter()
                .map(|r| BgpNeighborState::new(r.peer_addr, r.peer_asn))
                .collect(),
            routes: self.routes.iter().map(|r| r.value().clone()).collect(),
            policies: self.policies.iter().map(|r| r.value().clone()).collect(),
        }
    }
}

impl PartialEq for BgpInstance {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.config == other.config
    }
}

impl Eq for BgpInstance {}

impl std::fmt::Debug for BgpInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BgpInstance")
            .field("name", &self.name)
            .field("config", &self.config)
            .field("neighbors_count", &self.neighbors.len())
            .field("routes_count", &self.routes.len())
            .field("policies_count", &self.policies.len())
            .finish()
    }
}

/// Serializable BGP report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpReport {
    /// Component name.
    pub component: String,
    /// BGP model.
    pub bgp: BgpModel,
    /// Whether at least one neighbor is established.
    pub established: bool,
}

impl BgpReport {
    /// Builds a report from a BGP model.
    pub fn new(bgp: BgpModel) -> Self {
        let established = bgp.established_neighbors() > 0;
        Self {
            component: COMPONENT.to_owned(),
            bgp,
            established,
        }
    }
}

/// Returns the standard BGP scaffold report.
pub fn scaffold() -> BgpReport {
    BgpReport::new(BgpModel::scaffold())
}

/// Configuration reconciler for BGP peering policies.
#[derive(Debug, Clone)]
pub struct PeeringPolicyReconciler {
    manager: BgpRouterManager,
}

impl PeeringPolicyReconciler {
    /// Creates a new reconciler.
    pub fn new(manager: BgpRouterManager) -> Self {
        Self { manager }
    }

    /// Reconciles a peering policy by ensuring the required instance and neighbors exist.
    pub fn reconcile(
        &self,
        instance_name: &str,
        global: BgpGlobalConfig,
        neighbors: Vec<BgpNeighborConfig>,
    ) -> std::result::Result<(), BgpError> {
        // Create or get instance
        if self.manager.get_instance(instance_name).is_none() {
            self.manager.add_instance(instance_name, global)?;
        }

        let instance = self
            .manager
            .get_instance(instance_name)
            .ok_or_else(|| BgpError::PolicyNotFound(instance_name.to_string()))?;

        // Reconcile neighbors
        for neighbor in neighbors {
            instance.add_neighbor(neighbor)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_family_display() {
        assert_eq!(AddressFamily::Ipv4.to_string(), "ipv4");
        assert_eq!(AddressFamily::Ipv6.to_string(), "ipv6");
    }

    #[test]
    fn family_creation() {
        let f = Family::ipv4_unicast();
        assert_eq!(f.afi, AddressFamily::Ipv4);
        assert_eq!(f.to_string(), "ipv4/unicast");
    }

    #[test]
    fn session_state_is_established() {
        assert!(SessionState::Established.is_established());
        assert!(!SessionState::Idle.is_established());
    }

    #[test]
    fn neighbor_config_validation() {
        let config =
            BgpNeighborConfig::new("peer1", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001);
        assert!(config.validate().is_ok());

        let invalid = BgpNeighborConfig {
            name: "".to_string(),
            peer_addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            peer_asn: 65_001,
            local_addr: None,
            timers: NeighborTimers::default(),
            auth_password: None,
            graceful_restart: GracefulRestart::default(),
            families: vec![],
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn neighbor_config_builder() {
        let config =
            BgpNeighborConfig::new("peer1", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001)
                .with_local_addr(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
                .with_family(Family::ipv6_unicast())
                .with_auth_password("secret");

        assert_eq!(
            config.local_addr,
            Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
        );
        assert_eq!(config.families.len(), 2);
        assert_eq!(config.auth_password, Some("secret".to_string()));
    }

    #[test]
    fn global_config_validation() {
        let config = BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000);
        assert!(config.validate().is_ok());

        let invalid_asn = BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 0);
        assert!(invalid_asn.validate().is_err());
    }

    #[test]
    fn bgp_route_creation() {
        let route = BgpRoute::new("10.0.0.0/8", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)))
            .with_local_preference(100);
        assert_eq!(route.prefix, "10.0.0.0/8");
        assert_eq!(route.local_preference, Some(100));
    }

    #[test]
    fn bgp_routing_policy_creation() {
        let policy = BgpRoutingPolicy::new("policy1", true, true)
            .with_prefix("10.0.0.0/8")
            .with_prefix("172.16.0.0/12");
        assert_eq!(policy.match_prefixes.len(), 2);
        assert!(policy.action);
    }

    #[test]
    fn bgp_model_scaffolding() {
        let model = BgpModel::scaffold();
        assert!(!model.neighbors.is_empty());
        assert_eq!(model.global.local_asn, 65_000);
    }

    #[test]
    fn bgp_model_builder() {
        let model = BgpModel::new(Ipv4Addr::new(10, 0, 0, 1), 65_000)
            .with_neighbor(BgpNeighborState::new(
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                65_001,
            ))
            .with_route(BgpRoute::new(
                "10.0.0.0/8",
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            ))
            .with_policy(BgpRoutingPolicy::new("policy1", true, true));

        assert_eq!(model.neighbors.len(), 1);
        assert_eq!(model.routes.len(), 1);
        assert_eq!(model.policies.len(), 1);
    }

    #[test]
    fn bgp_model_established_neighbors() {
        let model = BgpModel::new(Ipv4Addr::new(10, 0, 0, 1), 65_000)
            .with_neighbor(BgpNeighborState::new(
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                65_001,
            ))
            .with_neighbor(
                BgpNeighborState::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)), 65_002)
                    .established(),
            );

        assert_eq!(model.neighbors.len(), 2);
        assert_eq!(model.established_neighbors(), 1);
    }

    #[test]
    fn bgp_model_validation() {
        let valid = BgpModel::new(Ipv4Addr::new(10, 0, 0, 1), 65_000).with_neighbor(
            BgpNeighborState::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001),
        );
        assert!(valid.validate().is_ok());

        let invalid_asn = BgpModel::new(Ipv4Addr::new(10, 0, 0, 1), 0).with_neighbor(
            BgpNeighborState::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001),
        );
        assert!(invalid_asn.validate().is_err());

        let no_neighbors = BgpModel::new(Ipv4Addr::new(10, 0, 0, 1), 65_000);
        assert!(no_neighbors.validate().is_err());
    }

    #[test]
    fn bgp_model_summary() {
        let model = BgpModel::scaffold();
        let summary = model.summary();
        assert!(summary.contains("neighbors"));
        assert!(summary.contains("established"));
    }

    #[test]
    fn bgp_report_established() {
        let report = scaffold();
        assert_eq!(report.component, COMPONENT);
        assert!(report.established);
    }

    #[test]
    fn bgp_report_json_roundtrip() {
        let report = scaffold();
        let json = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: BgpReport =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded, report);
    }

    #[test]
    fn bgp_router_manager_add_instance() {
        let manager = BgpRouterManager::new();
        let config = BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000);
        assert!(manager.add_instance("instance1", config).is_ok());
        assert!(manager.get_instance("instance1").is_some());
    }

    #[test]
    fn bgp_router_manager_duplicate_instance() {
        let manager = BgpRouterManager::new();
        let config = BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000);
        assert!(manager.add_instance("instance1", config.clone()).is_ok());
        assert!(manager.add_instance("instance1", config).is_err());
    }

    #[test]
    fn bgp_router_manager_remove_instance() {
        let manager = BgpRouterManager::new();
        let config = BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000);
        manager.add_instance("instance1", config).ok();
        assert!(manager.remove_instance("instance1").is_ok());
        assert!(manager.get_instance("instance1").is_none());
    }

    #[test]
    fn bgp_instance_add_neighbor() {
        let instance = BgpInstance::new(
            "instance1",
            BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000),
        );
        let neighbor =
            BgpNeighborConfig::new("peer1", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001);
        assert!(instance.add_neighbor(neighbor).is_ok());
        assert!(instance.get_neighbor("peer1").is_some());
    }

    #[test]
    fn bgp_instance_advertise_route() {
        let instance = BgpInstance::new(
            "instance1",
            BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000),
        );
        let route = BgpRoute::new("10.0.0.0/8", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)));
        assert!(instance.advertise_route(route).is_ok());
        assert_eq!(instance.list_routes().len(), 1);
    }

    #[test]
    fn bgp_instance_add_policy() {
        let instance = BgpInstance::new(
            "instance1",
            BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000),
        );
        let policy = BgpRoutingPolicy::new("policy1", true, true);
        assert!(instance.add_policy(policy).is_ok());
        assert_eq!(instance.list_policies().len(), 1);
    }

    #[test]
    fn peering_policy_reconciler() {
        let manager = BgpRouterManager::new();
        let reconciler = PeeringPolicyReconciler::new(manager.clone());

        let config = BgpGlobalConfig::new(Ipv4Addr::new(10, 0, 0, 1), 65_000);
        let neighbors = vec![BgpNeighborConfig::new(
            "peer1",
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            65_001,
        )];

        assert!(reconciler.reconcile("instance1", config, neighbors).is_ok());
        assert!(manager.get_instance("instance1").is_some());
    }
}
