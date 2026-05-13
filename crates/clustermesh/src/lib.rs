//! Pure ClusterMesh data types and in-memory coordination primitives.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

const CLUSTER_NAME_MAX_LENGTH: usize = 32;
const DEFAULT_MAX_CONNECTED_CLUSTERS: u8 = 255;

/// Cilium cluster ID (0-255, where 0 identifies the local cluster).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClusterID(pub u8);

impl ClusterID {
    /// Cluster ID used for the local cluster.
    pub const LOCAL: Self = Self(0);

    /// Returns whether this ID refers to the local cluster.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        self.0 == Self::LOCAL.0
    }
}

/// Validated ClusterMesh cluster name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClusterName(pub String);

impl ClusterName {
    /// Creates a validated cluster name.
    pub fn new(name: impl Into<String>) -> Result<Self, ClusterMeshError> {
        let name = name.into();
        let bytes = name.as_bytes();

        let is_valid = !name.is_empty()
            && bytes.len() <= CLUSTER_NAME_MAX_LENGTH
            && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');

        if is_valid {
            Ok(Self(name))
        } else {
            Err(ClusterMeshError::InvalidClusterName(name))
        }
    }

    /// Returns the cluster name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClusterName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error type for pure ClusterMesh validation and registry operations.
#[derive(Debug, Error)]
pub enum ClusterMeshError {
    /// Returned when a cluster name violates ClusterMesh naming rules.
    #[error("cluster name too long or invalid: {0}")]
    InvalidClusterName(String),

    /// Returned when a new remote cluster would exceed the configured limit.
    #[error("max connected clusters ({0}) reached")]
    MaxClustersReached(u8),

    /// Returned when a requested cluster does not exist.
    #[error("cluster {0} not found")]
    ClusterNotFound(u8),
}

/// Current lifecycle state of a remote cluster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteClusterStatus {
    /// Initial state before a remote cluster is usable.
    Connecting,

    /// Connected and actively receiving state.
    Connected,

    /// Connectivity was lost and the cluster is retrying.
    Disconnected,

    /// The cluster was permanently removed.
    Removed,
}

/// In-memory summary of a peered cluster.
#[derive(Debug, Clone)]
pub struct RemoteCluster {
    /// Unique remote cluster identifier.
    pub id: ClusterID,

    /// Human-readable validated cluster name.
    pub name: ClusterName,

    /// Current cluster lifecycle status.
    pub status: RemoteClusterStatus,

    /// Number of known nodes in this cluster.
    pub node_count: u32,

    /// Number of known identities from this cluster.
    pub identity_count: u32,

    /// Number of known endpoints from this cluster.
    pub endpoint_count: u32,

    /// Number of known services from this cluster.
    pub service_count: u32,
}

impl RemoteCluster {
    /// Creates a new remote cluster in the connecting state.
    #[must_use]
    pub fn new(id: ClusterID, name: ClusterName) -> Self {
        Self {
            id,
            name,
            status: RemoteClusterStatus::Connecting,
            node_count: 0,
            identity_count: 0,
            endpoint_count: 0,
            service_count: 0,
        }
    }

    /// Returns whether the cluster is ready for traffic/state consumption.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.status == RemoteClusterStatus::Connected
    }

    /// Marks the cluster as connected.
    pub fn mark_connected(&mut self) {
        self.status = RemoteClusterStatus::Connected;
    }

    /// Marks the cluster as disconnected.
    pub fn mark_disconnected(&mut self) {
        self.status = RemoteClusterStatus::Disconnected;
    }
}

/// Key for a global service across all clusters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlobalServiceKey {
    /// Origin cluster for the service definition.
    pub cluster: ClusterName,

    /// Kubernetes namespace of the service.
    pub namespace: String,

    /// Service name.
    pub name: String,
}

impl GlobalServiceKey {
    /// Creates a new global service key.
    #[must_use]
    pub fn new(
        cluster: impl Into<String>,
        namespace: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            cluster: ClusterName(cluster.into()),
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// Creates a key for a local service.
    #[must_use]
    pub fn local(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new("local", namespace, name)
    }
}

impl fmt::Display for GlobalServiceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.cluster, self.namespace, self.name)
    }
}

/// Single backend of a global service.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlobalServiceBackend {
    /// Cluster contributing this backend.
    pub cluster: ClusterName,

    /// IP address of the backend.
    pub ip: IpAddr,

    /// Service port exposed by the backend.
    pub port: u16,
}

