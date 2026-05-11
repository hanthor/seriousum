//! Lightweight wireguard scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{
    Error, IpNetwork, Port, Result,
    chrono::{DateTime, Utc},
};
use std::{net::IpAddr, str::FromStr};

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
}
