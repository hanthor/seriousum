//! Request and response types for the REST API endpoints.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Common error and status types
// ============================================================================

/// API error response body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    /// Numeric error code.
    pub code: u32,
    /// Human-readable error message.
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Agent state or health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// The component is healthy.
    #[default]
    Ok,
    /// The component is degraded.
    Warning,
    /// The component has failed.
    Failure,
    /// The component is disabled.
    Disabled,
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warning => write!(f, "warning"),
            Self::Failure => write!(f, "failure"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

/// Status of an individual component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Status {
    /// State the component is in.
    pub state: State,
    /// Human-readable status or warning message.
    pub msg: String,
}

/// Cluster mesh status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ClusterMeshStatus {
    /// Number of global services exported through ClusterMesh.
    pub num_global_services: i64,
    /// Cluster mesh state when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
}

/// Kubernetes integration status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct K8sStatus {
    /// State of the Kubernetes subsystem.
    pub state: State,
    /// Optional Kubernetes status message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    /// Supported Kubernetes API versions.
    #[serde(default)]
    pub k8s_api_versions: Vec<String>,
}

/// IPAM allocation status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IpamStatus {
    /// Allocated IPv4 addresses.
    #[serde(default)]
    pub ipv4: Vec<String>,
    /// Allocated IPv6 addresses.
    #[serde(default)]
    pub ipv6: Vec<String>,
    /// Optional IPAM status message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Full daemon status returned by `GET /healthz`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StatusResponse {
    /// Overall Cilium daemon status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cilium: Option<Status>,
    /// Kubernetes subsystem status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubernetes: Option<K8sStatus>,
    /// ClusterMesh subsystem status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_mesh: Option<ClusterMeshStatus>,
    /// IPAM subsystem status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipam: Option<IpamStatus>,
}

impl StatusResponse {
    /// Create a new status response with the daemon status populated.
    #[must_use]
    pub fn new(state: State, msg: impl Into<String>) -> Self {
        Self {
            cilium: Some(Status {
                state,
                msg: msg.into(),
            }),
            kubernetes: None,
            cluster_mesh: None,
            ipam: None,
        }
    }

    /// Attach Kubernetes status details.
    #[must_use]
    pub fn with_kubernetes(mut self, kubernetes: K8sStatus) -> Self {
        self.kubernetes = Some(kubernetes);
        self
    }

    /// Attach ClusterMesh status details.
    #[must_use]
    pub fn with_cluster_mesh(mut self, cluster_mesh: ClusterMeshStatus) -> Self {
        self.cluster_mesh = Some(cluster_mesh);
        self
    }

    /// Attach IPAM status details.
    #[must_use]
    pub fn with_ipam(mut self, ipam: IpamStatus) -> Self {
        self.ipam = Some(ipam);
        self
    }
}

/// Status of a daemon component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentStatus {
    /// Component status string.
    pub status: String,
    /// Optional component message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ComponentStatus {
    /// Create a healthy component status.
    #[must_use]
    pub fn healthy() -> Self {
        Self {
            status: "healthy".to_string(),
            message: None,
        }
    }

    /// Create a degraded component status.
    #[must_use]
    pub fn degraded(message: impl Into<String>) -> Self {
        Self {
            status: "degraded".to_string(),
            message: Some(message.into()),
        }
    }

    /// Create an unhealthy component status.
    #[must_use]
    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self {
            status: "unhealthy".to_string(),
            message: Some(message.into()),
        }
    }
}

// ============================================================================
// Configuration endpoints
// ============================================================================

/// Daemon configuration.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonConfiguration {
    /// Cluster name.
    pub cluster_name: String,
    /// Local node name.
    pub node_name: String,
    /// eBPF datapath enabled.
    pub ebpf_enabled: bool,
    /// Policy enforcement enabled.
    pub policy_enabled: bool,
    /// Kubernetes integration enabled.
    pub kubernetes_enabled: bool,
    /// Identity management enabled.
    pub identity_enabled: bool,
    /// Load balancer enabled.
    pub loadbalancer_enabled: bool,
    /// DNS proxy enabled.
    pub dns_proxy_enabled: bool,
    /// Observability enabled.
    pub observability_enabled: bool,
    /// Additional configuration options.
    #[serde(default)]
    pub options: HashMap<String, String>,
}

