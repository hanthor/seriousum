use std::sync::Arc;
use std::net::IpAddr;

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Node, Pod, Service};
use kube::{
    Api, Client,
    api::{Patch, PatchParams},
    runtime::{
        WatchStreamExt,
        watcher::{self, watcher},
    },
};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Error type for watcher operations.
#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("kubernetes client error: {0}")]
    Client(#[from] kube::Error),
    #[error("watcher initialisation failed: {0}")]
    Init(String),
}

/// Result type returned by Kubernetes watcher operations.
pub type Result<T> = std::result::Result<T, WatcherError>;

/// Events emitted by the Kubernetes watcher to other subsystems.
#[derive(Debug, Clone)]
pub enum K8sEvent {
    /// A node was observed for the first time or reapplied.
    NodeAdded(Node),
    /// A node was updated.
    NodeUpdated(Node),
    /// A node was deleted.
    NodeDeleted(String),
    /// A pod was observed for the first time or reapplied.
    PodAdded(Pod),
    /// A pod was updated.
    PodUpdated(Pod),
    /// A pod was deleted.
    PodDeleted(String),
    /// A service was observed for the first time or reapplied.
    ServiceAdded(Service),
    /// A service was updated.
    ServiceUpdated(Service),
    /// A service was deleted.
    ServiceDeleted(String),
}

/// Kubernetes watcher that streams resource changes to subscribers.
pub struct K8sWatcher {
    client: Client,
    tx: tokio::sync::broadcast::Sender<K8sEvent>,
}

impl K8sWatcher {
    /// Create a new watcher with an in-cluster or kubeconfig client.
    pub async fn new() -> Result<(Self, tokio::sync::broadcast::Receiver<K8sEvent>)> {
        let client = Client::try_default().await?;
        Ok(Self::from_client(client))
    }

    /// Create from an existing client.
    #[must_use]
    pub fn from_client(client: Client) -> (Self, tokio::sync::broadcast::Receiver<K8sEvent>) {
        let (tx, rx) = tokio::sync::broadcast::channel(1024);
        (Self { client, tx }, rx)
    }

