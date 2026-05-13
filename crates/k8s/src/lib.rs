//! Pure Kubernetes resource and watcher types for offline and unit-test use.

pub mod watcher;

pub use watcher::{K8sEvent, K8sWatcher, WatcherError};

use std::{collections::HashMap, net::IpAddr, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};
use tracing::debug;

/// Kubernetes object metadata used by slim resource types.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ObjectMeta {
    /// Object name.
    pub name: String,
    /// Object namespace.
    pub namespace: String,
    /// Kubernetes UID.
    pub uid: String,
    /// Opaque resource version.
    pub resource_version: String,
    /// Object labels.
    pub labels: HashMap<String, String>,
    /// Object annotations.
    pub annotations: HashMap<String, String>,
}

impl ObjectMeta {
    /// Creates object metadata with empty UID, resource version, labels, and annotations.
    #[must_use]
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
            uid: String::new(),
            resource_version: String::new(),
            labels: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    /// Returns the `namespace/name` identifier for the object.
    #[must_use]
    pub fn namespaced_name(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }

    /// Returns whether the object has the given label key and value.
    #[must_use]
    pub fn has_label(&self, key: &str, value: &str) -> bool {
        self.labels.get(key).is_some_and(|current| current == value)
    }
}

/// Slim Kubernetes pod containing only fields Cilium commonly consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pod {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Pod scheduling and runtime spec.
    pub spec: PodSpec,
    /// Pod status observed from Kubernetes.
    pub status: PodStatus,
}

/// Slim pod specification.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PodSpec {
    /// Name of the node the pod is scheduled to.
    pub node_name: String,
    /// Whether the pod uses the host network namespace.
    pub host_network: bool,
    /// Service account name attached to the pod.
    pub service_account_name: String,
}

/// Slim pod status.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PodStatus {
    /// Primary pod IP.
    pub pod_ip: Option<IpAddr>,
    /// Host IP for the node running the pod.
    pub host_ip: Option<IpAddr>,
    /// Lifecycle phase for the pod.
    pub phase: PodPhase,
}

/// High-level lifecycle phase for a pod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PodPhase {
    /// Kubernetes did not report a known phase.
    #[default]
    Unknown,
    /// Pod has been accepted but is not yet running.
    Pending,
    /// Pod is running on a node.
    Running,
    /// Pod completed successfully.
    Succeeded,
    /// Pod completed with failure.
    Failed,
}

/// Slim Kubernetes node containing only fields used by higher-level logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct K8sNode {
    /// Object metadata.
    pub meta: ObjectMeta,
    /// Node configuration.
    pub spec: K8sNodeSpec,
    /// Node status.
    pub status: K8sNodeStatus,
}

/// Slim node specification.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct K8sNodeSpec {
    /// Single pod CIDR assigned to the node.
    pub pod_cidr: Option<String>,
    /// All pod CIDRs assigned to the node.
    pub pod_cidrs: Vec<String>,
    /// Provider identifier in `<provider>://<id>` format.
    pub provider_id: String,
    /// Node taints.
    pub taints: Vec<Taint>,
}

/// Kubernetes taint applied to a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taint {
    /// Taint key.
    pub key: String,
    /// Taint value.
    pub value: String,
    /// Taint effect such as `NoSchedule`.
    pub effect: String,
}

/// Slim node status.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct K8sNodeStatus {
    /// Reachable node addresses.
    pub addresses: Vec<NodeAddress>,
}

/// A node address entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeAddress {
    /// Address type such as `InternalIP`, `ExternalIP`, or `Hostname`.
    pub type_: String,
    /// Address value.
    pub address: String,
}

/// A typed Kubernetes watch event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent<T> {
    /// A resource was added.
    Added(T),
    /// A resource was modified.
    Modified(T),
    /// A resource was deleted.
    Deleted(T),
}

impl<T> WatchEvent<T> {
    /// Returns the object carried by the event.
    #[must_use]
    pub fn object(&self) -> &T {
        match self {
            Self::Added(object) | Self::Modified(object) | Self::Deleted(object) => object,
        }
    }

