//! Request and response types for the REST API endpoints.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Health / Status endpoints
// ============================================================================

/// Health status response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusResponse {
    /// Overall daemon health status
    pub status: String,
    /// Human-readable message
    pub message: String,
    /// Detailed component statuses
    pub components: HashMap<String, ComponentStatus>,
}

impl StatusResponse {
    /// Create a new status response.
    pub fn new(status: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            message: message.into(),
            components: HashMap::new(),
        }
    }

    /// Add a component status.
    pub fn with_component(
        mut self,
        name: impl Into<String>,
        status: ComponentStatus,
    ) -> Self {
        self.components.insert(name.into(), status);
        self
    }
}

/// Status of a daemon component
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentStatus {
    /// Component status (healthy, degraded, unhealthy)
    pub status: String,
    /// Optional message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ComponentStatus {
    /// Create a healthy component status.
    pub fn healthy() -> Self {
        Self {
            status: "healthy".to_string(),
            message: None,
        }
    }

    /// Create a degraded component status.
    pub fn degraded(message: impl Into<String>) -> Self {
        Self {
            status: "degraded".to_string(),
            message: Some(message.into()),
        }
    }

    /// Create an unhealthy component status.
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

/// Daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonConfiguration {
    /// Cluster name
    pub cluster_name: String,
    /// Local node name
    pub node_name: String,
    /// eBPF datapath enabled
    pub ebpf_enabled: bool,
    /// Policy enforcement enabled
    pub policy_enabled: bool,
    /// Kubernetes integration enabled
    pub kubernetes_enabled: bool,
    /// Identity management enabled
    pub identity_enabled: bool,
    /// Load balancer enabled
    pub loadbalancer_enabled: bool,
    /// DNS proxy enabled
    pub dns_proxy_enabled: bool,
    /// Observability (Hubble) enabled
    pub observability_enabled: bool,
    /// Additional configuration options
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

/// Configuration update request
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigurationSpec {
    /// Cluster name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    /// Node name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    /// eBPF datapath enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebpf_enabled: Option<bool>,
    /// Policy enforcement enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_enabled: Option<bool>,
    /// Kubernetes integration enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubernetes_enabled: Option<bool>,
    /// Identity management enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_enabled: Option<bool>,
    /// Load balancer enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loadbalancer_enabled: Option<bool>,
    /// DNS proxy enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_proxy_enabled: Option<bool>,
    /// Observability enabled (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability_enabled: Option<bool>,
    /// Additional options
    #[serde(default)]
    pub options: HashMap<String, String>,
}

impl ConfigurationSpec {
    /// Apply this spec to a configuration.
    pub fn apply_to(&self, config: &mut DaemonConfiguration) {
        if let Some(cluster_name) = &self.cluster_name {
            config.cluster_name = cluster_name.clone();
        }
        if let Some(node_name) = &self.node_name {
            config.node_name = node_name.clone();
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
        for (k, v) in &self.options {
            config.options.insert(k.clone(), v.clone());
        }
    }
}

// ============================================================================
// Cluster endpoints
// ============================================================================

/// Cluster node status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClusterNodeStatus {
    /// Node ID
    pub id: u32,
    /// Node name
    pub name: String,
    /// Node IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    /// Node health status
    pub status: String,
}

impl ClusterNodeStatus {
    /// Create a new node status.
    pub fn new(id: u32, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            ip: None,
            status: "healthy".to_string(),
        }
    }

    /// Set the IP address.
    pub fn with_ip(mut self, ip: impl Into<String>) -> Self {
        self.ip = Some(ip.into());
        self
    }

    /// Set the status.
    pub fn with_status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }
}

/// Cluster nodes information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterNodes {
    /// List of node statuses
    pub nodes: Vec<ClusterNodeStatus>,
}


// ============================================================================
// Endpoint types
// ============================================================================

/// Endpoint identity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointID(pub u16);

/// Endpoint status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    /// Endpoint ID
    pub id: u16,
    /// Endpoint name
    pub name: String,
    /// Endpoint status
    pub status: String,
    /// Container ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// Pod name (K8s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_name: Option<String>,
    /// Pod namespace (K8s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_namespace: Option<String>,
    /// IPv4 address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    /// IPv6 address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
    /// MAC address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    /// Labels
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

impl Endpoint {
    /// Create a new endpoint.
    pub fn new(id: u16, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            status: "ready".to_string(),
            container_id: None,
            pod_name: None,
            pod_namespace: None,
            ipv4: None,
            ipv6: None,
            mac: None,
            labels: HashMap::new(),
        }
    }

    /// Set the container ID.
    pub fn with_container_id(mut self, container_id: impl Into<String>) -> Self {
        self.container_id = Some(container_id.into());
        self
    }

    /// Set the pod information.
    pub fn with_pod(
        mut self,
        name: impl Into<String>,
        namespace: impl Into<String>,
    ) -> Self {
        self.pod_name = Some(name.into());
        self.pod_namespace = Some(namespace.into());
        self
    }

    /// Set the IPv4 address.
    pub fn with_ipv4(mut self, ipv4: impl Into<String>) -> Self {
        self.ipv4 = Some(ipv4.into());
        self
    }

    /// Set the IPv6 address.
    pub fn with_ipv6(mut self, ipv6: impl Into<String>) -> Self {
        self.ipv6 = Some(ipv6.into());
        self
    }

    /// Set a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Endpoint change request
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EndpointChangeRequest {
    /// Endpoint name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Container ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// Pod name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_name: Option<String>,
    /// Pod namespace (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_namespace: Option<String>,
    /// IPv4 address (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    /// IPv6 address (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
    /// MAC address (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    /// Labels
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Endpoint configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EndpointConfiguration {
    /// Endpoint status
    pub status: String,
    /// Configuration options
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

/// Endpoint configuration status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfigurationStatus {
    /// Current endpoint configuration
    pub configuration: EndpointConfiguration,
}

/// Endpoint configuration spec for updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfigurationSpec {
    /// Configuration options
    pub options: HashMap<String, String>,
}

/// Label configuration for endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelConfiguration {
    /// Endpoint labels
    pub labels: HashMap<String, String>,
}

impl LabelConfiguration {
    /// Create a new label configuration.
    pub fn new() -> Self {
        Self {
            labels: HashMap::new(),
        }
    }

    /// Add a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

impl Default for LabelConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_serializes() {
        let response = StatusResponse::new("ok", "daemon is running");
        let json = serde_json::to_string(&response).expect("serializes");
        let decoded: StatusResponse = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(decoded.status, "ok");
        assert_eq!(decoded.message, "daemon is running");
    }

    #[test]
    fn daemon_configuration_validates() {
        let config = DaemonConfiguration::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn daemon_configuration_rejects_empty_cluster_name() {
        let mut config = DaemonConfiguration::default();
        config.cluster_name = String::new();
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
    fn endpoint_serializes() {
        let endpoint = Endpoint::new(100, "pod-1")
            .with_ipv4("10.1.1.1")
            .with_label("app", "web");

        let json = serde_json::to_string(&endpoint).expect("serializes");
        let decoded: Endpoint = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(decoded.id, 100);
        assert_eq!(decoded.name, "pod-1");
        assert_eq!(decoded.ipv4.as_deref(), Some("10.1.1.1"));
        assert_eq!(decoded.labels.get("app").map(|s| s.as_str()), Some("web"));
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

        assert_eq!(labels.labels.get("app").map(|s| s.as_str()), Some("backend"));
        assert_eq!(labels.labels.get("tier").map(|s| s.as_str()), Some("api"));
    }
}
