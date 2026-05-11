use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use seriousum_api::{HealthReport, HealthStatus, VersionInfo};

/// Default component name reported by the operator scaffold.
pub const OPERATOR_COMPONENT: &str = "seriousum-operator";

/// Reusable operator state for reporting health and version metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    summary: String,
    status: HealthStatus,
    version: VersionInfo,
}

impl Operator {
    /// Creates a healthy operator state with the provided summary.
    #[must_use]
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            status: HealthStatus::Healthy,
            version: VersionInfo::current(),
        }
    }

    /// Creates a healthy operator state with the default scaffold summary.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("operator scaffold ready")
    }

    /// Returns the current health status.
    #[must_use]
    pub fn status(&self) -> HealthStatus {
        self.status
    }

    /// Returns the operator summary string.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Returns the current version metadata.
    #[must_use]
    pub fn version(&self) -> &VersionInfo {
        &self.version
    }

    /// Updates the operator health status.
    #[must_use]
    pub fn with_status(mut self, status: HealthStatus) -> Self {
        self.status = status;
        self
    }

    /// Updates the operator version metadata.
    #[must_use]
    pub fn with_version(mut self, version: VersionInfo) -> Self {
        self.version = version;
        self
    }

    /// Builds a health report using the shared API contract.
    #[must_use]
    pub fn health_report(&self) -> HealthReport {
        HealthReport {
            status: self.status,
            message: Some(self.summary.clone()),
            version: self.version.clone(),
        }
    }

    /// Builds a complete operator report for future HTTP endpoints.
    #[must_use]
    pub fn report(&self) -> OperatorReport {
        OperatorReport {
            summary: self.summary.clone(),
            health: self.health_report(),
            version: self.version.clone(),
        }
    }

    /// Builds the `/healthz` payload.
    #[must_use]
    pub fn healthz_payload(&self) -> HealthReport {
        healthz_payload(self)
    }

    /// Builds the `/v1/metrics` payload.
    #[must_use]
    pub fn metrics_payload(&self) -> MetricsPayload {
        metrics_payload(self)
    }

    /// Builds the `/v1/cluster` payload.
    #[must_use]
    pub fn cluster_payload(&self) -> ClusterPayload {
        cluster_payload(self)
    }

    /// Builds all scaffold endpoint payloads.
    #[must_use]
    pub fn scaffold_payloads(&self) -> ScaffoldPayloads {
        ScaffoldPayloads {
            healthz: self.healthz_payload(),
            metrics: self.metrics_payload(),
            cluster: self.cluster_payload(),
        }
    }
}

impl Default for Operator {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable operator reporting payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorReport {
    /// Human-readable summary for the operator instance.
    pub summary: String,

    /// Shared health report data.
    pub health: HealthReport,

    /// Shared version metadata.
    pub version: VersionInfo,
}

/// Payload for the `/v1/metrics` scaffold endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsPayload {
    /// Endpoint path represented by this payload.
    pub path: String,

    /// Shared health report data.
    pub health: HealthReport,

    /// Shared version metadata.
    pub version: VersionInfo,

    /// Example metrics emitted by the scaffold.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<String, u64>,
}

/// Payload for the `/v1/cluster` scaffold endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterPayload {
    /// Endpoint path represented by this payload.
    pub path: String,

    /// Shared health report data.
    pub health: HealthReport,

    /// Shared version metadata.
    pub version: VersionInfo,

    /// High-level cluster status for the scaffold.
    pub status: HealthStatus,

    /// Human-readable cluster summary.
    pub summary: String,
}

/// Aggregated payloads for the operator scaffold endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScaffoldPayloads {
    /// `/healthz` payload.
    pub healthz: HealthReport,

    /// `/v1/metrics` payload.
    pub metrics: MetricsPayload,

    /// `/v1/cluster` payload.
    pub cluster: ClusterPayload,
}

