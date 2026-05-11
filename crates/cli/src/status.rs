//! Status collection and reporting for Track U.
//! 
//! Provides status collection for clusters, nodes, endpoints, and services.

use serde::{Deserialize, Serialize};
use crate::Result;

/// Overall cluster status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterStatus {
    /// Number of nodes in the cluster.
    pub node_count: usize,

    /// Number of endpoints managed.
    pub endpoint_count: usize,

    /// Whether the cluster is healthy.
    pub is_healthy: bool,

    /// Number of pods running.
    pub pod_count: usize,

    /// Status message.
    pub status_message: String,
}

/// Status information for a service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Service name.
    pub name: String,

    /// Service namespace.
    pub namespace: String,

    /// Service type (ClusterIP, NodePort, LoadBalancer).
    pub service_type: String,

    /// Number of backend endpoints.
    pub backend_count: usize,

    /// Whether the service is healthy.
    pub is_healthy: bool,

    /// IP address of the service (if applicable).
    pub cluster_ip: Option<String>,
}

/// Status collector for gathering cluster and service status.
pub struct StatusCollector;

impl StatusCollector {
    /// Create a new status collector.
    pub fn new() -> Self {
        Self
    }

    /// Collect overall cluster status.
    pub fn collect_cluster_status(&self) -> Result<ClusterStatus> {
        // Simulate cluster status collection
        Ok(ClusterStatus {
            node_count: 3,
            endpoint_count: 12,
            is_healthy: true,
            pod_count: 25,
            status_message: "Cluster is healthy and operational".to_string(),
        })
    }

    /// Collect endpoint status, optionally filtered by namespace and pod name.
    pub fn collect_endpoint_status(
        &self,
        namespace: Option<String>,
        pod_name: Option<String>,
    ) -> Result<Vec<crate::endpoint::EndpointStatus>> {
        use crate::endpoint::EndpointStatus;

        let mut endpoints = vec![
            EndpointStatus {
                name: "client-1".to_string(),
                pod_name: "client-pod-1".to_string(),
                namespace: "default".to_string(),
                status: "ready".to_string(),
                ip_address: Some("10.0.1.5".to_string()),
            },
            EndpointStatus {
                name: "server-1".to_string(),
                pod_name: "server-pod-1".to_string(),
                namespace: "default".to_string(),
                status: "ready".to_string(),
                ip_address: Some("10.0.1.6".to_string()),
            },
            EndpointStatus {
                name: "worker-1".to_string(),
                pod_name: "worker-pod-1".to_string(),
                namespace: "kube-system".to_string(),
                status: "ready".to_string(),
                ip_address: Some("10.0.2.3".to_string()),
            },
        ];

        // Filter by namespace if provided
        if let Some(ns) = namespace {
            endpoints.retain(|ep| ep.namespace == ns);
        }

        // Filter by pod name if provided
        if let Some(pn) = pod_name {
            endpoints.retain(|ep| ep.pod_name == pn);
        }

        Ok(endpoints)
    }

    /// Collect service status, optionally filtered by namespace.
    pub fn collect_service_status(&self, namespace: Option<String>) -> Result<Vec<ServiceStatus>> {
        let mut services = vec![
            ServiceStatus {
                name: "kubernetes".to_string(),
                namespace: "default".to_string(),
                service_type: "ClusterIP".to_string(),
                backend_count: 3,
                is_healthy: true,
                cluster_ip: Some("10.96.0.1".to_string()),
            },
            ServiceStatus {
                name: "cilium-agent".to_string(),
                namespace: "kube-system".to_string(),
                service_type: "ClusterIP".to_string(),
                backend_count: 3,
                is_healthy: true,
                cluster_ip: Some("10.96.1.1".to_string()),
            },
            ServiceStatus {
                name: "test-service".to_string(),
                namespace: "default".to_string(),
                service_type: "NodePort".to_string(),
                backend_count: 2,
                is_healthy: true,
                cluster_ip: Some("10.96.2.1".to_string()),
            },
        ];

        // Filter by namespace if provided
        if let Some(ns) = namespace {
            services.retain(|svc| svc.namespace == ns);
        }

        Ok(services)
    }
}