    /// Returns whether this event represents a deletion.
    #[must_use]
    pub fn is_deleted(&self) -> bool {
        matches!(self, Self::Deleted(_))
    }
}

/// A simplified Kubernetes label selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelSelector {
    /// Always matches.
    Everything,
    /// Never matches.
    Nothing,
    /// Matches only when all listed `key=value` pairs are present.
    MatchLabels(HashMap<String, String>),
}

impl LabelSelector {
    /// Returns whether the selector matches the provided label set.
    #[must_use]
    pub fn matches(&self, labels: &HashMap<String, String>) -> bool {
        match self {
            Self::Everything => true,
            Self::Nothing => false,
            Self::MatchLabels(required) => required
                .iter()
                .all(|(key, value)| labels.get(key) == Some(value)),
        }
    }
}

/// In-memory store of Kubernetes resources for unit tests and offline mode.
pub struct ResourceStore<T: Clone> {
    items: Arc<RwLock<HashMap<String, T>>>,
    tx: broadcast::Sender<WatchEvent<T>>,
}

impl<T: Clone + Send + Sync + 'static> ResourceStore<T> {
    /// Creates an empty resource store.
    #[must_use]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            items: Arc::default(),
            tx,
        }
    }

    /// Subscribes to future watch events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<WatchEvent<T>> {
        self.tx.subscribe()
    }

    /// Adds a resource and emits an `Added` watch event.
    pub async fn add(&self, key: String, object: T) {
        self.items.write().await.insert(key.clone(), object.clone());
        debug!(%key, "added resource to store");
        let _ = self.tx.send(WatchEvent::Added(object));
    }

    /// Updates a resource and emits a `Modified` watch event.
    pub async fn update(&self, key: String, object: T) {
        self.items.write().await.insert(key.clone(), object.clone());
        debug!(%key, "updated resource in store");
        let _ = self.tx.send(WatchEvent::Modified(object));
    }

    /// Deletes a resource and emits a `Deleted` watch event if present.
    pub async fn delete(&self, key: &str) -> Option<T> {
        let removed = self.items.write().await.remove(key);
        if let Some(object) = removed.as_ref() {
            debug!(%key, "deleted resource from store");
            let _ = self.tx.send(WatchEvent::Deleted(object.clone()));
        }
        removed
    }

    /// Returns a cloned resource by key.
    pub async fn get(&self, key: &str) -> Option<T> {
        self.items.read().await.get(key).cloned()
    }

    /// Returns all stored resources.
    pub async fn list(&self) -> Vec<T> {
        self.items.read().await.values().cloned().collect()
    }

    /// Returns the number of stored resources.
    pub async fn len(&self) -> usize {
        self.items.read().await.len()
    }

    /// Returns whether the store contains no resources.
    pub async fn is_empty(&self) -> bool {
        self.items.read().await.is_empty()
    }
}

impl<T: Clone + Send + Sync + 'static> Default for ResourceStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors returned by pure Kubernetes resource helpers.
#[derive(Debug, thiserror::Error)]
pub enum K8sError {
    /// The requested resource was not found.
    #[error("resource not found: {0}")]
    NotFound(String),
    /// A Kubernetes connection failed.
    #[error("connection error: {0}")]
    Connection(String),
    /// A Kubernetes payload could not be deserialized.
    #[error("deserialization error: {0}")]
    Deserialization(String),
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::watcher::{K8sEvent, WatcherError};
    use super::{
        K8sNode, K8sNodeSpec, K8sNodeStatus, LabelSelector, NodeAddress, ObjectMeta, Pod, PodPhase,
        PodSpec, PodStatus, ResourceStore, Taint, WatchEvent,
    };

