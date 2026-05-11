//! Lightweight endpoint scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Identity, Port, Result, SecurityIdentity, SecurityLabel};
use std::{collections::BTreeMap, net::IpAddr};

/// Default component name for endpoint scaffolds.
pub const COMPONENT: &str = "seriousum-endpoint";

/// Endpoint lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointState {
    /// The endpoint is being prepared.
    Pending,
    /// The endpoint is ready for traffic.
    Ready,
    /// The endpoint is draining.
    Draining,
}

/// Compact endpoint model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointModel {
    /// Endpoint name.
    pub name: String,

    /// Security identity associated with the endpoint.
    pub identity: Identity,

    /// Primary endpoint address.
    pub address: IpAddr,

    /// Primary endpoint port.
    pub port: Port,

    /// Endpoint labels for policy and routing hints.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,

    /// Current endpoint state.
    pub state: EndpointState,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl EndpointModel {
    /// Creates a new endpoint model.
    #[must_use]
    pub fn new(name: impl Into<String>, identity: Identity, address: IpAddr, port: Port) -> Self {
        Self {
            name: name.into(),
            identity,
            address,
            port,
            labels: BTreeMap::from([(String::from("workload"), String::from("scaffold"))]),
            state: EndpointState::Pending,
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "endpoint scaffold",
            Identity::new(
                SecurityIdentity::world(),
                [SecurityLabel::new("endpoint", "scaffold")],
            ),
            IpAddr::from([127, 0, 0, 1]),
            Port::cilium_health(),
        )
        .mark_ready()
    }

    /// Marks the endpoint as ready.
    #[must_use]
    pub fn mark_ready(mut self) -> Self {
        self.state = EndpointState::Ready;
        self
    }

    /// Marks the endpoint as draining.
    #[must_use]
    pub fn mark_draining(mut self) -> Self {
        self.state = EndpointState::Draining;
        self
    }

    /// Adds or updates a label.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Returns the endpoint socket-like string.
    #[must_use]
    pub fn socket_string(&self) -> String {
        format!("{}:{}", self.address, self.port)
    }

    /// Validates the endpoint model.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::Endpoint(String::from(
                "endpoint name must not be empty",
            )));
        }

        if matches!(self.address, IpAddr::V4(addr) if addr.is_unspecified()) {
            return Err(Error::Endpoint(String::from(
                "endpoint address must not be unspecified",
            )));
        }

        Ok(())
    }
}

impl Default for EndpointModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable endpoint report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointReport {
    /// Component name.
    pub component: String,

    /// Endpoint model.
    pub endpoint: EndpointModel,

    /// Whether the endpoint is ready.
    pub ready: bool,
}

impl EndpointReport {
    /// Builds a report from an endpoint model.
    #[must_use]
    pub fn new(endpoint: EndpointModel) -> Self {
        let ready = matches!(endpoint.state, EndpointState::Ready);
        Self {
            component: COMPONENT.to_owned(),
            endpoint,
            ready,
        }
    }
}

/// Returns the standard endpoint scaffold report.
#[must_use]
pub fn scaffold() -> EndpointReport {
    EndpointReport::new(EndpointModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_ready() {
        let report = scaffold();

        assert!(report.ready);
        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.endpoint.version, VersionInfo::current());
        assert_eq!(report.endpoint.socket_string(), "127.0.0.1:4244");
    }

    #[test]
    fn validate_rejects_unspecified_addresses() {
        let endpoint = EndpointModel::new(
            "bad",
            Identity::new(
                SecurityIdentity::host(),
                [SecurityLabel::new("kind", "bad")],
            ),
            IpAddr::from([0, 0, 0, 0]),
            Port::cilium_health(),
        );

        let error = endpoint.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Endpoint(_)));
    }
}