    /// Resolve the node name for a given pod.
    pub async fn resolve_node_name_from_pod(
        &self,
        namespace: &str,
        pod_name: &str,
    ) -> Result<Option<String>> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
        let pod = pods.get(pod_name).await?;
        Ok(pod.spec.and_then(|spec| spec.node_name))
    }

    /// Remove bootstrap-blocking taints from the given node.
    ///
    /// Returns `true` when at least one taint was present and removed.
    pub async fn remove_agent_not_ready_taint(&self, node_name: &str) -> Result<bool> {
        let api: Api<Node> = Api::all(self.client.clone());
        let mut node = api.get(node_name).await?;
        let Some(spec) = node.spec.as_mut() else {
            return Ok(false);
        };
        let Some(taints) = spec.taints.as_mut() else {
            return Ok(false);
        };

        if !remove_bootstrap_blocking_taints(taints) {
            return Ok(false);
        }

        let params = PatchParams::default();
        let patch = Patch::Merge(serde_json::json!({
            "spec": {
                "taints": spec.taints,
            }
        }));
        let _updated = api.patch(node_name, &params, &patch).await?;
        Ok(true)
    }

    /// Resolve the preferred internal IP for the given node.
    pub async fn resolve_node_internal_ip(&self, node_name: &str) -> Result<Option<IpAddr>> {
        let api: Api<Node> = Api::all(self.client.clone());
        let node = api.get(node_name).await?;
        let Some(status) = node.status else {
            return Ok(None);
        };

        for address in status.addresses.unwrap_or_default() {
            if address.type_ == "InternalIP"
                && let Ok(ip) = address.address.parse::<IpAddr>()
            {
                return Ok(Some(ip));
            }
        }

        Ok(None)
    }

    /// Start watching nodes in the background.
    #[must_use]
    pub fn watch_nodes(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let api: Api<Node> = Api::all(client);
            let mut stream = watcher(api, watcher::Config::default())
                .applied_objects()
                .boxed();
            loop {
                match stream.try_next().await {
                    Ok(Some(node)) => {
                        let name = node.metadata.name.clone().unwrap_or_default();
                        debug!(node = %name, "node event");
                        let _ = tx.send(K8sEvent::NodeAdded(node));
                    }
                    Ok(None) => {
                        info!("node watch stream ended, reconnecting");
                        break;
                    }
                    Err(error) => {
                        warn!(error = %error, "node watch error");
                        break;
                    }
                }
            }
        })
    }

    /// Start watching pods in the background.
    #[must_use]
    pub fn watch_pods(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let api: Api<Pod> = Api::all(client);
            let mut stream = watcher(api, watcher::Config::default())
                .applied_objects()
                .boxed();
            loop {
                match stream.try_next().await {
                    Ok(Some(pod)) => {
                        let name = pod.metadata.name.clone().unwrap_or_default();
                        debug!(pod = %name, "pod event");
                        let _ = tx.send(K8sEvent::PodAdded(pod));
                    }
                    Ok(None) => {
                        info!("pod watch stream ended, reconnecting");
                        break;
                    }
                    Err(error) => {
                        warn!(error = %error, "pod watch error");
                        break;
                    }
                }
            }
        })
    }

    /// Start watching services in the background.
    #[must_use]
    pub fn watch_services(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let api: Api<Service> = Api::all(client);
            let mut stream = watcher(api, watcher::Config::default())
                .applied_objects()
                .boxed();
            loop {
                match stream.try_next().await {
                    Ok(Some(service)) => {
                        let name = service.metadata.name.clone().unwrap_or_default();
                        debug!(service = %name, "service event");
                        let _ = tx.send(K8sEvent::ServiceAdded(service));
                    }
                    Ok(None) => {
                        info!("service watch stream ended, reconnecting");
                        break;
                    }
                    Err(error) => {
                        warn!(error = %error, "service watch error");
                        break;
                    }
                }
            }
        })
    }
}

fn remove_bootstrap_blocking_taints(taints: &mut Vec<k8s_openapi::api::core::v1::Taint>) -> bool {
    let before = taints.len();
    taints.retain(|taint| {
        !matches!(
            taint.key.as_str(),
            "node.cilium.io/agent-not-ready" | "node.kubernetes.io/not-ready"
        )
    });
    taints.len() != before
}

#[cfg(test)]
mod tests {
    use super::remove_bootstrap_blocking_taints;

    #[test]
    fn test_remove_bootstrap_blocking_taints() {
        let mut taints = vec![
            k8s_openapi::api::core::v1::Taint {
                key: String::from("node.cilium.io/agent-not-ready"),
                value: Some(String::from("true")),
                effect: String::from("NoSchedule"),
                time_added: None,
            },
            k8s_openapi::api::core::v1::Taint {
                key: String::from("node.kubernetes.io/not-ready"),
                value: None,
                effect: String::from("NoSchedule"),
                time_added: None,
            },
            k8s_openapi::api::core::v1::Taint {
                key: String::from("dedicated"),
                value: Some(String::from("system")),
                effect: String::from("NoSchedule"),
                time_added: None,
            },
        ];

        assert!(remove_bootstrap_blocking_taints(&mut taints));
        assert_eq!(taints.len(), 1);
        assert_eq!(taints[0].key, "dedicated");
    }

    #[test]
    fn test_remove_bootstrap_blocking_taints_noop() {
        let mut taints = vec![k8s_openapi::api::core::v1::Taint {
            key: String::from("dedicated"),
            value: Some(String::from("system")),
            effect: String::from("NoSchedule"),
            time_added: None,
        }];

        assert!(!remove_bootstrap_blocking_taints(&mut taints));
        assert_eq!(taints.len(), 1);
    }
}