impl Default for DaemonConfiguration {
    fn default() -> Self {
        Self {
            cluster_name: "default".to_string(),
            node_name: "local-node".to_string(),
            ebpf_enabled: true,
            policy_enabled: true,
            kubernetes_enabled: true,
            identity_enabled: true,
            loadbalancer_enabled: true,
            dns_proxy_enabled: true,
            observability_enabled: true,
            options: HashMap::new(),
        }
    }
}

impl DaemonConfiguration {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.cluster_name.is_empty() {
            return Err("cluster_name cannot be empty".to_string());
        }
        if self.node_name.is_empty() {
            return Err("node_name cannot be empty".to_string());
        }
        if self.cluster_name.len() > 253 {
            return Err("cluster_name exceeds maximum length (253 chars)".to_string());
        }
        if self.node_name.len() > 253 {
            return Err("node_name exceeds maximum length (253 chars)".to_string());
        }
        Ok(())
    }
}

/// Configuration update request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigurationSpec {
    /// Cluster name override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    /// Node name override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// eBPF datapath enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebpf_enabled: Option<bool>,
    /// Policy enforcement enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_enabled: Option<bool>,
    /// Kubernetes integration enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubernetes_enabled: Option<bool>,
    /// Identity management enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_enabled: Option<bool>,
    /// Load balancer enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loadbalancer_enabled: Option<bool>,
    /// DNS proxy enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_proxy_enabled: Option<bool>,
    /// Observability enabled override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability_enabled: Option<bool>,
    /// Additional options to merge.
    #[serde(default)]
    pub options: HashMap<String, String>,
}

impl ConfigurationSpec {
    /// Apply this spec to a configuration.
    pub fn apply_to(&self, config: &mut DaemonConfiguration) {
        if let Some(cluster_name) = &self.cluster_name {
            config.cluster_name.clone_from(cluster_name);
        }
        if let Some(node_name) = &self.node_name {
            config.node_name.clone_from(node_name);
        }
        if let Some(ebpf_enabled) = self.ebpf_enabled {
            config.ebpf_enabled = ebpf_enabled;
        }
        if let Some(policy_enabled) = self.policy_enabled {
            config.policy_enabled = policy_enabled;
        }
        if let Some(kubernetes_enabled) = self.kubernetes_enabled {
            config.kubernetes_enabled = kubernetes_enabled;
        }
        if let Some(identity_enabled) = self.identity_enabled {
            config.identity_enabled = identity_enabled;
        }
        if let Some(loadbalancer_enabled) = self.loadbalancer_enabled {
            config.loadbalancer_enabled = loadbalancer_enabled;
        }
        if let Some(dns_proxy_enabled) = self.dns_proxy_enabled {
            config.dns_proxy_enabled = dns_proxy_enabled;
        }
        if let Some(observability_enabled) = self.observability_enabled {
            config.observability_enabled = observability_enabled;
        }
        for (key, value) in &self.options {
            config.options.insert(key.clone(), value.clone());
        }
    }
}

// ============================================================================
// Cluster endpoints
// ============================================================================

/// Cluster node status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterNodeStatus {
    /// Node ID.
    pub id: u32,
    /// Node name.
    pub name: String,
    /// Optional node IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// Node health status string.
    pub status: String,
}

impl ClusterNodeStatus {
    /// Create a new node status.
    #[must_use]
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            ip: None,
            status: "healthy".to_string(),
        }
    }

    /// Set the IP address.
    #[must_use]
    pub fn with_ip(mut self, ip: impl Into<String>) -> Self {
        self.ip = Some(ip.into());
        self
    }

    /// Set the node status string.
    #[must_use]
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }
}

/// Cluster node collection.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ClusterNodes {
    /// List of cluster nodes.
    pub nodes: Vec<ClusterNodeStatus>,
}

// ============================================================================
// Endpoint types
// ============================================================================

/// Endpoint identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointID(pub u16);

/// Endpoint lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointState {
    /// Endpoint creation has started.
    Creating,
    /// Endpoint is waiting for an identity.
    WaitingForIdentity,
    /// Endpoint is not ready yet.
    NotReady,
    /// Endpoint is waiting for a follow-up action.
    Waiting,
    /// Endpoint is ready.
    Ready,
    /// Endpoint is being disconnected.
    Disconnecting,
    /// Endpoint has been disconnected.
    Disconnected,
    /// Endpoint is invalid.
    Invalid,
    /// Endpoint is being restored.
    Restoring,
}

/// Endpoint health information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EndpointHealth {
    /// BPF program health.
    pub bpf: State,
    /// Policy health.
    pub policy: State,
    /// Whether the endpoint is connected.
    pub connected: bool,
    /// Overall endpoint health.
    #[serde(rename = "overallHealth")]
    pub overall_health: State,
}

