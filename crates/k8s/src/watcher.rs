use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Node, Pod, Service};
use kube::{
    Api, Client,
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
