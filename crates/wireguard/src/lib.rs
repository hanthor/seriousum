//! Lightweight wireguard scaffolds for parity-friendly model work.

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use seriousum_core::{
    Error, IpNetwork, Port, Result,
    chrono::{DateTime, Utc},
};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
};

/// Default component name for wireguard scaffolds.
pub const COMPONENT: &str = "seriousum-wireguard";

/// Wireguard lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireGuardState {
    /// The tunnel is down.
    Down,
    /// The tunnel is negotiating peers.
    Handshaking,
    /// The tunnel is ready.
    Ready,
}

/// Underlay protocol used for peer endpoint selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnderlayProtocol {
    /// Automatic underlay selection.
    Auto,
    /// Prefer IPv4 underlay.
    Ipv4,
    /// Prefer IPv6 underlay.
    Ipv6,
}

/// Errors produced by pure WireGuard data model helpers.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WireguardError {
    /// The provided key byte length is invalid.
    #[error("invalid key length: expected 32, got {0}")]
    InvalidKeyLength(usize),
    /// A requested peer does not exist in the registry.
    #[error("peer not found")]
    PeerNotFound,
    /// A generic device-scoped WireGuard error.
    #[error("device error: {0}")]
    Device(String),
}

/// A 32-byte WireGuard public or private key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WGKey([u8; 32]);

impl WGKey {
    /// Builds a key from a raw 32-byte slice.
    pub fn from_bytes(b: &[u8]) -> std::result::Result<Self, WireguardError> {
        b.try_into()
            .map(Self)
            .map_err(|_| WireguardError::InvalidKeyLength(b.len()))
    }

    /// Returns the raw key bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns whether the key is all zeroes.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }
}

impl std::fmt::Display for WGKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for WGKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let encoded = self.to_string();
        write!(f, "WGKey({}...)", &encoded[..8])
    }
}

/// A WireGuard peer in the overlay mesh.
#[derive(Debug, Clone)]
pub struct WGPeer {
    /// Remote peer public key.
    pub public_key: WGKey,
    /// Optional remote endpoint socket address.
    pub endpoint: Option<SocketAddr>,
    /// Networks routed to the peer.
    pub allowed_ips: Vec<IpNet>,
    /// Persistent keepalive interval in seconds. Zero disables keepalive.
    pub persistent_keepalive: u16,
}

impl WGPeer {
    /// Creates a peer with no endpoint or allowed IPs configured.
    #[must_use]
    pub fn new(public_key: WGKey) -> Self {
        Self {
            public_key,
            endpoint: None,
            allowed_ips: vec![],
            persistent_keepalive: 0,
        }
    }

    /// Sets the peer endpoint.
    #[must_use]
    pub fn with_endpoint(mut self, ep: SocketAddr) -> Self {
        self.endpoint = Some(ep);
        self
    }

    /// Adds an allowed IP CIDR if it is not already present.
    pub fn add_allowed_ip(&mut self, cidr: IpNet) {
        if !self.allowed_ips.contains(&cidr) {
            self.allowed_ips.push(cidr);
        }
    }

    /// Removes an allowed IP CIDR from the peer.
    pub fn remove_allowed_ip(&mut self, cidr: &IpNet) {
        self.allowed_ips.retain(|candidate| candidate != cidr);
    }
}

/// Configuration for the local WireGuard interface.
#[derive(Debug, Clone)]
pub struct WGDeviceConfig {
    /// Name of the WireGuard network interface.
    pub interface_name: String,
    /// Local private key.
    pub private_key: WGKey,
    /// UDP listen port for the interface.
    pub listen_port: u16,
    /// Device MTU.
    pub mtu: u32,
}

impl WGDeviceConfig {
    /// Creates a new local WireGuard device configuration.
    #[must_use]
    pub fn new(iface: impl Into<String>, private_key: WGKey, port: u16) -> Self {
        Self {
            interface_name: iface.into(),
            private_key,
            listen_port: port,
            mtu: 1_420,
        }
    }
}

/// Tracks configured WireGuard peers without touching the kernel.
#[derive(Debug, Default)]
pub struct WGPeerRegistry {
    peers: HashMap<WGKey, WGPeer>,
}

impl WGPeerRegistry {
    /// Creates an empty peer registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a peer keyed by its public key.
    pub fn add_or_update(&mut self, peer: WGPeer) {
        self.peers.insert(peer.public_key.clone(), peer);
    }

