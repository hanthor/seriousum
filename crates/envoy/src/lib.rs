//! Lightweight Envoy scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Port, Result};

/// Default component name for Envoy scaffolds.
pub const COMPONENT: &str = "seriousum-envoy";

/// High-level proxy status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyStatus {
    /// The proxy has not finished booting.
    Starting,
    /// The proxy is ready to serve traffic.
    Ready,
    /// The proxy is serving traffic with reduced capacity.
    Degraded,
}

/// Compact Envoy cluster model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvoyCluster {
    /// Cluster name.
    pub name: String,

    /// Logical service name.
    pub service: String,

    /// Upstream port.
    pub port: Port,

    /// Known upstream endpoints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<String>,
}

impl EnvoyCluster {
    /// Creates a new cluster model.
    #[must_use]
    pub fn new(name: impl Into<String>, service: impl Into<String>, port: Port) -> Self {
        Self {
            name: name.into(),
            service: service.into(),
            port,
            endpoints: Vec::new(),
        }
    }

    /// Adds an endpoint.
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoints.push(endpoint.into());
        self
    }
}

/// Compact Envoy model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvoyModel {
    /// Component name.
    pub component: String,

    /// Admin port.
    pub admin_port: Port,

    /// Current proxy status.
    pub status: ProxyStatus,

    /// Configured clusters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clusters: Vec<EnvoyCluster>,
}

impl EnvoyModel {
    /// Creates a new Envoy model.
    #[must_use]
    pub fn new(admin_port: Port) -> Self {
        Self {
            component: COMPONENT.to_owned(),
            admin_port,
            status: ProxyStatus::Starting,
            clusters: Vec::new(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(Port::cilium_operator())
            .with_cluster(
                EnvoyCluster::new("health", "health.svc", Port::cilium_health())
                    .with_endpoint("127.0.0.1:4244"),
            )
            .ready()
    }

    /// Adds a cluster.
    #[must_use]
    pub fn with_cluster(mut self, cluster: EnvoyCluster) -> Self {
        self.clusters.push(cluster);
        self
    }

    /// Marks the proxy as ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.status = ProxyStatus::Ready;
        self
    }

    /// Returns the number of configured clusters.
    #[must_use]
    pub fn cluster_count(&self) -> usize {
        self.clusters.len()
    }

    /// Returns a concise summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} clusters={} status={:?}",
            self.component,
            self.cluster_count(),
            self.status
        )
    }

    /// Validates the Envoy model.
    pub fn validate(&self) -> Result<()> {
        if self.clusters.is_empty() {
            return Err(Error::Envoy(String::from(
                "envoy model must contain at least one cluster",
            )));
        }

        if self
            .clusters
            .iter()
            .any(|cluster| cluster.name.trim().is_empty() || cluster.service.trim().is_empty())
        {
            return Err(Error::Envoy(String::from(
                "clusters must have names and services",
            )));
        }

        Ok(())
    }
}

impl Default for EnvoyModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable Envoy report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvoyReport {
    /// Component name.
    pub component: String,

    /// Envoy model.
    pub envoy: EnvoyModel,

    /// Whether the proxy is ready.
    pub ready: bool,
}

impl EnvoyReport {
    /// Builds a report from an Envoy model.
    #[must_use]
    pub fn new(envoy: EnvoyModel) -> Self {
        let ready = matches!(envoy.status, ProxyStatus::Ready) && !envoy.clusters.is_empty();
        Self {
            component: COMPONENT.to_owned(),
            envoy,
            ready,
        }
    }
}

/// Returns the standard Envoy scaffold report.
#[must_use]
pub fn scaffold() -> EnvoyReport {
    EnvoyReport::new(EnvoyModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_ready() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.ready);
        assert_eq!(report.envoy.cluster_count(), 1);
        assert!(matches!(report.envoy.status, ProxyStatus::Ready));
    }

    #[test]
    fn validate_rejects_empty_clusters() {
        let model = EnvoyModel::new(Port::cilium_operator());

        let error = model.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Envoy(_)));
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = scaffold();
        let encoded = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: EnvoyReport =
            serde_json::from_str(&encoded).expect("deserialization should succeed");

        assert_eq!(decoded, report);
    }
}
