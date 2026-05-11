//! Lightweight load balancer scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Port, Result};
use std::net::IpAddr;

/// Default component name for load balancer scaffolds.
pub const COMPONENT: &str = "seriousum-loadbalancer";

/// Load balancing strategy for the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BalancingMode {
    /// Translate to a node-local backend.
    Nat,
    /// Preserve the original destination.
    Dsr,
}

/// Backend target for a virtual service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backend {
    /// Backend address.
    pub address: IpAddr,

    /// Backend port.
    pub port: Port,

    /// Backend selection weight.
    pub weight: u16,
}

impl Backend {
    /// Creates a backend target.
    #[must_use]
    pub fn new(address: IpAddr, port: Port) -> Self {
        Self {
            address,
            port,
            weight: 1,
        }
    }
}

/// Virtual service model for the load balancer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceModel {
    /// Service name.
    pub name: String,

    /// Frontend address.
    pub frontend: IpAddr,

    /// Frontend port.
    pub port: Port,

    /// Balancing strategy.
    pub mode: BalancingMode,

    /// Backends serving the service.
    pub backends: Vec<Backend>,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl ServiceModel {
    /// Creates a new service model.
    #[must_use]
    pub fn new(name: impl Into<String>, frontend: IpAddr, port: Port) -> Self {
        Self {
            name: name.into(),
            frontend,
            port,
            mode: BalancingMode::Nat,
            backends: vec![Backend::new(
                IpAddr::from([127, 0, 0, 1]),
                Port::cilium_operator(),
            )],
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            "service scaffold",
            IpAddr::from([10, 0, 0, 10]),
            Port::cilium_agent(),
        )
    }

    /// Updates the balancing mode.
    #[must_use]
    pub fn with_mode(mut self, mode: BalancingMode) -> Self {
        self.mode = mode;
        self
    }

    /// Adds a backend to the service.
    #[must_use]
    pub fn with_backend(mut self, backend: Backend) -> Self {
        self.backends.push(backend);
        self
    }

    /// Returns a socket address-like summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} {}:{} backends={}",
            self.name,
            self.frontend,
            self.port,
            self.backends.len()
        )
    }

    /// Returns the first backend or an error if none exist.
    pub fn primary_backend(&self) -> Result<&Backend> {
        self.backends
            .first()
            .ok_or_else(|| Error::Loadbalancer(String::from("service has no backends")))
    }

    /// Validates the service model.
    pub fn validate(&self) -> Result<()> {
        self.primary_backend()?;

        if self.backends.iter().any(|backend| backend.weight == 0) {
            return Err(Error::Loadbalancer(String::from(
                "backend weight must be non-zero",
            )));
        }

        Ok(())
    }
}

impl Default for ServiceModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable service report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceReport {
    /// Component name.
    pub component: String,

    /// Load balancer model.
    pub service: ServiceModel,

    /// Whether the service is ready to route traffic.
    pub ready: bool,
}

impl ServiceReport {
    /// Builds a report from a service model.
    #[must_use]
    pub fn new(service: ServiceModel) -> Self {
        let ready = !service.backends.is_empty();
        Self {
            component: COMPONENT.to_owned(),
            service,
            ready,
        }
    }
}

/// Returns the standard load balancer scaffold report.
#[must_use]
pub fn scaffold() -> ServiceReport {
    ServiceReport::new(ServiceModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_has_a_backend() {
        let report = scaffold();

        assert!(report.ready);
        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.service.version, VersionInfo::current());
        assert_eq!(
            report
                .service
                .primary_backend()
                .expect("backend exists")
                .port,
            Port::cilium_operator()
        );
    }

    #[test]
    fn validate_rejects_zero_weight_backend() {
        let service =
            ServiceModel::new("broken", IpAddr::from([10, 0, 0, 20]), Port::cilium_agent())
                .with_backend(Backend {
                    address: IpAddr::from([10, 0, 0, 21]),
                    port: Port::cilium_operator(),
                    weight: 0,
                });

        let error = service.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Loadbalancer(_)));
    }
}