    /// Removes a peer from the registry.
    pub fn remove(&mut self, key: &WGKey) -> Option<WGPeer> {
        self.peers.remove(key)
    }

    /// Gets a peer by public key.
    #[must_use]
    pub fn get(&self, key: &WGKey) -> Option<&WGPeer> {
        self.peers.get(key)
    }

    /// Returns the number of configured peers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Returns whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Finds all peers whose allowed IPs contain the given address.
    #[must_use]
    pub fn peers_for_ip(&self, ip: IpAddr) -> Vec<&WGPeer> {
        self.peers
            .values()
            .filter(|peer| peer.allowed_ips.iter().any(|cidr| cidr.contains(&ip)))
            .collect()
    }
}

/// Peer endpoint selection configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerEndpointSelection {
    /// Whether IPv4 is enabled in the cluster.
    pub enable_ipv4: bool,
    /// Whether IPv6 is enabled in the cluster.
    pub enable_ipv6: bool,
    /// Whether tunneling mode is enabled.
    pub tunneling_enabled: bool,
    /// Underlay preference.
    pub underlay_protocol: UnderlayProtocol,
    /// WireGuard listen port for peer endpoints.
    pub listen_port: Port,
}

impl PeerEndpointSelection {
    /// Selects peer endpoint using Cilium's updatePeer endpoint preference rules.
    pub fn select_endpoint(
        &self,
        node_ipv4: Option<Ipv4Addr>,
        node_ipv6: Option<Ipv6Addr>,
    ) -> Result<SocketAddr> {
        let ip = if self.tunneling_enabled
            && self.underlay_protocol == UnderlayProtocol::Ipv6
            && self.enable_ipv6
        {
            node_ipv6.map(IpAddr::V6)
        } else {
            None
        }
        .or_else(|| {
            if self.enable_ipv4 {
                node_ipv4.map(IpAddr::V4)
            } else {
                None
            }
        })
        .or_else(|| {
            if self.enable_ipv6 {
                node_ipv6.map(IpAddr::V6)
            } else {
                None
            }
        })
        .ok_or_else(|| Error::Wireguard(String::from("missing node IP for peer endpoint")))?;

        Ok(SocketAddr::new(ip, self.listen_port.as_u16()))
    }
}

