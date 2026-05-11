//! Endpoint status models for Track U.
//! 
//! Provides endpoint status information and reporting.

use serde::{Deserialize, Serialize};

/// Status information for a single endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointStatus {
    /// Endpoint name.
    pub name: String,

    /// Pod name associated with the endpoint.
    pub pod_name: String,

    /// Kubernetes namespace.
    pub namespace: String,

    /// Endpoint status (e.g., "ready", "not-ready", "error").
    pub status: String,

    /// IP address of the endpoint (if applicable).
    pub ip_address: Option<String>,
}

impl EndpointStatus {
    /// Check if this endpoint is ready.
    pub fn is_ready(&self) -> bool {
        self.status == "ready"
    }

    /// Get a summary of this endpoint.
    pub fn summary(&self) -> String {
        format!(
            "{}/{} ({})",
            self.namespace, self.pod_name, self.status
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_status_creation() {
        let ep = EndpointStatus {
            name: "test-ep".to_string(),
            pod_name: "test-pod".to_string(),
            namespace: "default".to_string(),
            status: "ready".to_string(),
            ip_address: Some("10.0.1.5".to_string()),
        };

        assert_eq!(ep.name, "test-ep");
        assert_eq!(ep.pod_name, "test-pod");
    }

    #[test]
    fn test_endpoint_status_is_ready() {
        let ready_ep = EndpointStatus {
            name: "ready".to_string(),
            pod_name: "pod".to_string(),
            namespace: "default".to_string(),
            status: "ready".to_string(),
            ip_address: None,
        };

        let not_ready_ep = EndpointStatus {
            name: "not-ready".to_string(),
            pod_name: "pod".to_string(),
            namespace: "default".to_string(),
            status: "not-ready".to_string(),
            ip_address: None,
        };

        assert!(ready_ep.is_ready());
        assert!(!not_ready_ep.is_ready());
    }

    #[test]
    fn test_endpoint_status_summary() {
        let ep = EndpointStatus {
            name: "test-ep".to_string(),
            pod_name: "test-pod".to_string(),
            namespace: "default".to_string(),
            status: "ready".to_string(),
            ip_address: Some("10.0.1.5".to_string()),
        };

        let summary = ep.summary();
        assert!(summary.contains("default"));
        assert!(summary.contains("test-pod"));
        assert!(summary.contains("ready"));
    }

    #[test]
    fn test_endpoint_status_serialization() {
        let ep = EndpointStatus {
            name: "test-ep".to_string(),
            pod_name: "test-pod".to_string(),
            namespace: "default".to_string(),
            status: "ready".to_string(),
            ip_address: Some("10.0.1.5".to_string()),
        };

        let json = serde_json::to_string(&ep).expect("serialize");
        assert!(json.contains("\"name\":\"test-ep\""));
        assert!(json.contains("\"pod_name\":\"test-pod\""));
    }

    #[test]
    fn test_endpoint_status_deserialization() {
        let json = r#"{"name":"test-ep","pod_name":"test-pod","namespace":"default","status":"ready","ip_address":"10.0.1.5"}"#;
        let ep: EndpointStatus = serde_json::from_str(json).expect("deserialize");

        assert_eq!(ep.name, "test-ep");
        assert_eq!(ep.pod_name, "test-pod");
        assert_eq!(ep.status, "ready");
    }
}
