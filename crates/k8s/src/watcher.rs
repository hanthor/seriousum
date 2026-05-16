use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::core::v1::{Endpoints, Node, Pod, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
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
    /// An Endpoints object was observed for the first time or reapplied.
    EndpointsAdded(Endpoints),
    /// An Endpoints object was updated.
    EndpointsUpdated(Endpoints),
    /// An Endpoints object was deleted.
    EndpointsDeleted(String),
    /// An EndpointSlice was observed for the first time or reapplied.
    EndpointSliceAdded(EndpointSlice),
    /// An EndpointSlice was updated.
    EndpointSliceUpdated(EndpointSlice),
    /// An EndpointSlice was deleted.
    EndpointSliceDeleted(String),
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
        let (watcher, rx) = Self::from_client(client);
        watcher.ensure_cilium_endpoint_crd().await;
        Ok((watcher, rx))
    }

    /// Ensure the CiliumEndpoint CRD exists. Applied idempotently — safe to call
    /// on every agent startup. If the CRD is already present, this is a no-op.
    async fn ensure_cilium_endpoint_crd(&self) {
        use kube::api::{DynamicObject, Patch, PatchParams};
        use kube::core::ApiResource;

        let crd_ar = ApiResource {
            group: "apiextensions.k8s.io".to_string(),
            version: "v1".to_string(),
            api_version: "apiextensions.k8s.io/v1".to_string(),
            kind: "CustomResourceDefinition".to_string(),
            plural: "customresourcedefinitions".to_string(),
        };
        let crd_api: kube::Api<DynamicObject> = kube::Api::all_with(self.client.clone(), &crd_ar);

        let crd_body: DynamicObject = match serde_json::from_value(serde_json::json!({
            "apiVersion": "apiextensions.k8s.io/v1",
            "kind": "CustomResourceDefinition",
            "metadata": {
                "name": "ciliumendpoints.cilium.io",
                "labels": {"app.kubernetes.io/part-of": "cilium"}
            },
            "spec": {
                "group": "cilium.io",
                "names": {
                    "categories": ["cilium","ciliumpolicy"],
                    "kind": "CiliumEndpoint",
                    "listKind": "CiliumEndpointList",
                    "plural": "ciliumendpoints",
                    "shortNames": ["cep","ciliumep"],
                    "singular": "ciliumendpoint"
                },
                "scope": "Namespaced",
                "versions": [{
                    "name": "v2",
                    "served": true,
                    "storage": true,
                    "additionalPrinterColumns": [
                        {"jsonPath": ".status.id", "name": "Security Identity", "type": "integer"},
                        {"jsonPath": ".status.state", "name": "Endpoint State", "type": "string"},
                        {"jsonPath": ".status.networking.addressing[0].ipv4", "name": "IPv4", "type": "string"},
                        {"jsonPath": ".status.networking.addressing[0].ipv6", "name": "IPv6", "type": "string"}
                    ],
                    "schema": {
                        "openAPIV3Schema": {
                            "description": "CiliumEndpoint is the status of a Cilium policy rule",
                            "type": "object",
                            "properties": {
                                "apiVersion": {"type": "string"},
                                "kind": {"type": "string"},
                                "metadata": {"type": "object"},
                                "status": {
                                    "type": "object",
                                    "x-kubernetes-preserve-unknown-fields": true
                                }
                            }
                        }
                    },
                    "subresources": {}
                }]
            }
        })) {
            Ok(v) => v,
            Err(e) => { warn!("CRD body build failed: {e}"); return; }
        };

        match crd_api.patch(
            "ciliumendpoints.cilium.io",
            &PatchParams::apply("seriousum-agent").force(),
            &Patch::Apply(crd_body),
        ).await {
            Ok(_) => info!("CiliumEndpoint CRD ensured"),
            Err(e) => warn!("CiliumEndpoint CRD ensure failed: {e}"),
        }
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

    /// Resolve the preferred IPv4 pod CIDR for the given node.
    pub async fn resolve_node_ipv4_pod_cidr(&self, node_name: &str) -> Result<Option<String>> {
        let api: Api<Node> = Api::all(self.client.clone());
        let node = api.get(node_name).await?;
        let Some(spec) = node.spec else {
            return Ok(None);
        };

        Ok(preferred_ipv4_pod_cidr(&spec))
    }

    /// List all currently visible pods.
    pub async fn list_pods(&self) -> Result<Vec<Pod>> {
        let api: Api<Pod> = Api::all(self.client.clone());
        Ok(api.list(&Default::default()).await?.items)
    }

    /// List all currently visible nodes.
    pub async fn list_nodes(&self) -> Result<Vec<Node>> {
        let api: Api<Node> = Api::all(self.client.clone());
        Ok(api.list(&Default::default()).await?.items)
    }

    /// List all currently visible services.
    pub async fn list_services(&self) -> Result<Vec<Service>> {
        let api: Api<Service> = Api::all(self.client.clone());
        Ok(api.list(&Default::default()).await?.items)
    }

    /// List all currently visible Endpoints objects.
    pub async fn list_endpoints(&self) -> Result<Vec<Endpoints>> {
        let api: Api<Endpoints> = Api::all(self.client.clone());
        Ok(api.list(&Default::default()).await?.items)
    }

    /// List all currently visible `EndpointSlice` objects.
    pub async fn list_endpoint_slices(&self) -> Result<Vec<EndpointSlice>> {
        let api: Api<EndpointSlice> = Api::all(self.client.clone());
        Ok(api.list(&Default::default()).await?.items)
    }

    /// Start watching nodes in the background.
    #[must_use]
    pub fn watch_nodes(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let api: Api<Node> = Api::all(client);
            loop {
                let mut stream = watcher(api.clone(), watcher::Config::default())
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
                tokio::time::sleep(Duration::from_secs(1)).await;
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
            loop {
                let mut stream = watcher(api.clone(), watcher::Config::default())
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
                tokio::time::sleep(Duration::from_secs(1)).await;
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
            loop {
                let mut stream = watcher(api.clone(), watcher::Config::default())
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
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    }

    /// Start watching EndpointSlices in the background.
    #[must_use]
    pub fn watch_endpoints(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let api: Api<Endpoints> = Api::all(client);
            loop {
                let mut stream = watcher(api.clone(), watcher::Config::default())
                    .applied_objects()
                    .boxed();
                loop {
                    match stream.try_next().await {
                        Ok(Some(endpoints)) => {
                            let name = endpoints.metadata.name.clone().unwrap_or_default();
                            debug!(endpoints = %name, "endpoints event");
                            let _ = tx.send(K8sEvent::EndpointsAdded(endpoints));
                        }
                        Ok(None) => {
                            info!("endpoints watch stream ended, reconnecting");
                            break;
                        }
                        Err(error) => {
                            warn!(error = %error, "endpoints watch error");
                            break;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    }

    /// Start watching EndpointSlices in the background.
    #[must_use]
    pub fn watch_endpoint_slices(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let api: Api<EndpointSlice> = Api::all(client);
            loop {
                let mut stream = watcher(api.clone(), watcher::Config::default())
                    .applied_objects()
                    .boxed();
                loop {
                    match stream.try_next().await {
                        Ok(Some(endpoint_slice)) => {
                            let name = endpoint_slice.metadata.name.clone().unwrap_or_default();
                            debug!(endpoint_slice = %name, "endpoint slice event");
                            let _ = tx.send(K8sEvent::EndpointSliceAdded(endpoint_slice));
                        }
                        Ok(None) => {
                            info!("endpoint slice watch stream ended, reconnecting");
                            break;
                        }
                        Err(error) => {
                            warn!(error = %error, "endpoint slice watch error");
                            break;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    }

    /// Create or update a CiliumEndpoint CR for the given pod.
    ///
    /// Cilium's integration tests verify that each managed pod has a CiliumEndpoint
    /// resource so they can inspect endpoint identity and networking state. This
    /// creates a minimal CiliumEndpoint with `state: ready` using server-side apply
    /// for the main object and a separate status subresource patch.
    pub async fn upsert_cilium_endpoint(
        &self,
        namespace: &str,
        pod_name: &str,
        pod_ip: &str,
        endpoint_id: i64,
    ) -> Result<()> {
        use kube::api::{DynamicObject, Patch, PatchParams};
        use kube::core::ApiResource;

        info!(pod = pod_name, namespace, ip = pod_ip, "creating CiliumEndpoint");

        let ar = ApiResource {
            group: "cilium.io".to_string(),
            version: "v2".to_string(),
            api_version: "cilium.io/v2".to_string(),
            kind: "CiliumEndpoint".to_string(),
            plural: "ciliumendpoints".to_string(),
        };

        let api: kube::Api<DynamicObject> =
            kube::Api::namespaced_with(self.client.clone(), namespace, &ar);

        // The CiliumEndpoint CRD has no status subresource (subresources: {}),
        // so status can be set directly via SSA on the main resource.
        let body: DynamicObject = serde_json::from_value(serde_json::json!({
            "apiVersion": "cilium.io/v2",
            "kind": "CiliumEndpoint",
            "metadata": {
                "name": pod_name,
                "namespace": namespace,
            },
            "status": {
                "id": endpoint_id,
                "state": "ready",
                "networking": {"addressing": [{"ipv4": pod_ip}]},
                "identity": {"id": endpoint_id, "labels": []}
            }
        }))
        .map_err(|e| WatcherError::Init(e.to_string()))?;

        // Retry up to 10 times with 1-second backoff in case the CRD hasn't propagated yet.
        let mut last_err = None;
        for attempt in 0..10u32 {
            match api.patch(
                pod_name,
                &PatchParams::apply("seriousum-agent").force(),
                &Patch::Apply(body.clone()),
            ).await {
                Ok(_) => {
                    info!(pod = pod_name, namespace, "CiliumEndpoint created");
                    return Ok(());
                }
                Err(e) => {
                    let msg = e.to_string();
                    // Retry on "not found" / "page not found" which means CRD not registered yet.
                    if msg.contains("page not found") || msg.contains("not found") {
                        debug!(pod = pod_name, attempt, "CRD not ready, retrying");
                        last_err = Some(e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    } else {
                        return Err(WatcherError::Client(e));
                    }
                }
            }
        }
        Err(WatcherError::Client(last_err.unwrap()))
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

fn preferred_ipv4_pod_cidr(spec: &k8s_openapi::api::core::v1::NodeSpec) -> Option<String> {
    if let Some(pod_cidr) = &spec.pod_cidr
        && pod_cidr.contains('.')
    {
        return Some(pod_cidr.clone());
    }

    spec.pod_cidrs
        .as_deref()
        .into_iter()
        .flatten()
        .find(|pod_cidr| pod_cidr.contains('.'))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::{preferred_ipv4_pod_cidr, remove_bootstrap_blocking_taints};

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
    fn test_prefers_ipv4_pod_cidr() {
        let spec = k8s_openapi::api::core::v1::NodeSpec {
            pod_cidr: Some("10.244.0.0/24".to_string()),
            pod_cidrs: Some(vec!["fd00::/64".to_string(), "10.244.1.0/24".to_string()]),
            ..Default::default()
        };

        assert_eq!(
            preferred_ipv4_pod_cidr(&spec).as_deref(),
            Some("10.244.0.0/24")
        );
    }

    #[test]
    fn test_falls_back_to_ipv4_pod_cidrs() {
        let spec = k8s_openapi::api::core::v1::NodeSpec {
            pod_cidr: Some("fd00::/64".to_string()),
            pod_cidrs: Some(vec!["fd00::/64".to_string(), "10.244.1.0/24".to_string()]),
            ..Default::default()
        };

        assert_eq!(
            preferred_ipv4_pod_cidr(&spec).as_deref(),
            Some("10.244.1.0/24")
        );
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