impl Default for PeerEndpointSelection {
    fn default() -> Self {
        Self {
            enable_ipv4: true,
            enable_ipv6: true,
            tunneling_enabled: true,
            underlay_protocol: UnderlayProtocol::Auto,
            listen_port: Port::new(51_871),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllowedIpsMode {
    Native,
    TunnelWithFallback,
    TunnelWithoutFallback,
}

#[cfg(test)]
impl AllowedIpsMode {
    const fn tracks_workload_ips(self) -> bool {
        !matches!(self, Self::TunnelWithoutFallback)
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct ReconciledPeer {
    public_key: WGKey,
    node_ipv4: Option<Ipv4Addr>,
    node_ipv6: Option<Ipv6Addr>,
    endpoint: SocketAddr,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum RestoredOwner {
    Node(String),
    Key(WGKey),
}

#[cfg(test)]
impl RestoredOwner {
    fn belongs_to(&self, node_name: &str) -> bool {
        matches!(self, Self::Node(owner) if owner == node_name)
    }
}

#[cfg(test)]
#[derive(Debug)]
struct WireGuardPeerReconciler {
    mode: AllowedIpsMode,
    endpoint_selection: PeerEndpointSelection,
    peers: HashMap<String, ReconciledPeer>,
    workload_owner: HashMap<IpNet, String>,
    restored_owner: HashMap<IpNet, RestoredOwner>,
    restore_finished: bool,
}

#[cfg(test)]
impl WireGuardPeerReconciler {
    fn new(mode: AllowedIpsMode, endpoint_selection: PeerEndpointSelection) -> Self {
        Self {
            mode,
            endpoint_selection,
            peers: HashMap::new(),
            workload_owner: HashMap::new(),
            restored_owner: HashMap::new(),
            restore_finished: false,
        }
    }

    fn seed_restored_peer<I>(&mut self, public_key: WGKey, allowed_ips: I)
    where
        I: IntoIterator<Item = IpNet>,
    {
        for allowed_ip in allowed_ips {
            self.restored_owner
                .insert(allowed_ip, RestoredOwner::Key(public_key.clone()));
        }
    }

    fn upsert_workload(&mut self, prefix: IpNet, node_name: impl Into<String>) {
        let node_name = node_name.into();
        self.workload_owner
            .insert(prefix.clone(), node_name.clone());
        if let Some(owner) = self.restored_owner.get_mut(&prefix) {
            *owner = RestoredOwner::Node(node_name);
        }
    }

    fn delete_workload(&mut self, prefix: &IpNet) {
        self.workload_owner.remove(prefix);
        self.restored_owner.remove(prefix);
    }

    fn update_peer(
        &mut self,
        node_name: impl Into<String>,
        public_key: WGKey,
        node_ipv4: Option<Ipv4Addr>,
        node_ipv6: Option<Ipv6Addr>,
    ) -> Result<()> {
        let node_name = node_name.into();
        if public_key.is_zero() {
            return Err(Error::Wireguard(format!(
                "node {node_name} is not allowed to use the dummy peer key"
            )));
        }
        if self
            .peers
            .iter()
            .any(|(other_name, peer)| other_name != &node_name && peer.public_key == public_key)
        {
            return Err(Error::Wireguard(format!(
                "detected duplicate public key for node {node_name}"
            )));
        }

        let endpoint = self.endpoint_selection.select_endpoint(node_ipv4, node_ipv6)?;
        self.peers.insert(
            node_name.clone(),
            ReconciledPeer {
                public_key: public_key.clone(),
                node_ipv4,
                node_ipv6,
                endpoint,
            },
        );

        for owner in self.restored_owner.values_mut() {
            if matches!(owner, RestoredOwner::Key(key) if *key == public_key) {
                *owner = RestoredOwner::Node(node_name.clone());
            }
        }

        Ok(())
    }

    fn delete_peer(&mut self, node_name: &str) {
        self.peers.remove(node_name);
        self.workload_owner.retain(|_, owner| owner != node_name);
        self.restored_owner
            .retain(|_, owner| !matches!(owner, RestoredOwner::Node(owner_name) if owner_name == node_name));
    }

    fn restore_finished(&mut self) {
        self.restore_finished = true;
    }

    fn peer(&self, node_name: &str) -> Option<&ReconciledPeer> {
        self.peers.get(node_name)
    }

    fn allowed_ips_by_node(&self, node_name: &str) -> Vec<IpNet> {
        let Some(peer) = self.peers.get(node_name) else {
            return Vec::new();
        };

        let mut allowed_ips = Vec::new();
        if let Some(node_ipv4) = peer.node_ipv4 {
            allowed_ips.push(host_prefix(IpAddr::V4(node_ipv4)));
        }
        if let Some(node_ipv6) = peer.node_ipv6 {
            allowed_ips.push(host_prefix(IpAddr::V6(node_ipv6)));
        }
        if self.mode.tracks_workload_ips() {
            allowed_ips.extend(
                self.workload_owner
                    .iter()
                    .filter(|(_, owner)| *owner == node_name)
                    .map(|(prefix, _)| prefix.clone()),
            );
        }
        if !self.restore_finished {
            allowed_ips.extend(
                self.restored_owner
                    .iter()
                    .filter(|(_, owner)| owner.belongs_to(node_name))
                    .map(|(prefix, _)| prefix.clone()),
            );
        }

        allowed_ips.sort_by_key(|prefix| prefix.to_string());
        allowed_ips.dedup();
        allowed_ips
    }

    fn allowed_ips_by_public_key(&self) -> HashMap<WGKey, Vec<IpNet>> {
        self.peers
            .iter()
            .map(|(node_name, peer)| (peer.public_key.clone(), self.allowed_ips_by_node(node_name)))
            .collect()
    }
}

#[cfg(test)]
fn host_prefix(ip: IpAddr) -> IpNet {
    let prefix_len = if ip.is_ipv4() { 32 } else { 128 };
    match IpNet::new(ip, prefix_len) {
        Ok(prefix) => prefix,
        Err(_) => unreachable!("host prefix lengths are always valid"),
    }
}

/// Compact wireguard configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardConfig {
    /// Interface name.
    pub interface: String,

    /// Local listen port.
    pub listen_port: Port,

    /// Tunnel address.
    pub tunnel_address: IpAddr,

    /// Allowed peer networks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_networks: Vec<IpNetwork>,

    /// MTU for the tunnel.
    pub mtu: u32,
}

impl WireGuardConfig {
    /// Creates a new wireguard configuration.
    #[must_use]
    pub fn new(interface: impl Into<String>, listen_port: Port, tunnel_address: IpAddr) -> Self {
        Self {
            interface: interface.into(),
            listen_port,
            tunnel_address,
            peer_networks: Vec::new(),
            mtu: 1_420,
        }
    }

    /// Returns the default scaffold configuration.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("wg0", Port::new(51_820), IpAddr::from([10, 0, 0, 1]))
            .with_peer_network(IpNetwork::from_str("10.0.0.0/24").expect("valid wireguard network"))
    }

    /// Adds an allowed peer network.
    #[must_use]
    pub fn with_peer_network(mut self, network: IpNetwork) -> Self {
        self.peer_networks.push(network);
        self
    }

    /// Returns the local socket-like representation.
    #[must_use]
    pub fn endpoint_string(&self) -> String {
        format!("{}:{}", self.tunnel_address, self.listen_port)
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.interface.trim().is_empty() {
            return Err(Error::Wireguard(String::from(
                "wireguard interface must not be empty",
            )));
        }

        if self.listen_port.as_u16() == 0 {
            return Err(Error::Wireguard(String::from(
                "wireguard listen port must not be zero",
            )));
        }

        if self.peer_networks.is_empty() {
            return Err(Error::Wireguard(String::from(
                "wireguard must allow at least one peer network",
            )));
        }

        if self.mtu < 1_280 {
            return Err(Error::Wireguard(String::from(
                "wireguard mtu must be at least 1280",
            )));
        }

        Ok(())
    }
}

impl Default for WireGuardConfig {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Wireguard session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardSession {
    /// Peer public key.
    pub peer_public_key: String,

    /// Last handshake time.
    pub last_handshake: Option<DateTime<Utc>>,

    /// Whether the session is active.
    pub active: bool,
}

impl WireGuardSession {
    /// Creates a new wireguard session.
    #[must_use]
    pub fn new(peer_public_key: impl Into<String>) -> Self {
        Self {
            peer_public_key: peer_public_key.into(),
            last_handshake: None,
            active: true,
        }
    }

    /// Returns the default scaffold session.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("scaffold-peer-key").mark_handshake()
    }

    /// Marks the session as having a handshake.
    #[must_use]
    pub fn mark_handshake(mut self) -> Self {
        self.last_handshake = Some(Utc::now());
        self
    }

    /// Marks the session inactive.
    #[must_use]
    pub fn deactivate(mut self) -> Self {
        self.active = false;
        self
    }

    /// Validates the session.
    pub fn validate(&self) -> Result<()> {
        if self.peer_public_key.trim().is_empty() {
            return Err(Error::Wireguard(String::from(
                "wireguard peer public key must not be empty",
            )));
        }

        Ok(())
    }
}

impl Default for WireGuardSession {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Compact wireguard model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardModel {
    /// Wireguard configuration.
    pub config: WireGuardConfig,

    /// Peer/session state.
    pub session: WireGuardSession,

    /// Lifecycle state.
    pub state: WireGuardState,
}

impl WireGuardModel {
    /// Creates a new wireguard model.
    #[must_use]
    pub fn new(config: WireGuardConfig, session: WireGuardSession) -> Self {
        Self {
            config,
            session,
            state: WireGuardState::Down,
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(WireGuardConfig::scaffold(), WireGuardSession::scaffold()).ready()
    }

    /// Marks the model ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.state = WireGuardState::Ready;
        self
    }

    /// Marks the model as handshaking.
    #[must_use]
    pub fn handshaking(mut self) -> Self {
        self.state = WireGuardState::Handshaking;
        self
    }

    /// Returns a stable summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "interface={} peers={} active={}",
            self.config.interface,
            self.config.peer_networks.len(),
            self.session.active
        )
    }

    /// Validates the model.
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        self.session.validate()?;
        Ok(())
    }
}

impl Default for WireGuardModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable wireguard report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireGuardReport {
    /// Component name.
    pub component: String,

    /// Wireguard model.
    pub wireguard: WireGuardModel,

    /// Whether the tunnel is connected.
    pub connected: bool,
}

impl WireGuardReport {
    /// Builds a report from a wireguard model.
    #[must_use]
    pub fn new(wireguard: WireGuardModel) -> Self {
        let connected =
            matches!(wireguard.state, WireGuardState::Ready) && wireguard.session.active;
        Self {
            component: COMPONENT.to_owned(),
            connected,
            wireguard,
        }
    }
}

/// Returns the standard wireguard scaffold report.
#[must_use]
pub fn scaffold() -> WireGuardReport {
    WireGuardReport::new(WireGuardModel::scaffold())
}

#[cfg(test)]
mod parity_tests {
    use super::{
        AllowedIpsMode, PeerEndpointSelection, UnderlayProtocol, WGKey, WireGuardPeerReconciler,
    };
    use ipnet::IpNet;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn peer_key(byte: u8) -> WGKey {
        WGKey::from_bytes(&[byte; 32]).expect("32-byte key should be accepted")
    }

    fn cidr(value: &str) -> IpNet {
        value.parse().expect("CIDR should parse")
    }

    fn assert_cidrs_eq(actual: Vec<IpNet>, expected: &[&str]) {
        let mut actual = actual
            .into_iter()
            .map(|prefix| prefix.to_string())
            .collect::<Vec<_>>();
        let mut expected = expected
            .iter()
            .map(|prefix| (*prefix).to_owned())
            .collect::<Vec<_>>();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }

    // Stub ported from pkg/wireguard/agent/cell_test.go.

    // Requires privileged Linux netns + WireGuard netlink device lifecycle;
    // ref TestPrivileged_TestWireGuardCell in pkg/wireguard/agent/cell_test.go.
    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_wireguard_cell() {}

    #[test]
    fn parity_test_agent_peer_config() {
        let node1_ipv4 = Ipv4Addr::new(192, 168, 60, 11);
        let node1_ipv6 = Ipv6Addr::new(0xfd01, 0, 0, 0, 0, 0, 0, 0x000b);
        let node2_ipv4 = Ipv4Addr::new(192, 168, 60, 12);
        let node2_ipv6 = Ipv6Addr::new(0xfd01, 0, 0, 0, 0, 0, 0, 0x000c);

        for (name, mode, first_expected, second_k8s1, second_k8s2) in [
            (
                "native",
                AllowedIpsMode::Native,
                vec![
                    "10.0.0.1/32",
                    "10.0.0.2/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd01::b/128",
                ],
                vec!["10.0.0.2/32", "192.168.60.11/32", "fd00::2/128", "fd01::b/128"],
                vec!["10.0.0.3/32", "192.168.60.12/32", "fd00::3/128", "fd01::c/128"],
            ),
            (
                "tunnel-with-fallback",
                AllowedIpsMode::TunnelWithFallback,
                vec![
                    "10.0.0.1/32",
                    "10.0.0.2/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd01::b/128",
                ],
                vec!["10.0.0.2/32", "192.168.60.11/32", "fd00::2/128", "fd01::b/128"],
                vec!["10.0.0.3/32", "192.168.60.12/32", "fd00::3/128", "fd01::c/128"],
            ),
            (
                "tunnel-without-fallback",
                AllowedIpsMode::TunnelWithoutFallback,
                vec!["192.168.60.11/32", "fd01::b/128"],
                vec!["192.168.60.11/32", "fd01::b/128"],
                vec!["192.168.60.12/32", "fd01::c/128"],
            ),
        ] {
            let mut reconciler = WireGuardPeerReconciler::new(mode, PeerEndpointSelection::default());
            reconciler.upsert_workload(cidr("10.0.0.1/32"), "k8s1");
            reconciler.upsert_workload(cidr("fd00::1/128"), "k8s1");
            reconciler.upsert_workload(cidr("10.0.0.2/32"), "k8s1");
            reconciler.upsert_workload(cidr("fd00::2/128"), "k8s1");

            reconciler
                .update_peer("k8s1", peer_key(1), Some(node1_ipv4), Some(node1_ipv6))
                .expect("first peer should reconcile");
            let peer = reconciler.peer("k8s1").expect("k8s1 should exist");
            assert_eq!(peer.node_ipv4, Some(node1_ipv4), "{name}");
            assert_eq!(peer.node_ipv6, Some(node1_ipv6), "{name}");
            assert_eq!(peer.endpoint.ip(), IpAddr::V4(node1_ipv4), "{name}");
            assert_cidrs_eq(reconciler.allowed_ips_by_node("k8s1"), &first_expected);

            reconciler.upsert_workload(cidr("10.0.0.3/32"), "k8s2");
            reconciler.upsert_workload(cidr("fd00::3/128"), "k8s2");
            reconciler.delete_workload(&cidr("10.0.0.1/32"));
            reconciler.delete_workload(&cidr("fd00::1/128"));
            reconciler
                .update_peer("k8s2", peer_key(2), Some(node2_ipv4), Some(node2_ipv6))
                .expect("second peer should reconcile");

            assert_cidrs_eq(reconciler.allowed_ips_by_node("k8s1"), &second_k8s1);
            assert_cidrs_eq(reconciler.allowed_ips_by_node("k8s2"), &second_k8s2);

            let error = reconciler
                .update_peer("k8s2", peer_key(1), Some(node2_ipv4), Some(node2_ipv6))
                .expect_err("duplicate key should fail");
            assert!(error.to_string().contains("duplicate public key"), "{name}");

            reconciler.delete_peer("k8s1");
            reconciler.delete_peer("k8s2");
            assert!(reconciler.peers.is_empty(), "{name}");
            assert!(reconciler.workload_owner.is_empty(), "{name}");
            assert!(reconciler.restored_owner.is_empty(), "{name}");
        }
    }

    #[test]
    fn parity_test_agent_allowed_ips_restoration() {
        let node1_ipv4 = Ipv4Addr::new(192, 168, 60, 11);
        let node1_ipv6 = Ipv6Addr::new(0xfd01, 0, 0, 0, 0, 0, 0, 0x000b);
        let node2_ipv4 = Ipv4Addr::new(192, 168, 60, 12);
        let node2_ipv6 = Ipv6Addr::new(0xfd01, 0, 0, 0, 0, 0, 0, 0x000c);
        let node2_ipv4_alt = Ipv4Addr::new(192, 168, 60, 13);

        for (name, mode, step1, step2, step3, step4, step5, step6, step7, step8) in [
            (
                "native",
                AllowedIpsMode::Native,
                vec![
                    "10.0.0.1/32",
                    "10.0.0.3/32",
                    "10.0.0.4/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd00::4/128",
                    "fd01::b/128",
                ],
                vec![
                    "10.0.0.2/32",
                    "10.0.0.3/32",
                    "10.0.0.4/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd00::4/128",
                    "fd01::b/128",
                ],
                vec![
                    "10.0.0.2/32",
                    "10.0.0.3/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd01::b/128",
                ],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
                vec!["10.0.0.2/32", "192.168.60.11/32", "fd00::1/128", "fd00::2/128", "fd01::b/128"],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
                vec!["10.0.0.4/32", "192.168.60.13/32", "fd00::4/128", "fd01::c/128"],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
            ),
            (
                "tunnel-with-fallback",
                AllowedIpsMode::TunnelWithFallback,
                vec![
                    "10.0.0.1/32",
                    "10.0.0.3/32",
                    "10.0.0.4/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd00::4/128",
                    "fd01::b/128",
                ],
                vec![
                    "10.0.0.2/32",
                    "10.0.0.3/32",
                    "10.0.0.4/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd00::4/128",
                    "fd01::b/128",
                ],
                vec![
                    "10.0.0.2/32",
                    "10.0.0.3/32",
                    "192.168.60.11/32",
                    "fd00::1/128",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd01::b/128",
                ],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
                vec!["10.0.0.2/32", "192.168.60.11/32", "fd00::1/128", "fd00::2/128", "fd01::b/128"],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
                vec!["10.0.0.4/32", "192.168.60.13/32", "fd00::4/128", "fd01::c/128"],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
            ),
            (
                "tunnel-without-fallback",
                AllowedIpsMode::TunnelWithoutFallback,
                vec![
                    "10.0.0.1/32",
                    "10.0.0.3/32",
                    "10.0.0.4/32",
                    "192.168.60.11/32",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd00::4/128",
                    "fd01::b/128",
                ],
                vec![
                    "10.0.0.3/32",
                    "10.0.0.4/32",
                    "192.168.60.11/32",
                    "fd00::2/128",
                    "fd00::3/128",
                    "fd00::4/128",
                    "fd01::b/128",
                ],
                vec!["10.0.0.3/32", "192.168.60.11/32", "fd00::2/128", "fd00::3/128", "fd01::b/128"],
                vec!["10.0.0.4/32", "192.168.60.12/32", "fd00::4/128", "fd01::c/128"],
                vec!["192.168.60.11/32", "fd01::b/128"],
                vec!["192.168.60.12/32", "fd01::c/128"],
                vec!["192.168.60.13/32", "fd01::c/128"],
                vec!["192.168.60.12/32", "fd01::c/128"],
            ),
        ] {
            let key1 = peer_key(1);
            let key2 = peer_key(2);
            let key3 = peer_key(3);
            let mut reconciler = WireGuardPeerReconciler::new(mode, PeerEndpointSelection::default());
            reconciler.seed_restored_peer(
                key1.clone(),
                [
                    cidr("10.0.0.1/32"),
                    cidr("10.0.0.3/32"),
                    cidr("10.0.0.4/32"),
                    cidr("fd00::2/128"),
                    cidr("fd00::3/128"),
                    cidr("fd00::4/128"),
                ],
            );

            reconciler.upsert_workload(cidr("10.0.0.1/32"), "k8s1");
            reconciler.upsert_workload(cidr("fd00::1/128"), "k8s1");
            reconciler
                .update_peer("k8s1", key1.clone(), Some(node1_ipv4), Some(node1_ipv6))
                .expect("initial peer restore should succeed");
            let mut by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step1);

            reconciler.upsert_workload(cidr("10.0.0.2/32"), "k8s1");
            reconciler.upsert_workload(cidr("fd00::2/128"), "k8s1");
            reconciler.delete_workload(&cidr("10.0.0.1/32"));
            by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step2);

            reconciler.upsert_workload(cidr("10.0.0.4/32"), "k8s2");
            reconciler
                .update_peer("k8s2", key2.clone(), Some(node2_ipv4), Some(node2_ipv6))
                .expect("second peer restore should succeed");
            reconciler.upsert_workload(cidr("fd00::4/128"), "k8s2");
            reconciler
                .update_peer("k8s1", key1.clone(), Some(node1_ipv4), Some(node1_ipv6))
                .expect("refreshing the first peer should succeed");
            by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step3);
            assert_cidrs_eq(by_key.remove(&key2).expect("key2 should exist"), &step4);

            reconciler.restore_finished();
            by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step5);
            assert_cidrs_eq(by_key.remove(&key2).expect("key2 should exist"), &step6);