/// Builds the `/healthz` payload.
#[must_use]
pub fn healthz_payload(operator: &Operator) -> HealthReport {
    operator.health_report()
}

/// Builds the `/v1/metrics` payload.
#[must_use]
pub fn metrics_payload(operator: &Operator) -> MetricsPayload {
    let mut metrics = BTreeMap::new();
    metrics.insert(
        String::from("operator_scaffold_ready"),
        if operator.status() == HealthStatus::Healthy {
            1
        } else {
            0
        },
    );

    MetricsPayload {
        path: String::from("/v1/metrics"),
        health: operator.health_report(),
        version: operator.version().clone(),
        metrics,
    }
}

/// Builds the `/v1/cluster` payload.
#[must_use]
pub fn cluster_payload(operator: &Operator) -> ClusterPayload {
    ClusterPayload {
        path: String::from("/v1/cluster"),
        health: operator.health_report(),
        version: operator.version().clone(),
        status: operator.status(),
        summary: operator.summary().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scaffold_reports_healthy_with_current_version() {
        let operator = Operator::scaffold();
        let report = operator.report();

        assert_eq!(operator.status(), HealthStatus::Healthy);
        assert_eq!(operator.summary(), "operator scaffold ready");
        assert_eq!(operator.version().contract, VersionInfo::current().contract);
        assert_eq!(report.summary, "operator scaffold ready");
        assert_eq!(report.health.status, HealthStatus::Healthy);
        assert_eq!(
            report.health.message.as_deref(),
            Some("operator scaffold ready")
        );
        assert_eq!(report.health.version, VersionInfo::current());
        assert_eq!(report.version, VersionInfo::current());
    }

    #[test]
    fn health_report_reflects_custom_state() {
        let version = VersionInfo {
            contract: "contract-x".to_owned(),
            core: "core-y".to_owned(),
        };

        let operator = Operator::new("warming up")
            .with_status(HealthStatus::Degraded)
            .with_version(version.clone());

        let report = operator.health_report();

        assert_eq!(report.status, HealthStatus::Degraded);
        assert_eq!(report.message.as_deref(), Some("warming up"));
        assert_eq!(report.version, version);
    }

    #[test]
    fn healthz_payload_serializes_like_the_api_contract() {
        let payload = healthz_payload(&Operator::scaffold());
        let json = serde_json::to_value(&payload).expect("healthz payload serializes");

        assert_eq!(
            json,
            json!({
                "status": "healthy",
                "message": "operator scaffold ready",
                "version": {
                    "contract": VersionInfo::current().contract,
                    "core": VersionInfo::current().core,
                }
            })
        );
    }

    #[test]
    fn metrics_payload_serializes_endpoint_shape() {
        let payload = metrics_payload(&Operator::scaffold());
        let json = serde_json::to_value(&payload).expect("metrics payload serializes");

        assert_eq!(json["path"], "/v1/metrics");
        assert_eq!(json["health"]["status"], "healthy");
        assert_eq!(json["health"]["message"], "operator scaffold ready");
        assert_eq!(
            json["version"],
            json!({
                "contract": VersionInfo::current().contract,
                "core": VersionInfo::current().core,
            })
        );
        assert_eq!(json["metrics"], json!({"operator_scaffold_ready": 1}));
    }

    #[test]
    fn cluster_payload_serializes_endpoint_shape() {
        let payload =
            cluster_payload(&Operator::new("cluster ready").with_status(HealthStatus::Degraded));
        let json = serde_json::to_value(&payload).expect("cluster payload serializes");

        assert_eq!(
            json,
            json!({
                "path": "/v1/cluster",
                "health": {
                    "status": "degraded",
                    "message": "cluster ready",
                    "version": {
                        "contract": VersionInfo::current().contract,
                        "core": VersionInfo::current().core,
                    }
                },
                "version": {
                    "contract": VersionInfo::current().contract,
                    "core": VersionInfo::current().core,
                },
                "status": "degraded",
                "summary": "cluster ready"
            })
        );
    }
}
