//! Lightweight CNI scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Identity, IpNetwork, Result, SecurityIdentity, SecurityLabel};

/// Default component name for CNI scaffolds.
pub const COMPONENT: &str = "seriousum-cni";

/// CNI operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CniOperation {
    /// Add a pod to the network.
    Add,
    /// Check the pod network.
    Check,
    /// Delete the pod network.
    Delete,
}

/// CNI lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CniState {
    /// Setup is pending.
    Pending,
    /// CNI is ready.
    Ready,
    /// CNI has been torn down.
    Deleted,
}

/// Compact CNI configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniConfig {
    /// Plugin name.
    pub plugin_name: String,

    /// Pod CIDR assigned to the plugin.
    pub pod_cidr: IpNetwork,

    /// MTU for the pod interface.
    pub mtu: u32,

    /// Whether masquerading is enabled.
    pub masquerade: bool,
}

impl CniConfig {
    /// Creates a new CNI configuration.
    #[must_use]
    pub fn new(plugin_name: impl Into<String>, pod_cidr: IpNetwork) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            pod_cidr,
            mtu: 1_500,
            masquerade: true,
        }
    }

    /// Returns the default scaffold configuration.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "seriousum-cni",
            "10.42.0.0/24".parse().expect("valid cni pod cidr"),
        )
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.plugin_name.trim().is_empty() {
            return Err(Error::Cni(String::from(
                "cni plugin name must not be empty",
            )));
        }

        if self.mtu < 576 {
            return Err(Error::Cni(String::from("cni mtu must be at least 576")));
        }

        Ok(())
    }
}

impl Default for CniConfig {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// CNI session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniSession {
    /// Container identifier.
    pub container_id: String,

    /// Network namespace path.
    pub netns: String,

    /// Requested operation.
    pub operation: CniOperation,

    /// Whether the session is active.
    pub active: bool,
}

impl CniSession {
    /// Creates a new CNI session.
    #[must_use]
    pub fn new(
        container_id: impl Into<String>,
        netns: impl Into<String>,
        operation: CniOperation,
    ) -> Self {
        Self {
            container_id: container_id.into(),
            netns: netns.into(),
            operation,
            active: true,
        }
    }

    /// Returns the default scaffold session.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("container-scaffold", "/proc/self/ns/net", CniOperation::Add)
    }

    /// Marks the session inactive.
    #[must_use]
    pub fn deactivate(mut self) -> Self {
        self.active = false;
        self
    }

    /// Validates the session.
    pub fn validate(&self) -> Result<()> {
        if self.container_id.trim().is_empty() {
            return Err(Error::Cni(String::from(
                "cni container id must not be empty",
            )));
        }

        if self.netns.trim().is_empty() {
            return Err(Error::Cni(String::from("cni netns must not be empty")));
        }

        Ok(())
    }
}

impl Default for CniSession {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Compact CNI model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniModel {
    /// Identity associated with the workload.
    pub identity: Identity,

    /// CNI configuration.
    pub config: CniConfig,

    /// Session details.
    pub session: CniSession,

    /// Lifecycle state.
    pub state: CniState,
}

impl CniModel {
    /// Creates a new CNI model.
    #[must_use]
    pub fn new(identity: Identity, config: CniConfig, session: CniSession) -> Self {
        Self {
            identity,
            config,
            session,
            state: CniState::Pending,
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            Identity::new(
                SecurityIdentity::unmanaged(),
                [SecurityLabel::new("cni", "scaffold")],
            ),
            CniConfig::scaffold(),
            CniSession::scaffold(),
        )
        .ready()
    }

    /// Marks the model ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.state = CniState::Ready;
        self
    }

    /// Marks the model deleted.
    #[must_use]
    pub fn deleted(mut self) -> Self {
        self.state = CniState::Deleted;
        self
    }

    /// Returns a stable summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "plugin={} cidr={} active={}",
            self.config.plugin_name, self.config.pod_cidr, self.session.active
        )
    }

    /// Validates the model.
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        self.session.validate()?;

        Ok(())
    }
}

impl Default for CniModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable CNI report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CniReport {
    /// Component name.
    pub component: String,

    /// CNI model.
    pub cni: CniModel,

    /// Whether the CNI is ready.
    pub ready: bool,
}

impl CniReport {
    /// Builds a report from a CNI model.
    #[must_use]
    pub fn new(cni: CniModel) -> Self {
        let ready = matches!(cni.state, CniState::Ready) && cni.session.active;
        Self {
            component: COMPONENT.to_owned(),
            ready,
            cni,
        }
    }
}

/// Returns the standard CNI scaffold report.
#[must_use]
pub fn scaffold() -> CniReport {
    CniReport::new(CniModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_ready() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.ready);
        assert_eq!(report.cni.identity.id, SecurityIdentity::unmanaged());
    }

    #[test]
    fn validate_rejects_empty_plugin_name() {
        let config = CniConfig::new("", "10.42.0.0/24".parse().expect("valid cni pod cidr"));

        let error = config.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Cni(_)));
    }

    #[test]
    fn report_roundtrips_through_json() {
        let json = serde_json::to_string(&scaffold()).expect("report serializes");
        let decoded: CniReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.component, COMPONENT);
        assert!(decoded.ready);
    }
}