/// Local ClusterMesh configuration for pure in-memory state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMeshConfig {
    /// Local cluster ID.
    pub cluster_id: ClusterID,

    /// Local cluster name.
    pub cluster_name: ClusterName,

    /// Maximum number of remote clusters to peer with.
    pub max_connected_clusters: u8,

    /// Whether to enable global service load balancing.
    pub enable_global_services: bool,

    /// Whether to share local endpoints to remote clusters.
    pub enable_external_workloads: bool,
}

impl ClusterMeshConfig {
    /// Creates a default local-only configuration.
    #[must_use]
    pub fn default_local(name: ClusterName) -> Self {
        Self {
            cluster_id: ClusterID::LOCAL,
            cluster_name: name,
            max_connected_clusters: DEFAULT_MAX_CONNECTED_CLUSTERS,
            enable_global_services: true,
            enable_external_workloads: false,
        }
    }
}

/// In-memory registry of remote clusters and global service backends.
#[derive(Debug, Clone)]
pub struct ClusterMesh {
    config: ClusterMeshConfig,
    clusters: HashMap<ClusterID, RemoteCluster>,
    global_services: HashMap<GlobalServiceKey, Vec<GlobalServiceBackend>>,
}

impl ClusterMesh {
    /// Creates an empty ClusterMesh registry.
    #[must_use]
    pub fn new(config: ClusterMeshConfig) -> Self {
        Self {
            config,
            clusters: HashMap::new(),
            global_services: HashMap::new(),
        }
    }

    /// Registers a remote cluster.
    pub fn add_remote_cluster(&mut self, cluster: RemoteCluster) -> Result<(), ClusterMeshError> {
        let already_present = self.clusters.contains_key(&cluster.id);
        if !already_present
            && self.clusters.len() >= usize::from(self.config.max_connected_clusters)
        {
            return Err(ClusterMeshError::MaxClustersReached(
                self.config.max_connected_clusters,
            ));
        }

        debug!(cluster_id = cluster.id.0, cluster_name = %cluster.name, "registering remote cluster");
        self.clusters.insert(cluster.id, cluster);
        Ok(())
    }

    /// Removes a remote cluster and all of its global service backends.
    pub fn remove_remote_cluster(&mut self, id: ClusterID) {
        if let Some(cluster) = self.clusters.remove(&id) {
            debug!(cluster_id = id.0, cluster_name = %cluster.name, "removing remote cluster");
            self.remove_global_service_backends_for_cluster(&cluster.name);
        }
    }

    /// Returns a cluster by ID.
    #[must_use]
    pub fn get_cluster(&self, id: ClusterID) -> Option<&RemoteCluster> {
        self.clusters.get(&id)
    }

    /// Returns all connected remote clusters.
    pub fn connected_clusters(&self) -> impl Iterator<Item = &RemoteCluster> + '_ {
        self.clusters.values().filter(|cluster| cluster.is_ready())
    }

    /// Returns the number of tracked remote clusters.
    #[must_use]
    pub fn cluster_count(&self) -> usize {
        self.clusters.len()
    }

    /// Registers a backend for a global service.
    pub fn add_global_service_backend(
        &mut self,
        key: GlobalServiceKey,
        backend: GlobalServiceBackend,
    ) {
        if !self.config.enable_global_services {
            debug!(service = %key, "ignoring global service backend because feature is disabled");
            return;
        }

        let backends = self.global_services.entry(key).or_default();
        if !backends.contains(&backend) {
            debug!(cluster = %backend.cluster, backend_ip = %backend.ip, backend_port = backend.port, "adding global service backend");
            backends.push(backend);
        }
    }

    /// Removes all backends contributed by a specific cluster.
    pub fn remove_global_service_backends_for_cluster(&mut self, cluster: &ClusterName) {
        debug!(cluster = %cluster, "removing global service backends for cluster");
        self.global_services.retain(|_, backends| {
            backends.retain(|backend| &backend.cluster != cluster);
            !backends.is_empty()
        });
    }

    /// Returns all backends registered for a global service.
    #[must_use]
    pub fn global_service_backends(&self, key: &GlobalServiceKey) -> &[GlobalServiceBackend] {
        self.global_services
            .get(key)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns all backends across clusters for the given namespace/name.
    #[must_use]
    pub fn all_backends_for_service(&self, ns: &str, name: &str) -> Vec<&GlobalServiceBackend> {
        self.global_services
            .iter()
            .filter(|(key, _)| key.namespace == ns && key.name == name)
            .flat_map(|(_, backends)| backends.iter())
            .collect()
    }
}

