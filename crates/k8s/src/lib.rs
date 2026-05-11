//! Lightweight Kubernetes scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Result};

/// Default component name for Kubernetes scaffolds.
pub const COMPONENT: &str = "seriousum-k8s";

/// Resource kind used by the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// A namespace.
    Namespace,
    /// A deployment.
    Deployment,
    /// A service.
    Service,
    /// A pod.
    Pod,
}

/// Lifecycle phase for a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePhase {
    /// The resource is pending.
    Pending,
    /// The resource is ready.
    Ready,
    /// The resource is unhealthy.
    Failed,
}

/// Compact Kubernetes resource model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct K8sResource {
    /// Resource kind.
    pub kind: ResourceKind,

    /// Resource name.
    pub name: String,

    /// Optional namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Current resource phase.
    pub phase: ResourcePhase,
}

impl K8sResource {
    /// Creates a new Kubernetes resource.
    #[must_use]
    pub fn new(kind: ResourceKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            namespace: None,
            phase: ResourcePhase::Pending,
        }
    }

    /// Adds a namespace.
    #[must_use]
    pub fn in_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Marks the resource as ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.phase = ResourcePhase::Ready;
        self
    }
}

/// Compact Kubernetes model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct K8sModel {
    /// Component name.
    pub component: String,

    /// Target namespace.
    pub namespace: String,

    /// Managed resources.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<K8sResource>,
}

impl K8sModel {
    /// Creates a new Kubernetes model.
    #[must_use]
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            component: COMPONENT.to_owned(),
            namespace: namespace.into(),
            resources: Vec::new(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("seriousum")
            .with_resource(K8sResource::new(ResourceKind::Namespace, "seriousum").ready())
            .with_resource(
                K8sResource::new(ResourceKind::Deployment, "seriousum-agent")
                    .in_namespace("seriousum")
                    .ready(),
            )
            .with_resource(
                K8sResource::new(ResourceKind::Service, "seriousum-agent")
                    .in_namespace("seriousum")
                    .ready(),
            )
    }

    /// Adds a resource.
    #[must_use]
    pub fn with_resource(mut self, resource: K8sResource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Returns the number of ready resources.
    #[must_use]
    pub fn ready_resources(&self) -> usize {
        self.resources
            .iter()
            .filter(|resource| matches!(resource.phase, ResourcePhase::Ready))
            .count()
    }

    /// Returns a concise summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} namespace={} resources={} ready={}",
            self.component,
            self.namespace,
            self.resources.len(),
            self.ready_resources()
        )
    }

    /// Validates the Kubernetes model.
    pub fn validate(&self) -> Result<()> {
        if self.namespace.trim().is_empty() {
            return Err(Error::K8s(String::from("namespace must not be empty")));
        }

        if self.resources.is_empty() {
            return Err(Error::K8s(String::from(
                "k8s model must contain at least one resource",
            )));
        }

        if self
            .resources
            .iter()
            .any(|resource| resource.name.trim().is_empty())
        {
            return Err(Error::K8s(String::from("resources must have names")));
        }

        Ok(())
    }
}

impl Default for K8sModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable Kubernetes report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct K8sReport {
    /// Component name.
    pub component: String,

    /// Kubernetes model.
    pub k8s: K8sModel,

    /// Whether all resources are ready.
    pub ready: bool,
}

impl K8sReport {
    /// Builds a report from a Kubernetes model.
    #[must_use]
    pub fn new(k8s: K8sModel) -> Self {
        let ready = !k8s.resources.is_empty()
            && k8s
                .resources
                .iter()
                .all(|resource| matches!(resource.phase, ResourcePhase::Ready));
        Self {
            component: COMPONENT.to_owned(),
            k8s,
            ready,
        }
    }
}

/// Returns the standard Kubernetes scaffold report.
#[must_use]
pub fn scaffold() -> K8sReport {
    K8sReport::new(K8sModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_ready() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.ready);
        assert_eq!(report.k8s.ready_resources(), 3);
    }

    #[test]
    fn validate_rejects_empty_namespace() {
        let model =
            K8sModel::new("").with_resource(K8sResource::new(ResourceKind::Pod, "pod").ready());

        let error = model.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::K8s(_)));
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = scaffold();
        let encoded = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: K8sReport =
            serde_json::from_str(&encoded).expect("deserialization should succeed");

        assert_eq!(decoded, report);
    }
}