impl EndpointHealth {
    /// Create an empty endpoint health record.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Endpoint addressing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EndpointAddressing {
    /// IPv4 address.
    #[serde(rename = "ipv4", skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    /// IPv6 address.
    #[serde(rename = "ipv6", skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
    /// IPv4 expiration UUID.
    #[serde(rename = "ipv4ExpirationUUID", skip_serializing_if = "Option::is_none")]
    pub ipv4_expiration_uuid: Option<String>,
    /// IPv6 expiration UUID.
    #[serde(rename = "ipv6ExpirationUUID", skip_serializing_if = "Option::is_none")]
    pub ipv6_expiration_uuid: Option<String>,
}

/// API endpoint model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    /// Endpoint ID.
    pub id: i64,
    /// Optional endpoint state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EndpointState>,
    /// Optional endpoint name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional Kubernetes namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Optional endpoint addressing information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addressing: Option<EndpointAddressing>,
    /// Optional endpoint health information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<EndpointHealth>,
    /// Labels attached to the endpoint.
    #[serde(default)]
    pub labels: Vec<String>,
}

impl Endpoint {
    /// Create a new endpoint.
    #[must_use]
    pub fn new(id: i64) -> Self {
        Self {
            id,
            state: None,
            name: None,
            namespace: None,
            addressing: None,
            health: None,
            labels: Vec::new(),
        }
    }

    /// Set the endpoint state.
    #[must_use]
    pub fn with_state(mut self, state: EndpointState) -> Self {
        self.state = Some(state);
        self
    }

    /// Set the endpoint name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the endpoint namespace.
    #[must_use]
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set endpoint addressing details.
    #[must_use]
    pub fn with_addressing(mut self, addressing: EndpointAddressing) -> Self {
        self.addressing = Some(addressing);
        self
    }

    /// Set endpoint health details.
    #[must_use]
    pub fn with_health(mut self, health: EndpointHealth) -> Self {
        self.health = Some(health);
        self
    }

    /// Set the IPv4 address.
    #[must_use]
    pub fn with_ipv4(mut self, ipv4: impl Into<String>) -> Self {
        let addressing = self
            .addressing
            .get_or_insert_with(EndpointAddressing::default);
        addressing.ipv4 = Some(ipv4.into());
        self
    }

    /// Set the IPv6 address.
    #[must_use]
    pub fn with_ipv6(mut self, ipv6: impl Into<String>) -> Self {
        let addressing = self
            .addressing
            .get_or_insert_with(EndpointAddressing::default);
        addressing.ipv6 = Some(ipv6.into());
        self
    }

    /// Add a label using `key=value` formatting.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(format!("{}={}", key.into(), value.into()));
        self
    }
}

/// `GET /endpoint` response body.
pub type EndpointList = Vec<Endpoint>;

/// `PATCH /endpoint/{id}` request body.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EndpointChangeRequest {
    /// Desired endpoint state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EndpointState>,
    /// Desired endpoint addressing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addressing: Option<EndpointAddressing>,
    /// Desired endpoint labels.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Endpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointConfiguration {
    /// Endpoint status string.
    pub status: String,
    /// Endpoint configuration options.
    pub options: HashMap<String, String>,
}

impl Default for EndpointConfiguration {
    fn default() -> Self {
        Self {
            status: "ready".to_string(),
            options: HashMap::new(),
        }
    }
}

/// Endpoint configuration status response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointConfigurationStatus {
    /// Current endpoint configuration.
    pub configuration: EndpointConfiguration,
}

/// Endpoint configuration update request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointConfigurationSpec {
    /// Endpoint configuration options.
    pub options: HashMap<String, String>,
}

/// Label update request for an endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LabelConfigurationSpec {
    /// User labels to apply.
    #[serde(default)]
    pub user: Vec<String>,
}

/// Label configuration response.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LabelConfiguration {
    /// Labels attached to the endpoint.
    #[serde(default)]
    pub labels: Vec<String>,
}

impl LabelConfiguration {
    /// Create an empty label configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a label using `key=value` formatting.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push(format!("{}={}", key.into(), value.into()));
        self
    }
}

// ============================================================================
// Identity and policy types
// ============================================================================

/// Security identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    /// Numeric identity.
    pub id: i64,
    /// Flat list of identity labels.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Structured label entries when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_array: Option<Vec<LabelEntry>>,
}