            reconciler
                .update_peer("k8s2", key3.clone(), Some(node2_ipv4), Some(node2_ipv6))
                .expect("public key rotation should succeed");
            by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step5);
            assert_cidrs_eq(by_key.remove(&key3).expect("key3 should exist"), &step6);

            reconciler
                .update_peer("k8s2", key3.clone(), Some(node2_ipv4_alt), Some(node2_ipv6))
                .expect("node IP change should succeed");
            by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step5);
            assert_cidrs_eq(by_key.remove(&key3).expect("key3 should exist"), &step7);

            reconciler
                .update_peer("k8s2", key2.clone(), Some(node2_ipv4), Some(node2_ipv6))
                .expect("restoring the original key should succeed");
            by_key = reconciler.allowed_ips_by_public_key();
            assert_cidrs_eq(by_key.remove(&key1).expect("key1 should exist"), &step5);
            assert_cidrs_eq(by_key.remove(&key2).expect("key2 should exist"), &step8);

            let error = reconciler
                .update_peer("k8s2", peer_key(0), Some(node2_ipv4_alt), Some(node2_ipv6))
                .expect_err("dummy key should be rejected");
            assert!(error.to_string().contains("dummy peer key"), "{name}");
        }
    }

    #[test]
    fn parity_test_agent_peer_endpoint_selection() {
        let node_ipv4 = Ipv4Addr::new(10, 0, 0, 2);
        let node_ipv6 = Ipv6Addr::new(0xf00d, 0, 0, 0, 0, 0, 0, 2);

        let cases = [
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: true,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Ipv6,
                    ..PeerEndpointSelection::default()
                },
                Some(node_ipv4),
                Some(node_ipv6),
                Some((IpAddr::V6(node_ipv6), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: true,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Ipv4,
                    ..PeerEndpointSelection::default()
                },
                Some(node_ipv4),
                Some(node_ipv6),
                Some((IpAddr::V4(node_ipv4), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: true,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Auto,
                    ..PeerEndpointSelection::default()
                },
                Some(node_ipv4),
                Some(node_ipv6),
                Some((IpAddr::V4(node_ipv4), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: true,
                    tunneling_enabled: false,
                    underlay_protocol: UnderlayProtocol::Ipv6,
                    ..PeerEndpointSelection::default()
                },
                Some(node_ipv4),
                Some(node_ipv6),
                Some((IpAddr::V4(node_ipv4), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: false,
                    enable_ipv6: true,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Ipv6,
                    ..PeerEndpointSelection::default()
                },
                None,
                Some(node_ipv6),
                Some((IpAddr::V6(node_ipv6), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: true,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Ipv6,
                    ..PeerEndpointSelection::default()
                },
                Some(node_ipv4),
                None,
                Some((IpAddr::V4(node_ipv4), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: false,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Ipv4,
                    ..PeerEndpointSelection::default()
                },
                Some(node_ipv4),
                None,
                Some((IpAddr::V4(node_ipv4), 51_871)),
            ),
            (
                PeerEndpointSelection {
                    enable_ipv4: true,
                    enable_ipv6: true,
                    tunneling_enabled: true,
                    underlay_protocol: UnderlayProtocol::Ipv4,
                    ..PeerEndpointSelection::default()
                },
                None,
                None,
                None,
            ),
        ];

        for (selector, ipv4, ipv6, expected) in cases {
            match expected {
                Some((expected_ip, expected_port)) => {
                    let endpoint = selector
                        .select_endpoint(ipv4, ipv6)
                        .expect("endpoint selection should succeed");
                    assert_eq!(endpoint.ip(), expected_ip);
                    assert_eq!(endpoint.port(), expected_port);
                }
                None => {
                    let error = selector
                        .select_endpoint(ipv4, ipv6)
                        .expect_err("endpoint selection should fail");
                    assert!(matches!(error, seriousum_core::Error::Wireguard(_)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_connected() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.connected);
        assert_eq!(report.wireguard.config.endpoint_string(), "10.0.0.1:51820");
    }

    #[test]
    fn validate_rejects_empty_interface() {
        let config = WireGuardConfig::new("", Port::new(51_820), IpAddr::from([10, 0, 0, 1]))
            .with_peer_network(
                IpNetwork::from_str("10.0.0.0/24").expect("valid wireguard network"),
            );

        let error = config.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Wireguard(_)));
    }

    #[test]
    fn report_roundtrips_through_json() {
        let json = serde_json::to_string(&scaffold()).expect("report serializes");
        let decoded: WireGuardReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.component, COMPONENT);
        assert!(decoded.connected);
    }

    #[test]
    fn test_wgkey_from_bytes() {
        let key = WGKey::from_bytes(&[0xabu8; 32]).expect("32-byte key should be accepted");
        assert!(!key.is_zero());
        assert!(
            WGKey::from_bytes(&[0u8; 32])
                .expect("zero key is valid")
                .is_zero()
        );
        assert!(WGKey::from_bytes(&[0u8; 16]).is_err());
    }

    #[test]
    fn test_wgkey_equality() {
        let k1 = WGKey::from_bytes(&[1u8; 32]).expect("key should be accepted");
        let k2 = WGKey::from_bytes(&[1u8; 32]).expect("key should be accepted");
        let k3 = WGKey::from_bytes(&[2u8; 32]).expect("key should be accepted");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_peer_allowed_ips() {
        let key = WGKey::from_bytes(&[5u8; 32]).expect("key should be accepted");
        let mut peer = WGPeer::new(key);
        let cidr: IpNet = "10.0.0.0/24".parse().expect("CIDR should parse");
        peer.add_allowed_ip(cidr);
        peer.add_allowed_ip(cidr);
        assert_eq!(peer.allowed_ips.len(), 1);
        peer.remove_allowed_ip(&cidr);
        assert!(peer.allowed_ips.is_empty());
    }

    #[test]
    fn test_peer_registry() {
        let mut reg = WGPeerRegistry::new();
        let key = WGKey::from_bytes(&[7u8; 32]).expect("key should be accepted");
        let mut peer = WGPeer::new(key.clone());
        peer.add_allowed_ip("192.168.1.0/24".parse().expect("CIDR should parse"));
        reg.add_or_update(peer);
        assert_eq!(reg.len(), 1);
        let found = reg.peers_for_ip("192.168.1.5".parse().expect("IP should parse"));
        assert_eq!(found.len(), 1);
        reg.remove(&key);
        assert!(reg.is_empty());
    }

    #[test]
    fn test_device_config() {
        let key = WGKey::from_bytes(&[9u8; 32]).expect("key should be accepted");
        let cfg = WGDeviceConfig::new("cilium_wg0", key, 51_871);
        assert_eq!(cfg.listen_port, 51_871);
        assert_eq!(cfg.mtu, 1_420);
    }
}
