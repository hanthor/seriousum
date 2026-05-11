//! Lightweight node scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Identity, Port, Result, SecurityIdentity, SecurityLabel};
use std::net::IpAddr;

/// Default component name for node scaffolds.
pub const COMPONENT: &str = "seriousum-node";

/// Node role used by the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    /// A worker node.
    Worker,
    /// A control-plane node.
    ControlPlane,
    /// A hybrid node.
    Hybrid,
}

/// Compact node model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeModel {
    /// Node name.
    pub name: String,

    /// Security identity tied to the node.
    pub identity: Identity,

    /// Primary node address.
    pub address: IpAddr,

    /// Management or health port.
    pub port: Port,

    /// Node role.
    pub role: NodeRole,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl NodeModel {
    /// Creates a new node model.
    #[must_use]
    pub fn new(name: impl Into<String>, identity: Identity, address: IpAddr, port: Port) -> Self {
        Self {
            name: name.into(),
            identity,
            address,
            port,
            role: NodeRole::Worker,
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "node scaffold",
            Identity::new(
                SecurityIdentity::host(),
                [SecurityLabel::new("node", "scaffold")],
            ),
            IpAddr::from([127, 0, 0, 1]),
            Port::cilium_operator(),
        )
        .with_role(NodeRole::ControlPlane)
    }

    /// Updates the node role.
    #[must_use]
    pub fn with_role(mut self, role: NodeRole) -> Self {
        self.role = role;
        self
    }

    /// Returns the node socket string.
    #[must_use]
    pub fn socket_string(&self) -> String {
        format!("{}:{}", self.address, self.port)
    }

    /// Returns a concise node summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} role={:?} socket={}",
            self.name,
            self.role,
            self.socket_string()
        )
    }

    /// Validates the node model.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::Node(String::from("node name must not be empty")));
        }

        Ok(())
    }
}

impl Default for NodeModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable node report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeReport {
    /// Component name.
    pub component: String,

    /// Node model.
    pub node: NodeModel,

    /// Whether the node has a usable control-plane role.
    pub control_plane: bool,
}

impl NodeReport {
    /// Builds a report from a node model.
    #[must_use]
    pub fn new(node: NodeModel) -> Self {
        let control_plane = matches!(node.role, NodeRole::ControlPlane | NodeRole::Hybrid);
        Self {
            component: COMPONENT.to_owned(),
            node,
            control_plane,
        }
    }
}

/// Returns the standard node scaffold report.
#[must_use]
pub fn scaffold() -> NodeReport {
    NodeReport::new(NodeModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_marks_control_plane_nodes() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.control_plane);
        assert_eq!(report.node.version, VersionInfo::current());
        assert_eq!(report.node.socket_string(), "127.0.0.1:9234");
    }

    #[test]
    fn validate_rejects_empty_names() {
        let node = NodeModel::new(
            "",
            Identity::new(
                SecurityIdentity::cluster(),
                [SecurityLabel::new("node", "empty")],
            ),
            IpAddr::from([127, 0, 0, 1]),
            Port::cilium_operator(),
        );

        let error = node.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Node(_)));
    }
}