/// Structured label entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabelEntry {
    /// Label key.
    pub key: String,
    /// Label value.
    pub value: String,
    /// Label source.
    pub source: String,
}

/// Policy trace selector.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TraceSelector {
    /// Labels to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// Identity to match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<i64>,
}

/// Policy rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Policy {
    /// JSON-encoded policy specification.
    pub policy: String,
    /// Policy repository revision.
    #[serde(default)]
    pub revision: i64,
}

/// Policy verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyVerdict {
    /// The policy allows the traffic.
    Allowed,
    /// The policy denies the traffic.
    Denied,
    /// The policy verdict is unknown.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_serializes() {
        let response = StatusResponse::new(State::Ok, "daemon is running");
        let json = serde_json::to_string(&response).expect("serializes");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(
            decoded.cilium.as_ref().map(|status| status.state),
            Some(State::Ok)
        );
        assert_eq!(
            decoded.cilium.as_ref().map(|status| status.msg.as_str()),
            Some("daemon is running")
        );
    }

    #[test]
    fn daemon_configuration_validates() {
        let config = DaemonConfiguration::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn daemon_configuration_rejects_empty_cluster_name() {
        let config = DaemonConfiguration {
            cluster_name: String::new(),
            ..DaemonConfiguration::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn cluster_node_status_serializes() {
        let node = ClusterNodeStatus::new(1, "node-1")
            .with_ip("10.0.0.1")
            .with_status("healthy");

        let json = serde_json::to_string(&node).expect("serializes");
        let decoded: ClusterNodeStatus = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.name, "node-1");
        assert_eq!(decoded.ip.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn test_endpoint_serialization() {
        let ep = Endpoint {
            id: 42,
            state: Some(EndpointState::Ready),
            name: Some("pod-1".into()),
            namespace: Some("default".into()),
            addressing: Some(EndpointAddressing {
                ipv4: Some("10.0.0.1".into()),
                ..Default::default()
            }),
            health: None,
            labels: vec!["k8s:app=nginx".into()],
        };
        let json = serde_json::to_string(&ep).unwrap();
        let ep2: Endpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(ep2.id, 42);
        assert_eq!(ep2.state, Some(EndpointState::Ready));
        assert_eq!(ep2.labels.len(), 1);
    }

    #[test]
    fn test_endpoint_state_serde() {
        let json = serde_json::to_string(&EndpointState::WaitingForIdentity).unwrap();
        assert_eq!(json, "\"waiting-for-identity\"");
    }

    #[test]
    fn test_api_error_display() {
        let error = ApiError {
            code: 404,
            message: "not found".into(),
        };
        assert!(error.to_string().contains("404"));
    }

    #[test]
    fn test_status_response_default() {
        let status = StatusResponse::default();
        assert!(status.cilium.is_none());
        assert!(status.kubernetes.is_none());
    }

    #[test]
    fn test_policy_verdict_serde() {
        let verdict: PolicyVerdict = serde_json::from_str("\"allowed\"").unwrap();
        assert_eq!(verdict, PolicyVerdict::Allowed);
    }

    #[test]
    fn test_state_display() {
        assert_eq!(State::Ok.to_string(), "ok");
        assert_eq!(State::Failure.to_string(), "failure");
    }

    #[test]
    fn test_label_entry_roundtrip() {
        let entry = LabelEntry {
            key: "app".into(),
            value: "nginx".into(),
            source: "k8s".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let entry2: LabelEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry2.key, "app");
        assert_eq!(entry2.source, "k8s");
    }

    #[test]
    fn test_tls_config_mtls() {
        let spec = EndpointChangeRequest {
            state: Some(EndpointState::Ready),
            ..Default::default()
        };
        assert!(spec.labels.is_empty());
    }

    #[test]
    fn configuration_spec_applies_to_config() {
        let mut config = DaemonConfiguration::default();
        let spec = ConfigurationSpec {
            cluster_name: Some("prod".to_string()),
            policy_enabled: Some(false),
            ..Default::default()
        };

        spec.apply_to(&mut config);

        assert_eq!(config.cluster_name, "prod");
        assert!(!config.policy_enabled);
    }

    #[test]
    fn label_configuration_builds() {
        let labels = LabelConfiguration::new()
            .with_label("app", "backend")
            .with_label("tier", "api");

        assert_eq!(
            labels.labels.first().map(String::as_str),
            Some("app=backend")
        );
        assert_eq!(labels.labels.get(1).map(String::as_str), Some("tier=api"));
    }
}