    #[test]
    fn test_object_meta() {
        let mut meta = ObjectMeta::new("cilium-abc", "kube-system");
        meta.labels
            .insert(String::from("app"), String::from("cilium"));

        assert!(meta.has_label("app", "cilium"));
        assert!(!meta.has_label("app", "other"));
        assert_eq!(meta.namespaced_name(), "kube-system/cilium-abc");
    }

    #[test]
    fn test_label_selector_match() {
        let mut labels = HashMap::new();
        labels.insert(String::from("app"), String::from("nginx"));
        labels.insert(String::from("env"), String::from("prod"));

        assert!(LabelSelector::Everything.matches(&labels));
        assert!(!LabelSelector::Nothing.matches(&labels));

        let mut required = HashMap::new();
        required.insert(String::from("app"), String::from("nginx"));
        assert!(LabelSelector::MatchLabels(required.clone()).matches(&labels));

        required.insert(String::from("env"), String::from("staging"));
        assert!(!LabelSelector::MatchLabels(required).matches(&labels));
    }

    #[test]
    fn test_watch_event_helpers() {
        let pod = Pod {
            meta: ObjectMeta::new("p", "default"),
            spec: PodSpec::default(),
            status: PodStatus::default(),
        };

        let added = WatchEvent::Added(pod.clone());
        assert!(!added.is_deleted());
        assert_eq!(added.object().meta.name, "p");

        let deleted = WatchEvent::Deleted(pod);
        assert!(deleted.is_deleted());
    }

    #[tokio::test]
    async fn test_resource_store_add_get_delete() {
        let store: ResourceStore<Pod> = ResourceStore::new();
        let pod = Pod {
            meta: ObjectMeta::new("p1", "default"),
            spec: PodSpec::default(),
            status: PodStatus::default(),
        };

        store.add(String::from("default/p1"), pod).await;
        assert_eq!(store.len().await, 1);
        assert!(store.get("default/p1").await.is_some());

        let removed = store.delete("default/p1").await;
        assert!(removed.is_some());
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn test_resource_store_watch_events() {
        let store: ResourceStore<Pod> = ResourceStore::new();
        let mut receiver = store.subscribe();
        let pod = Pod {
            meta: ObjectMeta::new("p1", "ns"),
            spec: PodSpec::default(),
            status: PodStatus::default(),
        };

        store.add(String::from("ns/p1"), pod).await;

        let event = receiver.try_recv();
        assert!(matches!(event, Ok(WatchEvent::Added(_))));
    }

    #[test]
    fn test_slim_resources_round_trip_core_fields() {
        let pod = Pod {
            meta: ObjectMeta::new("agent", "kube-system"),
            spec: PodSpec {
                node_name: String::from("node-a"),
                host_network: true,
                service_account_name: String::from("cilium"),
            },
            status: PodStatus {
                pod_ip: Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 10))),
                host_ip: Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                    192, 168, 0, 10,
                ))),
                phase: PodPhase::Running,
            },
        };
        let node = K8sNode {
            meta: ObjectMeta::new("node-a", ""),
            spec: K8sNodeSpec {
                pod_cidr: Some(String::from("10.0.0.0/24")),
                pod_cidrs: vec![String::from("10.0.0.0/24")],
                provider_id: String::from("kind://node-a"),
                taints: vec![Taint {
                    key: String::from("node.cilium.io/agent-not-ready"),
                    value: String::from("true"),
                    effect: String::from("NoSchedule"),
                }],
            },
            status: K8sNodeStatus {
                addresses: vec![NodeAddress {
                    type_: String::from("InternalIP"),
                    address: String::from("192.168.0.10"),
                }],
            },
        };

        assert_eq!(pod.meta.namespaced_name(), "kube-system/agent");
        assert_eq!(node.status.addresses[0].type_, "InternalIP");
    }

    #[test]
    fn test_k8s_event_variants() {
        let events: Vec<K8sEvent> = vec![];
        assert!(events.is_empty());
    }

    #[test]
    fn test_watcher_error_display() {
        let error = WatcherError::Init(String::from("test"));
        assert!(error.to_string().contains("test"));
    }
}
