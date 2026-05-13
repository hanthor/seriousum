//! Lightweight network scaffolds for model-layer parity work.

mod types;

pub use types::{
    L2ResponderEntry, MAC, NetworkError, NodeIPv4, NodeIPv6, Route, RouteProtocol, RouteScope,
    cidr_contains, is_reserved, prefix_to_mask_v4, subtract_cidr,
};

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

    // ========== NetworkStatus Tests ==========

    #[test]
    fn network_status_equality_and_variants() {
        assert_eq!(NetworkStatus::Ready, NetworkStatus::Ready);
        assert_ne!(NetworkStatus::Ready, NetworkStatus::Degraded);
        assert_ne!(NetworkStatus::Provisioning, NetworkStatus::Ready);
    }

    #[test]
    fn network_status_serializes_snake_case() {
        let ready_json = serde_json::to_string(&NetworkStatus::Ready).expect("serializes");
        assert_eq!(ready_json, "\"ready\"");

        let provisioning_json =
            serde_json::to_string(&NetworkStatus::Provisioning).expect("serializes");
        assert_eq!(provisioning_json, "\"provisioning\"");

        let degraded_json = serde_json::to_string(&NetworkStatus::Degraded).expect("serializes");
        assert_eq!(degraded_json, "\"degraded\"");
    }

    // ========== NetworkModel Tests ==========

    #[test]
    fn network_model_new_starts_provisioning_with_default_mtu() {
        let model = NetworkModel::new("test-net", "192.168.0.0/16".parse().expect("valid cidr"));
        assert_eq!(model.status, NetworkStatus::Provisioning);
        assert_eq!(model.mtu, 1500);
        assert!(model.gateway.is_none());
        assert_eq!(model.name, "test-net");
    }

    #[test]
    fn network_model_with_gateway_sets_address() {
        let gw: IpAddr = "10.0.0.1".parse().expect("valid ip");
        let model =
            NetworkModel::new("net", "10.0.0.0/24".parse().expect("valid cidr")).with_gateway(gw);
        assert_eq!(model.gateway, Some(gw));
    }

    #[test]
    fn network_model_with_mtu_updates_field() {
        let model =
            NetworkModel::new("net", "10.0.0.0/24".parse().expect("valid cidr")).with_mtu(9000);
        assert_eq!(model.mtu, 9000);
    }

    #[test]
    fn network_model_mark_ready_and_degraded() {
        let model =
            NetworkModel::new("net", "10.0.0.0/24".parse().expect("valid cidr")).mark_ready();
        assert_eq!(model.status, NetworkStatus::Ready);

        let degraded = model.mark_degraded();
        assert_eq!(degraded.status, NetworkStatus::Degraded);
    }

    #[test]
    fn network_model_summary_contains_key_fields() {
        let model = NetworkModel::scaffold();
        let summary = model.summary();
        assert!(summary.contains("network scaffold"));
        assert!(summary.contains("10.0.0.0/24"));
        assert!(summary.contains("mtu=1500"));
    }

    #[test]
    fn network_model_validate_rejects_too_small_mtu() {
        let model = NetworkModel::new("small-mtu", "10.0.0.0/24".parse().expect("valid cidr"))
            .with_mtu(575);
        let err = model.validate().expect_err("should fail for mtu < 576");
        assert!(err.is_network());
    }

    #[test]
    fn network_model_validate_passes_for_valid_gateway_in_cidr() {
        let model = NetworkModel::new("good", "10.0.0.0/24".parse().expect("valid cidr"))
            .with_gateway("10.0.0.254".parse().expect("valid ip"))
            .mark_ready();
        assert!(model.validate().is_ok());
    }

    #[test]
    fn network_model_serialization_round_trip() {
        let model = NetworkModel::scaffold();
        let json = serde_json::to_string(&model).expect("serializes");
        let decoded: NetworkModel = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(model, decoded);
    }

    // ========== NetworkReport Tests ==========

    #[test]
    fn network_report_ready_false_when_model_not_ready() {
        let model = NetworkModel::new("provisioning", "10.0.0.0/24".parse().expect("valid cidr"));
        // status is Provisioning by default
        let report = NetworkReport::new(model);
        assert!(!report.ready);
    }

    #[test]
    fn network_report_component_is_constant() {
        let report = scaffold();
        assert_eq!(report.component, COMPONENT);
    }

    #[test]
    fn network_report_serialization_round_trip() {
        let report = scaffold();
        let json = serde_json::to_string(&report).expect("serializes");
        let decoded: NetworkReport = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(report, decoded);
    }
}
