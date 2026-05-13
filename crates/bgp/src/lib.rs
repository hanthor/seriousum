//! Pure BGP control-plane data model types.
//!
//! Ported from Cilium's `pkg/bgp/types` and related reconciler code without
//! GoBGP bindings, gRPC clients, or network I/O.

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use thiserror::Error;
use tracing::debug;

/// BGP Autonomous System Number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ASN(pub u32);

/// BGP Router ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RouterID(pub Ipv4Addr);

impl fmt::Display for RouterID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A BGP path attribute community value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Community(pub u32);

impl Community {
    /// Creates a community from an ASN and value.
    pub const fn new(asn: u16, value: u16) -> Self {
        Self(((asn as u32) << 16) | value as u32)
    }

    /// Returns the upper 16-bit ASN portion.
    pub const fn asn(&self) -> u16 {
        (self.0 >> 16) as u16
    }

    /// Returns the lower 16-bit community value portion.
    pub const fn value(&self) -> u16 {
        self.0 as u16
    }
}

impl fmt::Display for Community {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.asn(), self.value())
    }
}

/// A BGP route advertisement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advertisement {
    /// Prefix being advertised.
    pub prefix: IpNet,
    /// Next-hop address for the route.
    pub next_hop: IpAddr,
    /// Communities attached to the route.
    pub communities: Vec<Community>,
    /// Optional local preference.
    pub local_pref: Option<u32>,
    /// Optional multi-exit discriminator.
    pub med: Option<u32>,
}

impl Advertisement {
    /// Creates a new advertisement with default optional attributes.
    pub fn new(prefix: IpNet, next_hop: IpAddr) -> Self {
        Self {
            prefix,
            next_hop,
            communities: Vec::new(),
            local_pref: None,
            med: None,
        }
    }

    /// Appends a community to the advertisement.
    pub fn with_community(mut self, community: Community) -> Self {
        self.communities.push(community);
        self
    }
}

/// Desired advertisements for a local node.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvertisementSet {
    /// Pod CIDR advertisements.
    pub pod_cidr: Vec<Advertisement>,
    /// Service VIP advertisements.
    pub service_vip: Vec<Advertisement>,
    /// External IP advertisements.
    pub external_ip: Vec<Advertisement>,
}

impl AdvertisementSet {
    /// Returns an iterator over all advertisements.
    pub fn all(&self) -> impl Iterator<Item = &Advertisement> {
        self.pod_cidr
            .iter()
            .chain(self.service_vip.iter())
            .chain(self.external_ip.iter())
    }

    /// Returns the total number of advertisements.
    pub fn total_count(&self) -> usize {
        self.pod_cidr.len() + self.service_vip.len() + self.external_ip.len()
    }
}

/// State of a BGP peer session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerSessionState {
    /// Session state is unknown.
    Unknown,
    /// Session is idle.
    Idle,
    /// Session is retrying TCP connect.
    Connect,
    /// Session is actively attempting establishment.
    Active,
    /// OPEN message was sent.
    OpenSent,
    /// OPEN was acknowledged and awaiting KEEPALIVE.
    OpenConfirm,
    /// Session is established.
    Established,
}

/// Configuration for a single BGP peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConfig {
    /// Remote peer address.
    pub peer_address: IpAddr,
    /// Remote peer ASN.
    pub peer_asn: ASN,
    /// Local speaker ASN.
    pub local_asn: ASN,
    /// Connect retry timer in seconds.
    pub connect_retry_time_secs: u32,
    /// Hold timer in seconds.
    pub hold_time_secs: u32,
    /// Keepalive timer in seconds.
    pub keepalive_time_secs: u32,
    /// Whether graceful restart is enabled.
    pub graceful_restart: bool,
    /// Optional TCP authentication password.
    pub password: Option<String>,
}

impl PeerConfig {
    /// Creates a peer config with Cilium-style default timers.
    pub fn new(peer_address: IpAddr, peer_asn: ASN, local_asn: ASN) -> Self {
        Self {
            peer_address,
            peer_asn,
            local_asn,
            connect_retry_time_secs: 120,
            hold_time_secs: 90,
            keepalive_time_secs: 30,
            graceful_restart: false,
            password: None,
        }
    }
}

/// Runtime state of a BGP peer session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerState {
    /// Static peer configuration.
    pub config: PeerConfig,
    /// Current session state.
    pub session_state: PeerSessionState,
    /// Session uptime in seconds.
    pub uptime_secs: u64,
    /// Number of prefixes received from the peer.
    pub received_prefixes: u32,
    /// Number of prefixes sent to the peer.
    pub sent_prefixes: u32,
}

impl PeerState {
    /// Creates a new peer state in the idle state.
    pub fn new(config: PeerConfig) -> Self {
        Self {
            config,
            session_state: PeerSessionState::Idle,
            uptime_secs: 0,
            received_prefixes: 0,
            sent_prefixes: 0,
        }
    }

    /// Returns whether the peer session is established.
    pub fn is_established(&self) -> bool {
        self.session_state == PeerSessionState::Established
    }
}

/// Configuration for a local BGP speaker instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BGPInstanceConfig {
    /// Local ASN.
    pub local_asn: ASN,
    /// Local router ID.
    pub router_id: RouterID,
    /// Local TCP listen port.
    pub listen_port: u16,
}

/// In-memory BGP instance state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BGPInstance {
    /// Instance configuration.
    pub config: BGPInstanceConfig,
    peers: HashMap<IpAddr, PeerState>,
    advertisements: AdvertisementSet,
}

