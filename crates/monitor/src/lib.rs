//! Lightweight monitor scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Port, Result};

/// Default component name for monitor scaffolds.
pub const COMPONENT: &str = "seriousum-monitor";

/// Probe status for a monitored target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    /// The probe has not been evaluated yet.
    Unknown,
    /// The target is healthy.
    Healthy,
    /// The target is unhealthy but still reachable.
    Degraded,
    /// The target is unreachable.
    Failed,
}

/// Monitored target description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorTarget {
    /// Target name.
    pub name: String,

    /// Probe endpoint path.
    pub path: String,

    /// Probe port.
    pub port: Port,

    /// Current probe status.
    pub status: ProbeStatus,
}

impl MonitorTarget {
    /// Creates a new monitor target.
    #[must_use]
    pub fn new(name: impl Into<String>, path: impl Into<String>, port: Port) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            port,
            status: ProbeStatus::Unknown,
        }
    }

    /// Marks the target as healthy.
    #[must_use]
    pub fn healthy(mut self) -> Self {
        self.status = ProbeStatus::Healthy;
        self
    }
}

/// Compact monitor model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorModel {
    /// Component name.
    pub component: String,

    /// Tracked targets.
    pub targets: Vec<MonitorTarget>,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl MonitorModel {
    /// Creates a new monitor model.
    #[must_use]
    pub fn new(component: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            targets: Vec::new(),
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(COMPONENT)
            .with_target(
                MonitorTarget::new("healthz", "/healthz", Port::cilium_operator()).healthy(),
            )
            .with_target(
                MonitorTarget::new("metrics", "/metrics", Port::cilium_operator()).healthy(),
            )
    }

    /// Adds a target.
    #[must_use]
    pub fn with_target(mut self, target: MonitorTarget) -> Self {
        self.targets.push(target);
        self
    }

    /// Returns the number of healthy targets.
    #[must_use]
    pub fn healthy_targets(&self) -> usize {
        self.targets
            .iter()
            .filter(|target| matches!(target.status, ProbeStatus::Healthy))
            .count()
    }

    /// Returns whether any target has failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.targets
            .iter()
            .any(|target| matches!(target.status, ProbeStatus::Failed))
    }

    /// Returns a concise summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} targets={} healthy={}",
            self.component,
            self.targets.len(),
            self.healthy_targets()
        )
    }

    /// Validates the monitor model.
    pub fn validate(&self) -> Result<()> {
        if self.targets.is_empty() {
            return Err(Error::Monitor(String::from(
                "monitor model must contain at least one target",
            )));
        }

        Ok(())
    }
}

impl Default for MonitorModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable monitor report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorReport {
    /// Component name.
    pub component: String,

    /// Monitor model.
    pub monitor: MonitorModel,

    /// Whether all targets are healthy.
    pub healthy: bool,
}

impl MonitorReport {
    /// Builds a report from a monitor model.
    #[must_use]
    pub fn new(monitor: MonitorModel) -> Self {
        let healthy = monitor
            .targets
            .iter()
            .all(|target| matches!(target.status, ProbeStatus::Healthy));
        Self {
            component: COMPONENT.to_owned(),
            monitor,
            healthy,
        }
    }
}

/// Returns the standard monitor scaffold report.
#[must_use]
pub fn scaffold() -> MonitorReport {
    MonitorReport::new(MonitorModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_marks_healthy_targets() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.healthy);
        assert_eq!(report.monitor.version, VersionInfo::current());
        assert_eq!(report.monitor.healthy_targets(), 2);
    }

    #[test]
    fn validate_rejects_empty_monitor() {
        let monitor = MonitorModel::new(COMPONENT);

        let error = monitor.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Monitor(_)));
    }
}