impl Default for StatusCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_collector_creation() {
        let _collector = StatusCollector::new();
    }

    #[test]
    fn test_collect_cluster_status() {
        let collector = StatusCollector::new();
        let status = collector.collect_cluster_status().expect("collect cluster status");

        assert_eq!(status.node_count, 3);
        assert_eq!(status.endpoint_count, 12);
        assert!(status.is_healthy);
        assert_eq!(status.pod_count, 25);
    }

    #[test]
    fn test_collect_endpoint_status_all() {
        let collector = StatusCollector::new();
        let endpoints = collector
            .collect_endpoint_status(None, None)
            .expect("collect endpoints");

        assert_eq!(endpoints.len(), 3);
        assert!(endpoints.iter().all(|e| e.status == "ready"));
    }

    #[test]
    fn test_collect_endpoint_status_filter_by_namespace() {
        let collector = StatusCollector::new();
        let endpoints = collector
            .collect_endpoint_status(Some("default".to_string()), None)
            .expect("collect endpoints");

        assert_eq!(endpoints.len(), 2);
        assert!(endpoints.iter().all(|e| e.namespace == "default"));
    }

    #[test]
    fn test_collect_endpoint_status_filter_by_pod_name() {
        let collector = StatusCollector::new();
        let endpoints = collector
            .collect_endpoint_status(None, Some("client-pod-1".to_string()))
            .expect("collect endpoints");

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].pod_name, "client-pod-1");
    }

    #[test]
    fn test_collect_endpoint_status_combined_filter() {
        let collector = StatusCollector::new();
        let endpoints = collector
            .collect_endpoint_status(
                Some("default".to_string()),
                Some("client-pod-1".to_string()),
            )
            .expect("collect endpoints");

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].namespace, "default");
        assert_eq!(endpoints[0].pod_name, "client-pod-1");
    }

    #[test]
    fn test_collect_service_status_all() {
        let collector = StatusCollector::new();
        let services = collector
            .collect_service_status(None)
            .expect("collect services");

        assert_eq!(services.len(), 3);
        assert!(services.iter().all(|s| s.is_healthy));
    }

    #[test]
    fn test_collect_service_status_filter_by_namespace() {
        let collector = StatusCollector::new();
        let services = collector
            .collect_service_status(Some("kube-system".to_string()))
            .expect("collect services");

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "cilium-agent");
    }

    #[test]
    fn test_collect_service_status_default_namespace() {
        let collector = StatusCollector::new();
        let services = collector
            .collect_service_status(Some("default".to_string()))
            .expect("collect services");

        assert_eq!(services.len(), 2);
        assert!(services.iter().all(|s| s.namespace == "default"));
    }

    #[test]
    fn test_cluster_status_serialization() {
        let status = ClusterStatus {
            node_count: 3,
            endpoint_count: 12,
            is_healthy: true,
            pod_count: 25,
            status_message: "ok".to_string(),
        };

        let json = serde_json::to_string(&status).expect("serialize");
        assert!(json.contains("\"node_count\":3"));
        assert!(json.contains("\"is_healthy\":true"));
    }

    #[test]
    fn test_service_status_serialization() {
        let service = ServiceStatus {
            name: "test-svc".to_string(),
            namespace: "default".to_string(),
            service_type: "ClusterIP".to_string(),
            backend_count: 2,
            is_healthy: true,
            cluster_ip: Some("10.96.0.1".to_string()),
        };

        let json = serde_json::to_string(&service).expect("serialize");
        assert!(json.contains("\"name\":\"test-svc\""));
        assert!(json.contains("\"backend_count\":2"));
    }

    #[test]
    fn test_service_status_types() {
        let services = vec![
            ServiceStatus {
                name: "cluster-ip-svc".to_string(),
                namespace: "default".to_string(),
                service_type: "ClusterIP".to_string(),
                backend_count: 1,
                is_healthy: true,
                cluster_ip: Some("10.96.0.1".to_string()),
            },
            ServiceStatus {
                name: "node-port-svc".to_string(),
                namespace: "default".to_string(),
                service_type: "NodePort".to_string(),
                backend_count: 2,
                is_healthy: true,
                cluster_ip: Some("10.96.0.2".to_string()),
            },
            ServiceStatus {
                name: "lb-svc".to_string(),
                namespace: "default".to_string(),
                service_type: "LoadBalancer".to_string(),
                backend_count: 3,
                is_healthy: true,
                cluster_ip: Some("10.96.0.3".to_string()),
            },
        ];

        let types: Vec<_> = services.iter().map(|s| s.service_type.as_str()).collect();
        assert!(types.contains(&"ClusterIP"));
        assert!(types.contains(&"NodePort"));
        assert!(types.contains(&"LoadBalancer"));
    }
}