/// Returns a lightweight textual summary for the scaffold binary entrypoint.
pub fn run() -> Result<String, ClusterMeshError> {
    let config = ClusterMeshConfig::default_local(ClusterName::new("local")?);
    let mesh = ClusterMesh::new(config);
    Ok(format!(
        "ClusterMesh({}, {} remote clusters, {} global services)",
        mesh.config.cluster_name,
        mesh.cluster_count(),
        mesh.global_services.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_name_validation() {
        assert!(ClusterName::new("valid-cluster").is_ok());
        assert!(ClusterName::new("a".repeat(33)).is_err());
    }

    #[test]
    fn test_add_remove_remote_cluster() {
        let config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        let mut mesh = ClusterMesh::new(config);
        let rc = RemoteCluster::new(ClusterID(1), ClusterName::new("remote").unwrap());
        mesh.add_remote_cluster(rc).unwrap();
        assert_eq!(mesh.cluster_count(), 1);
        mesh.remove_remote_cluster(ClusterID(1));
        assert_eq!(mesh.cluster_count(), 0);
    }

    #[test]
    fn test_max_clusters_limit() {
        let mut config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        config.max_connected_clusters = 1;
        let mut mesh = ClusterMesh::new(config);
        let r1 = RemoteCluster::new(ClusterID(1), ClusterName::new("c1").unwrap());
        let r2 = RemoteCluster::new(ClusterID(2), ClusterName::new("c2").unwrap());
        mesh.add_remote_cluster(r1).unwrap();
        assert!(mesh.add_remote_cluster(r2).is_err());
    }

    #[test]
    fn test_global_service_backends() {
        let config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        let mut mesh = ClusterMesh::new(config);
        let key = GlobalServiceKey::new("remote", "default", "nginx");
        let backend = GlobalServiceBackend {
            cluster: ClusterName::new("remote").unwrap(),
            ip: "10.0.0.1".parse().unwrap(),
            port: 80,
        };
        mesh.add_global_service_backend(key.clone(), backend);
        assert_eq!(mesh.global_service_backends(&key).len(), 1);
    }

    #[test]
    fn test_remove_backends_for_cluster() {
        let config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        let mut mesh = ClusterMesh::new(config);
        let cluster = ClusterName::new("remote").unwrap();
        let key = GlobalServiceKey::new("remote", "default", "svc");
        mesh.add_global_service_backend(
            key.clone(),
            GlobalServiceBackend {
                cluster: cluster.clone(),
                ip: "10.0.0.1".parse().unwrap(),
                port: 80,
            },
        );
        mesh.remove_global_service_backends_for_cluster(&cluster);
        assert_eq!(mesh.global_service_backends(&key).len(), 0);
    }

    #[test]
    fn test_connected_clusters_filter() {
        let config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        let mut mesh = ClusterMesh::new(config);
        let mut r1 = RemoteCluster::new(ClusterID(1), ClusterName::new("r1").unwrap());
        r1.mark_connected();
        let r2 = RemoteCluster::new(ClusterID(2), ClusterName::new("r2").unwrap());
        mesh.add_remote_cluster(r1).unwrap();
        mesh.add_remote_cluster(r2).unwrap();
        assert_eq!(mesh.connected_clusters().count(), 1);
    }

    #[test]
    fn test_all_backends_for_service_across_clusters() {
        let config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        let mut mesh = ClusterMesh::new(config);

        mesh.add_global_service_backend(
            GlobalServiceKey::local("default", "api"),
            GlobalServiceBackend {
                cluster: ClusterName::new("local").unwrap(),
                ip: "10.0.0.1".parse().unwrap(),
                port: 80,
            },
        );
        mesh.add_global_service_backend(
            GlobalServiceKey::new("remote", "default", "api"),
            GlobalServiceBackend {
                cluster: ClusterName::new("remote").unwrap(),
                ip: "10.0.0.2".parse().unwrap(),
                port: 80,
            },
        );

        assert_eq!(mesh.all_backends_for_service("default", "api").len(), 2);
    }

    #[test]
    fn test_remove_remote_cluster_removes_associated_backends() {
        let config = ClusterMeshConfig::default_local(ClusterName::new("local").unwrap());
        let mut mesh = ClusterMesh::new(config);
        let cluster = ClusterName::new("remote").unwrap();
        let key = GlobalServiceKey::new("remote", "default", "svc");

        mesh.add_remote_cluster(RemoteCluster::new(ClusterID(1), cluster.clone()))
            .unwrap();
        mesh.add_global_service_backend(
            key.clone(),
            GlobalServiceBackend {
                cluster,
                ip: "10.0.0.3".parse().unwrap(),
                port: 8080,
            },
        );

        mesh.remove_remote_cluster(ClusterID(1));

        assert!(mesh.get_cluster(ClusterID(1)).is_none());
        assert!(mesh.global_service_backends(&key).is_empty());
    }
}
