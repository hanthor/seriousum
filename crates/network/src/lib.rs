//! Lightweight network scaffolds for model-layer parity work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, IpNetwork, Port, Result};
use std::net::IpAddr;

/// Default component name for network scaffolds.
pub const COMPONENT: &str = "seriousum-network";

/// High-level status for the network scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatus {
    /// The network model is being prepared.
    Provisioning,
    /// The network model is ready for use.
    Ready,
    /// The network model needs attention.
    Degraded,
}

/// Compact network model state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkModel {
    /// Human-readable network name.
    pub name: String,

    /// Primary CIDR associated with the network.
    pub cidr: IpNetwork,

    /// Optional gateway address for the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<IpAddr>,

    /// MTU used by the network.
    pub mtu: u16,

    /// Management port reserved for control-plane integration.
    pub control_port: Port,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,

    /// Current status of the scaffold.
    pub status: NetworkStatus,
}

impl NetworkModel {
    /// Creates a new network model.
    #[must_use]
    pub fn new(name: impl Into<String>, cidr: IpNetwork) -> Self {
        Self {
            name: name.into(),
            cidr,
            gateway: None,
            mtu: 1500,
            control_port: Port::cilium_agent(),
            version: VersionInfo::current(),
            status: NetworkStatus::Provisioning,
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "network scaffold",
            "10.0.0.0/24".parse().expect("valid scaffold network"),
        )
        .with_gateway("10.0.0.1".parse().expect("valid scaffold gateway"))
        .mark_ready()
    }

    /// Updates the gateway address.
    #[must_use]
    pub fn with_gateway(mut self, gateway: IpAddr) -> Self {
        self.gateway = Some(gateway);
        self
    }

    /// Updates the MTU.
    #[must_use]
    pub fn with_mtu(mut self, mtu: u16) -> Self {
        self.mtu = mtu;
        self
    }

    /// Updates the status to ready.
    #[must_use]
    pub fn mark_ready(mut self) -> Self {
        self.status = NetworkStatus::Ready;
        self
    }

    /// Updates the status to degraded.
    #[must_use]
    pub fn mark_degraded(mut self) -> Self {
        self.status = NetworkStatus::Degraded;
        self
    }

    /// Returns a compact human-readable summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} {} mtu={} port={}",
            self.name, self.cidr, self.mtu, self.control_port
        )
    }

    /// Validates the network model.
    pub fn validate(&self) -> Result<()> {
        if self.mtu < 576 {
            return Err(Error::Network(format!("mtu {} is too small", self.mtu)));
        }

        if let Some(gateway) = self.gateway
            && !self.cidr.contains(&gateway)
        {
            return Err(Error::Network(format!(
                "gateway {gateway} is outside {}",
                self.cidr
            )));
        }

        Ok(())
    }
}

impl Default for NetworkModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable network report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkReport {
    /// Component name.
    pub component: String,

    /// Current scaffold state.
    pub model: NetworkModel,

    /// Whether the model is ready for consumption.
    pub ready: bool,
}

impl NetworkReport {
    /// Builds a new report from a model.
    #[must_use]
    pub fn new(model: NetworkModel) -> Self {
        let ready = matches!(model.status, NetworkStatus::Ready);
        Self {
            component: COMPONENT.to_owned(),
            model,
            ready,
        }
    }
}

/// Returns the standard network scaffold report.
#[must_use]
pub fn scaffold() -> NetworkReport {
    NetworkReport::new(NetworkModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_is_ready_and_serializable() {
        let report = scaffold();

        assert!(report.ready);
        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.model.status, NetworkStatus::Ready);
        assert_eq!(report.model.version, VersionInfo::current());

        let json = serde_json::to_value(&report).expect("report serializes");
        assert_eq!(
            json["model"]["control_port"],
            u16::from(Port::cilium_agent())
        );
    }

    #[test]
    fn validate_rejects_gateway_outside_cidr() {
        let model = NetworkModel::new("broken", "10.10.0.0/24".parse().expect("valid cidr"))
            .with_gateway("10.20.0.1".parse().expect("valid gateway"));

        let error = model.validate().expect_err("validation should fail");
        assert!(error.is_network());
    }
}
