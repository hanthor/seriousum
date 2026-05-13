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

    #[test]
    fn probe_status_equality() {
        assert_eq!(ProbeStatus::Healthy, ProbeStatus::Healthy);
        assert_ne!(ProbeStatus::Healthy, ProbeStatus::Failed);
        assert_ne!(ProbeStatus::Unknown, ProbeStatus::Degraded);
    }

    #[test]
    fn probe_status_serializes_snake_case() {
        let healthy_json = serde_json::to_string(&ProbeStatus::Healthy).expect("serializes");
        assert_eq!(healthy_json, "\"healthy\"");

        let failed_json = serde_json::to_string(&ProbeStatus::Failed).expect("serializes");
        assert_eq!(failed_json, "\"failed\"");

        let degraded_json = serde_json::to_string(&ProbeStatus::Degraded).expect("serializes");
        assert_eq!(degraded_json, "\"degraded\"");
    }

    #[test]
    fn monitor_target_new_starts_unknown() {
        let target = MonitorTarget::new("api", "/api/health", Port::cilium_agent());
        assert_eq!(target.status, ProbeStatus::Unknown);
        assert_eq!(target.name, "api");
        assert_eq!(target.path, "/api/health");
    }

    #[test]
    fn monitor_target_healthy_transitions_status() {
        let target = MonitorTarget::new("metrics", "/metrics", Port::cilium_operator()).healthy();
        assert_eq!(target.status, ProbeStatus::Healthy);
    }

    #[test]
    fn monitor_target_serialization_round_trip() {
        let target = MonitorTarget::new("healthz", "/healthz", Port::cilium_operator()).healthy();
        let json = serde_json::to_string(&target).expect("serializes");
        let decoded: MonitorTarget = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(target, decoded);
    }

    #[test]
    fn monitor_model_with_target_appends() {
        let model = MonitorModel::new("test")
            .with_target(MonitorTarget::new("t1", "/t1", Port::cilium_agent()).healthy())
            .with_target(MonitorTarget::new("t2", "/t2", Port::cilium_operator()));

        assert_eq!(model.targets.len(), 2);
        assert_eq!(model.healthy_targets(), 1);
    }

    #[test]
    fn monitor_model_has_failures_detects_failed_target() {
        let model = MonitorModel::new("test").with_target(MonitorTarget {
            name: "down".into(),
            path: "/down".into(),
            port: Port::cilium_agent(),
            status: ProbeStatus::Failed,
        });

        assert!(model.has_failures());
        assert!(!MonitorModel::scaffold().has_failures());
    }

    #[test]
    fn monitor_model_summary_contains_component_and_counts() {
        let model = MonitorModel::new("mycomp")
            .with_target(MonitorTarget::new("a", "/a", Port::cilium_agent()).healthy());

        let summary = model.summary();
        assert!(summary.contains("mycomp"));
        assert!(summary.contains("targets=1"));
        assert!(summary.contains("healthy=1"));
    }

    #[test]
    fn monitor_model_validate_passes_with_targets() {
        let model = MonitorModel::scaffold();
        assert!(model.validate().is_ok());
    }

    #[test]
    fn monitor_report_healthy_false_when_any_target_not_healthy() {
        let model = MonitorModel::new("mixed")
            .with_target(MonitorTarget::new("ok", "/ok", Port::cilium_agent()).healthy())
            .with_target(MonitorTarget::new("bad", "/bad", Port::cilium_operator()));
        let report = MonitorReport::new(model);
        assert!(!report.healthy);
    }

    #[test]
    fn monitor_report_serialization_round_trip() {
        let report = scaffold();
        let json = serde_json::to_string(&report).expect("serializes");
        let decoded: MonitorReport = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(report, decoded);
    }
}