impl BGPInstance {
    /// Creates a new in-memory BGP instance.
    pub fn new(config: BGPInstanceConfig) -> Self {
        debug!(router_id = %config.router_id, local_asn = config.local_asn.0, "creating BGP instance");
        Self {
            config,
            peers: HashMap::new(),
            advertisements: AdvertisementSet::default(),
        }
    }

    /// Adds or replaces a peer in the instance.
    pub fn add_peer(&mut self, config: PeerConfig) {
        debug!(peer = %config.peer_address, peer_asn = config.peer_asn.0, "adding BGP peer");
        self.peers
            .insert(config.peer_address, PeerState::new(config));
    }

    /// Removes a peer from the instance.
    pub fn remove_peer(&mut self, addr: &IpAddr) -> Option<PeerState> {
        debug!(peer = %addr, "removing BGP peer");
        self.peers.remove(addr)
    }

    /// Returns a peer by address.
    pub fn get_peer(&self, addr: &IpAddr) -> Option<&PeerState> {
        self.peers.get(addr)
    }

    /// Returns the number of configured peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Returns an iterator over established peers.
    pub fn established_peers(&self) -> impl Iterator<Item = &PeerState> {
        self.peers.values().filter(|peer| peer.is_established())
    }

    /// Replaces the desired advertisement set.
    pub fn set_advertisements(&mut self, advertisements: AdvertisementSet) {
        debug!(
            count = advertisements.total_count(),
            "updating advertisements"
        );
        self.advertisements = advertisements;
    }

    /// Returns the current desired advertisements.
    pub fn advertisements(&self) -> &AdvertisementSet {
        &self.advertisements
    }

    /// Simulates a peer session state change.
    pub fn update_peer_state(
        &mut self,
        addr: &IpAddr,
        state: PeerSessionState,
    ) -> Result<(), BGPError> {
        self.peers
            .get_mut(addr)
            .map(|peer| {
                debug!(peer = %addr, ?state, "updating BGP peer state");
                peer.session_state = state;
            })
            .ok_or(BGPError::PeerNotFound(*addr))
    }
}

/// Errors produced by the BGP data model.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BGPError {
    /// The requested peer does not exist.
    #[error("peer {0} not found")]
    PeerNotFound(IpAddr),
    /// An ASN is invalid.
    #[error("invalid ASN: {0}")]
    InvalidASN(u32),
    /// A configuration value is invalid.
    #[error("config error: {0}")]
    InvalidConfig(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_community_encoding() {
        let community = Community::new(65_001, 100);
        assert_eq!(community.asn(), 65_001);
        assert_eq!(community.value(), 100);
        assert_eq!(community.to_string(), "65001:100");
    }

    #[test]
    fn test_advertisement_set() {
        let mut ads = AdvertisementSet::default();
        let next_hop: IpAddr = "10.0.0.1".parse().unwrap();
        ads.pod_cidr
            .push(Advertisement::new("10.1.0.0/24".parse().unwrap(), next_hop));
        ads.service_vip.push(Advertisement::new(
            "192.168.1.100/32".parse().unwrap(),
            next_hop,
        ));

        assert_eq!(ads.total_count(), 2);
        assert_eq!(ads.all().count(), 2);
    }

    #[test]
    fn test_peer_add_remove() {
        let config = BGPInstanceConfig {
            local_asn: ASN(65_000),
            router_id: RouterID("10.0.0.1".parse().unwrap()),
            listen_port: 179,
        };
        let mut instance = BGPInstance::new(config);
        let peer_config = PeerConfig::new("10.0.0.2".parse().unwrap(), ASN(65_001), ASN(65_000));

        instance.add_peer(peer_config);
        assert_eq!(instance.peer_count(), 1);

        instance.remove_peer(&"10.0.0.2".parse().unwrap());
        assert_eq!(instance.peer_count(), 0);
    }

    #[test]
    fn test_peer_state_update() {
        let config = BGPInstanceConfig {
            local_asn: ASN(65_000),
            router_id: RouterID("1.1.1.1".parse().unwrap()),
            listen_port: 179,
        };
        let mut instance = BGPInstance::new(config);
        instance.add_peer(PeerConfig::new(
            "10.0.0.2".parse().unwrap(),
            ASN(65_001),
            ASN(65_000),
        ));
        let addr: IpAddr = "10.0.0.2".parse().unwrap();

        instance
            .update_peer_state(&addr, PeerSessionState::Established)
            .unwrap();

        assert!(instance.get_peer(&addr).unwrap().is_established());
        assert_eq!(instance.established_peers().count(), 1);
    }

    #[test]
    fn test_peer_not_found_error() {
        let config = BGPInstanceConfig {
            local_asn: ASN(65_000),
            router_id: RouterID("1.1.1.1".parse().unwrap()),
            listen_port: 179,
        };
        let mut instance = BGPInstance::new(config);
        let result =
            instance.update_peer_state(&"9.9.9.9".parse().unwrap(), PeerSessionState::Idle);

        assert!(matches!(result, Err(BGPError::PeerNotFound(_))));
    }

    #[test]
    fn test_advertisement_with_community() {
        let next_hop: IpAddr = "10.0.0.1".parse().unwrap();
        let advertisement = Advertisement::new("10.0.0.0/8".parse().unwrap(), next_hop)
            .with_community(Community::new(65_000, 200));

        assert_eq!(advertisement.communities.len(), 1);
        assert_eq!(advertisement.communities[0].asn(), 65_000);
    }
}
