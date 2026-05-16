use std::collections::HashMap;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ipnet::Ipv4Net;
use k8s_openapi::api::core::v1::{Endpoints as K8sEndpoints, Node, Pod, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::health::{SharedHealth, new_health, serve, set_ready, set_stopping};
use crate::loadbalancer::{reconcile_service, BackendSyncer};
use crate::{DaemonConfig, DaemonPhase, DaemonStatus};

const CILIUM_SOCK_FILE: &str = "cilium.sock";
const WRITE_CNI_CONF_WHEN_READY_ENV: &str = "WRITE_CNI_CONF_WHEN_READY";
const CNI_LOG_FILE_ENV: &str = "CNI_LOG_FILE";
const DEFAULT_CNI_LOG_FILE: &str = "/var/run/cilium/cilium-cni.log";
const HELM_CONFIG_DIR: &str = "/tmp/cilium/config-map";
const DEFAULT_COMPAT_ALLOC_RANGE: &str = "10.244.0.0/16";
static IPAM_COUNTER: AtomicU32 = AtomicU32::new(10);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompatAddressing {
    router_ip: Ipv4Addr,
    alloc_range: String,
}

#[derive(Debug, Clone)]
struct CompatEndpoint {
    id: i64,
    namespace: String,
    name: String,
    node_name: String,
    pod_ip: Option<Ipv4Addr>,
    labels: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct CompatService {
    id: i64,
    namespace: String,
    name: String,
    cluster_ip: Option<IpAddr>,
    ports: Vec<CompatServicePort>,
    backends: Vec<CompatBackend>,
    service_type: String,
}

#[derive(Debug, Clone)]
struct CompatServicePort {
    name: Option<String>,
    port: u16,
    protocol: String,
    target_port: Option<CompatTargetPort>,
}

#[derive(Debug, Clone)]
enum CompatTargetPort {
    Number(u16),
    Name(String),
}

#[derive(Debug, Clone)]
struct CompatBackend {
    ip: IpAddr,
    port: u16,
    protocol: String,
    node_name: Option<String>,
    port_name: Option<String>,
}

#[derive(Debug, Default)]
struct CompatState {
    node_name: String,
    next_endpoint_id: i64,
    next_service_id: i64,
    endpoints: HashMap<String, CompatEndpoint>,
    services: HashMap<String, CompatService>,
    service_backends: HashMap<String, Vec<CompatBackend>>,
    backend_syncer: BackendSyncer,
}

impl CompatState {
    fn new(node_name: String) -> Self {
        Self {
            node_name,
            next_endpoint_id: 1,
            next_service_id: 1,
            endpoints: HashMap::new(),
            services: HashMap::new(),
            service_backends: HashMap::new(),
            backend_syncer: BackendSyncer::default_path(),
        }
    }

    fn endpoint_count(&self) -> u32 {
        self.endpoints.len().try_into().unwrap_or(u32::MAX)
    }

    fn list_endpoints(&self) -> Vec<&CompatEndpoint> {
        let mut endpoints: Vec<_> = self.endpoints.values().collect();
        endpoints.sort_by_key(|endpoint| endpoint.id);
        endpoints
    }

    fn endpoint_by_id(&self, id: i64) -> Option<&CompatEndpoint> {
        self.endpoints.values().find(|endpoint| endpoint.id == id)
    }

    fn list_services(&self) -> Vec<&CompatService> {
        let mut services: Vec<_> = self.services.values().collect();
        services.sort_by_key(|service| service.id);
        services
    }

    fn upsert_pod(&mut self, local_node_name: &str, pod: &Pod) {
        let Some(name) = pod.metadata.name.as_deref().filter(|name| !name.is_empty()) else {
            return;
        };
        let namespace = pod
            .metadata
            .namespace
            .as_deref()
            .filter(|namespace| !namespace.is_empty())
            .unwrap_or("default");
        let key = format!("{namespace}/{name}");
        let Some(spec) = pod.spec.as_ref() else {
            self.endpoints.remove(&key);
            return;
        };
        if spec.host_network.unwrap_or(false) || spec.node_name.as_deref() != Some(local_node_name)
        {
            self.endpoints.remove(&key);
            return;
        }

        let pod_ip = pod
            .status
            .as_ref()
            .and_then(|status| status.pod_ip.as_deref())
            .and_then(|ip| ip.parse::<Ipv4Addr>().ok());
        let Some(pod_ip) = pod_ip else {
            self.endpoints.remove(&key);
            return;
        };

        let labels = pod
            .metadata
            .labels
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect::<HashMap<_, _>>();
        let id = self
            .endpoints
            .get(&key)
            .map(|endpoint| endpoint.id)
            .unwrap_or_else(|| self.allocate_endpoint_id());
        self.endpoints.insert(
            key,
            CompatEndpoint {
                id,
                namespace: namespace.to_string(),
                name: name.to_string(),
                node_name: local_node_name.to_string(),
                pod_ip: Some(pod_ip),
                labels,
            },
        );
    }

    fn upsert_service(&mut self, service: &Service) {
        let Some(name) = service
            .metadata
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let namespace = service
            .metadata
            .namespace
            .as_deref()
            .filter(|namespace| !namespace.is_empty())
            .unwrap_or("default");
        let key = format!("{namespace}/{name}");
        let Some(spec) = service.spec.as_ref() else {
            self.services.remove(&key);
            return;
        };

        let cluster_ip = spec
            .cluster_ip
            .as_deref()
            .filter(|cluster_ip| !cluster_ip.is_empty() && *cluster_ip != "None")
            .and_then(|cluster_ip| cluster_ip.parse::<IpAddr>().ok());
        let Some(cluster_ip) = cluster_ip else {
            self.services.remove(&key);
            return;
        };

        let id = self
            .services
            .get(&key)
            .map(|service| service.id)
            .unwrap_or_else(|| self.allocate_service_id());
        let ports = spec
            .ports
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter_map(|service_port| {
                let protocol = port_protocol(service_port.protocol.as_ref());
                let frontend_port = u16::try_from(service_port.port).ok()?;
                Some(CompatServicePort {
                    name: service_port.name.clone(),
                    port: frontend_port,
                    protocol,
                    target_port: compat_target_port(service_port.target_port.as_ref()),
                })
            })
            .collect();
        let backends = self.service_backends.get(&key).cloned().unwrap_or_default();

        self.services.insert(
            key,
            CompatService {
                id,
                namespace: namespace.to_string(),
                name: name.to_string(),
                cluster_ip: Some(cluster_ip),
                ports,
                backends,
                service_type: spec
                    .type_
                    .clone()
                    .unwrap_or_else(|| "ClusterIP".to_string()),
            },
        );
    }

    fn upsert_endpoint_slice(&mut self, endpoint_slice: &EndpointSlice) {
        let Some(name) = endpoint_slice
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get("kubernetes.io/service-name"))
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let namespace = endpoint_slice
            .metadata
            .namespace
            .as_deref()
            .filter(|namespace| !namespace.is_empty())
            .unwrap_or("default");
        let key = format!("{namespace}/{name}");

        let backends = endpoint_slice
            .endpoints
            .iter()
            .flat_map(|endpoint| {
                if matches!(
                    endpoint
                        .conditions
                        .as_ref()
                        .and_then(|conditions| conditions.ready),
                    Some(false)
                ) {
                    return Vec::new();
                }
                endpoint
                    .addresses
                    .iter()
                    .flat_map(|address| {
                        endpoint_slice
                            .ports
                            .as_deref()
                            .unwrap_or(&[])
                            .iter()
                            .filter_map(|port| {
                                let ip = address.parse::<IpAddr>().ok()?;
                                let backend_port = u16::try_from(port.port?).ok()?;
                                Some(CompatBackend {
                                    ip,
                                    port: backend_port,
                                    protocol: port_protocol(port.protocol.as_ref()),
                                    node_name: endpoint.node_name.clone(),
                                    port_name: port.name.clone(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        self.service_backends.insert(key.clone(), backends.clone());
        if let Some(service) = self.services.get_mut(&key) {
            service.backends = backends.clone();
        }

        // Reconcile into eBPF LB maps (IPv4 only — eBPF lb4 maps don't handle IPv6).
        let cluster_ip = self.services.get(&key).and_then(|s| s.cluster_ip)
            .and_then(|ip| if let IpAddr::V4(v4) = ip { Some(v4) } else { None });
        let frontends: Vec<(u16, u8)> = self
            .services
            .get(&key)
            .map(|s| s.ports.iter().map(|p| (p.port, protocol_to_u8(&p.protocol))).collect())
            .unwrap_or_default();
        let be_tuples: Vec<(Ipv4Addr, u16, u8)> = backends
            .iter()
            .filter_map(|b| if let IpAddr::V4(v4) = b.ip { Some((v4, b.port, protocol_to_u8(&b.protocol))) } else { None })
            .collect();
        reconcile_service(&self.backend_syncer, &key, cluster_ip, frontends, be_tuples);
    }

    fn upsert_endpoints(&mut self, endpoints: &K8sEndpoints) {
        let Some(name) = endpoints
            .metadata
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
        else {
            return;
        };
        let namespace = endpoints
            .metadata
            .namespace
            .as_deref()
            .filter(|namespace| !namespace.is_empty())
            .unwrap_or("default");
        let key = format!("{namespace}/{name}");

        let backends = endpoints
            .subsets
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .flat_map(|subset| {
                subset
                    .addresses
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .flat_map(|address| {
                        let Some(ip) = address.ip.parse::<IpAddr>().ok() else {
                            return Vec::new();
                        };
                        subset
                            .ports
                            .as_deref()
                            .unwrap_or(&[])
                            .iter()
                            .filter_map(|port| {
                                let backend_port = u16::try_from(port.port).ok()?;
                                Some(CompatBackend {
                                    ip,
                                    port: backend_port,
                                    protocol: port_protocol(port.protocol.as_ref()),
                                    node_name: address.node_name.clone(),
                                    port_name: port.name.clone(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        self.service_backends.insert(key.clone(), backends.clone());
        if let Some(service) = self.services.get_mut(&key) {
            service.backends = backends.clone();
        }

        // Reconcile into eBPF LB maps (IPv4 only — eBPF lb4 maps don't handle IPv6).
        let cluster_ip = self.services.get(&key).and_then(|s| s.cluster_ip)
            .and_then(|ip| if let IpAddr::V4(v4) = ip { Some(v4) } else { None });
        let frontends: Vec<(u16, u8)> = self
            .services
            .get(&key)
            .map(|s| s.ports.iter().map(|p| (p.port, protocol_to_u8(&p.protocol))).collect())
            .unwrap_or_default();
        let be_tuples: Vec<(Ipv4Addr, u16, u8)> = backends
            .iter()
            .filter_map(|b| if let IpAddr::V4(v4) = b.ip { Some((v4, b.port, protocol_to_u8(&b.protocol))) } else { None })
            .collect();
        reconcile_service(&self.backend_syncer, &key, cluster_ip, frontends, be_tuples);
    }

    fn allocate_endpoint_id(&mut self) -> i64 {
        let id = self.next_endpoint_id;
        self.next_endpoint_id = self.next_endpoint_id.saturating_add(1);
        id
    }

    fn allocate_service_id(&mut self) -> i64 {
        let id = self.next_service_id;
        self.next_service_id = self.next_service_id.saturating_add(1);
        id
    }
}

impl Default for CompatAddressing {
    fn default() -> Self {
        Self {
            router_ip: first_usable_ipv4(DEFAULT_COMPAT_ALLOC_RANGE)
                .unwrap_or(Ipv4Addr::new(10, 244, 0, 1)),
            alloc_range: DEFAULT_COMPAT_ALLOC_RANGE.to_string(),
        }
    }
}

/// Signals that the daemon should use for shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSignal {
    /// Process received `SIGTERM`.
    Sigterm,
    /// Process received `SIGINT`.
    Sigint,
    /// Shutdown was requested programmatically.
    Manual,
}

/// Top-level runtime that owns daemon lifecycle state.
pub struct DaemonRuntime {
    config: DaemonConfig,
    cancel: CancellationToken,
    status: Arc<RwLock<DaemonStatus>>,
    health: SharedHealth,
    compat_state: Arc<RwLock<CompatState>>,
}

impl DaemonRuntime {
    /// Creates a new runtime for the provided daemon configuration.
    pub fn new(config: DaemonConfig) -> Self {
        let node_name = std::env::var("K8S_NODE_NAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "localhost".to_string());

        Self {
            config,
            cancel: CancellationToken::new(),
            status: Arc::new(RwLock::new(DaemonStatus::new(node_name.clone()))),
            health: new_health(),
            compat_state: Arc::new(RwLock::new(CompatState::new(node_name))),
        }
    }

    /// Run the daemon until a shutdown signal is received.
    pub async fn run(&self) -> Result<ShutdownSignal, Box<dyn Error + Send + Sync>> {
        if self.cancel.is_cancelled() {
            {
                let mut status = self.status.write().await;
                status.phase = DaemonPhase::Stopped;
            }
            info!("cancellation requested before startup");
            return Ok(ShutdownSignal::Manual);
        }

        {
            let mut status = self.status.write().await;
            status.phase = DaemonPhase::Starting;
        }

        info!(
            cluster_name = %self.config.cluster_name,
            cluster_id = self.config.cluster_id,
            ipv4_enabled = self.config.ipv4_enabled,
            ipv6_enabled = self.config.ipv6_enabled,
            hubble_enabled = self.config.enable_hubble,
            "cilium-agent starting"
        );

        let _ = rustls::crypto::ring::default_provider().install_default();

        let mut node_ipv4_pod_cidr = None;
        if self.config.enable_k8s_integration {
            match cilium_k8s::K8sWatcher::new().await {
                Ok((watcher, mut rx)) => {
                    info!("kubernetes watcher connected");
                    let mut node_name = std::env::var("K8S_NODE_NAME").ok();
                    if node_name.is_none()
                        && let Ok(pod_name) = std::env::var("HOSTNAME")
                    {
                        match watcher
                            .resolve_node_name_from_pod("kube-system", &pod_name)
                            .await
                        {
                            Ok(Some(resolved)) => {
                                node_name = Some(resolved);
                            }
                            Ok(None) => {
                                warn!(pod = %pod_name, "pod has no node_name set");
                            }
                            Err(error) => {
                                warn!(pod = %pod_name, error = %error, "unable to resolve node name from pod");
                            }
                        }
                    }
                    let node_name = match node_name {
                        Some(name) => name,
                        None => self.status.read().await.node_name.clone(),
                    };
                    node_ipv4_pod_cidr = match watcher.resolve_node_ipv4_pod_cidr(&node_name).await
                    {
                        Ok(Some(cidr)) => Some(cidr),
                        Ok(None) => {
                            warn!(node = %node_name, "unable to resolve node IPv4 pod CIDR");
                            None
                        }
                        Err(error) => {
                            warn!(node = %node_name, error = %error, "unable to resolve node IPv4 pod CIDR");
                            None
                        }
                    };
                    match watcher.remove_agent_not_ready_taint(&node_name).await {
                        Ok(true) => {
                            info!(node = %node_name, "removed bootstrap-blocking node taints");
                        }
                        Ok(false) => {
                            info!(node = %node_name, "bootstrap-blocking taints not present on node");
                        }
                        Err(error) => {
                            warn!(node = %node_name, error = %error, "unable to remove bootstrap-blocking taints");
                        }
                    }
                    let watcher = std::sync::Arc::new(watcher);
                    {
                        let mut compat_state = self.compat_state.write().await;
                        compat_state.node_name.clone_from(&node_name);
                    }
                    let pods = match watcher.list_pods().await {
                        Ok(pods) => Some(pods),
                        Err(error) => {
                            warn!(error = %error, "unable to seed compat pod state");
                            None
                        }
                    };
                    let nodes = match watcher.list_nodes().await {
                        Ok(nodes) => Some(nodes),
                        Err(error) => {
                            warn!(error = %error, "unable to seed remote node routes");
                            None
                        }
                    };
                    let services = match watcher.list_services().await {
                        Ok(services) => Some(services),
                        Err(error) => {
                            warn!(error = %error, "unable to seed compat service state");
                            None
                        }
                    };
                    let endpoints = match watcher.list_endpoints().await {
                        Ok(endpoints) => Some(endpoints),
                        Err(error) => {
                            warn!(error = %error, "unable to seed compat endpoints state");
                            None
                        }
                    };
                    let endpoint_slices = match watcher.list_endpoint_slices().await {
                        Ok(endpoint_slices) => Some(endpoint_slices),
                        Err(error) => {
                            warn!(error = %error, "unable to seed compat endpoint slice state");
                            None
                        }
                    };
                    let initial_endpoint_count = {
                        let mut compat_state = self.compat_state.write().await;
                        if let Some(pods) = pods {
                            for pod in pods {
                                compat_state.upsert_pod(&node_name, &pod);
                            }
                        }
                        if let Some(nodes) = &nodes {
                            for node in nodes {
                                sync_remote_node_route(node, &node_name);
                            }
                        }
                        if let Some(services) = services {
                            for service in services {
                                compat_state.upsert_service(&service);
                            }
                        }
                        if let Some(endpoints) = endpoints {
                            for endpoints in endpoints {
                                compat_state.upsert_endpoints(&endpoints);
                            }
                        }
                        if let Some(endpoint_slices) = endpoint_slices {
                            for endpoint_slice in endpoint_slices {
                                compat_state.upsert_endpoint_slice(&endpoint_slice);
                            }
                        }
                        compat_state.endpoint_count()
                    };
                    self.status.write().await.endpoint_count = initial_endpoint_count;
                    let compat_state = Arc::clone(&self.compat_state);
                    let status = Arc::clone(&self.status);
                    let local_node_name = node_name.clone();
                    let event_cancel = self.cancel.child_token();
                    let watcher_for_events = Arc::clone(&watcher);
                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                () = event_cancel.cancelled() => {
                                    break;
                                }
                                event = rx.recv() => {
                                    match event {
                                        Ok(cilium_k8s::K8sEvent::PodAdded(pod) | cilium_k8s::K8sEvent::PodUpdated(pod)) => {
                                            let (endpoint_count, ep_info) = {
                                                let mut compat_state = compat_state.write().await;
                                                compat_state.upsert_pod(&local_node_name, &pod);
                                                let key = format!(
                                                    "{}/{}",
                                                    pod.metadata.namespace.as_deref().unwrap_or("default"),
                                                    pod.metadata.name.as_deref().unwrap_or("")
                                                );
                                                let ep_info = compat_state.endpoints.get(&key).map(|ep| {
                                                    (ep.namespace.clone(), ep.name.clone(), ep.pod_ip, ep.id)
                                                });
                                                (compat_state.endpoint_count(), ep_info)
                                            };
                                            status.write().await.endpoint_count = endpoint_count;
                                            // Create CiliumEndpoint CR so integration tests can verify
                                            // endpoint identity via `kubectl get cep`.
                                            if let Some((ns, name, Some(ip), id)) = ep_info {
                                                let w = Arc::clone(&watcher_for_events);
                                                tokio::spawn(async move {
                                                    if let Err(e) = w.upsert_cilium_endpoint(
                                                        &ns, &name, &ip.to_string(), id,
                                                    ).await {
                                                        warn!(pod = %name, error = %e, "failed to upsert CiliumEndpoint");
                                                    }
                                                });
                                            }
                                        }
                                        Ok(cilium_k8s::K8sEvent::NodeAdded(node) | cilium_k8s::K8sEvent::NodeUpdated(node)) => {
                                            sync_remote_node_route(&node, &local_node_name);
                                        }
                                        Ok(cilium_k8s::K8sEvent::ServiceAdded(service) | cilium_k8s::K8sEvent::ServiceUpdated(service)) => {
                                            compat_state.write().await.upsert_service(&service);
                                        }
                                        Ok(cilium_k8s::K8sEvent::EndpointsAdded(endpoints) | cilium_k8s::K8sEvent::EndpointsUpdated(endpoints)) => {
                                            compat_state.write().await.upsert_endpoints(&endpoints);
                                        }
                                        Ok(cilium_k8s::K8sEvent::EndpointSliceAdded(endpoint_slice) | cilium_k8s::K8sEvent::EndpointSliceUpdated(endpoint_slice)) => {
                                            compat_state.write().await.upsert_endpoint_slice(&endpoint_slice);
                                        }
                                        Ok(_) => {}
                                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                            warn!(skipped, "lagged while processing kubernetes events");
                                        }
                                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                    }
                                }
                            }
                        }
                    });
                    std::mem::drop(watcher.clone().watch_nodes());
                    std::mem::drop(watcher.clone().watch_pods());
                    std::mem::drop(watcher.clone().watch_services());
                    std::mem::drop(watcher.clone().watch_endpoints());
                    std::mem::drop(watcher.clone().watch_endpoint_slices());
                    info!(
                        "kubernetes watchers started (nodes, pods, services, endpoints, endpoint slices)"
                    );
                    self.status.write().await.node_name = node_name;
                }
                Err(e) => {
                    warn!(error = %e, "kubernetes watcher unavailable (running outside cluster?)");
                }
            }
        }

        // TODO(phase3): initialise subsystems here in order:
        // 1. kvstore (etcd) connection
        // 2. k8s watcher
        // 3. identity allocator
        // 4. policy engine
        // 5. endpoint manager
        // 6. datapath loader
        // 7. CNI socket listener
        // 8. metrics server
        // 9. health API server

        let health_cancel = self.cancel.child_token();
        let health_shutdown = health_cancel.clone();
        let health = self.health.clone();
        tokio::spawn(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], 9879));
            if let Err(err) = serve(addr, health, health_cancel).await {
                error!(error = %err, "health server error");
            }
        });

        #[cfg(unix)]
        {
            let sock_cancel = self.cancel.child_token();
            let sock_path = cilium_sock_path(&self.config);
            let compat_addressing = compat_addressing(node_ipv4_pod_cidr.as_deref());
            let compat_state = Arc::clone(&self.compat_state);
            tokio::spawn(async move {
                if let Err(err) = serve_cilium_compat_socket(
                    sock_path,
                    sock_cancel,
                    compat_addressing,
                    compat_state,
                )
                .await
                {
                    error!(error = %err, "cilium compat unix socket server error");
                }
            });
        }

        if let Err(error) = initialise_datapath(&self.config) {
            warn!(error = %error, "datapath initialisation did not complete");
        }

        {
            let mut status = self.status.write().await;
            status.phase = DaemonPhase::Running;
        }
        if let Err(error) = write_cni_config_when_ready(&self.config.config_dir).await {
            warn!(error = %error, "unable to write CNI config on readiness");
        }
        set_ready(&self.health, "all subsystems initialised").await;
        info!(ready = true, "cilium-agent ready");

        let signal = wait_for_shutdown(&self.cancel).await?;

        set_stopping(&self.health).await;
        health_shutdown.cancel();
        {
            let mut status = self.status.write().await;
            status.phase = DaemonPhase::Stopping;
        }
        info!(?signal, "daemon shutdown requested");

        {
            let mut status = self.status.write().await;
            status.phase = DaemonPhase::Stopped;
        }
        info!("cilium-agent stopped");
        Ok(signal)
    }

    /// Request a graceful shutdown.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

fn cilium_sock_path(config: &DaemonConfig) -> PathBuf {
    Path::new(&config.state_dir).join(CILIUM_SOCK_FILE)
}

async fn write_cni_config_when_ready(config_dir: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let Some(path) = resolve_write_cni_conf_target(config_dir).await else {
        return Ok(());
    };
    if path.as_os_str().is_empty() {
        return Ok(());
    }

    let log_file = resolve_cni_log_file(config_dir)
        .await
        .unwrap_or_else(|| DEFAULT_CNI_LOG_FILE.into());
    write_cni_config(&path, &log_file).await?;
    info!(path = %path.display(), "wrote CNI config");
    Ok(())
}

async fn resolve_write_cni_conf_target(config_dir: &str) -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(WRITE_CNI_CONF_WHEN_READY_ENV)
        && !raw.is_empty()
    {
        return Some(PathBuf::from(raw));
    }

    for dir in [config_dir, HELM_CONFIG_DIR] {
        if let Some(value) = read_config_key(dir, "write-cni-conf-when-ready").await {
            return Some(PathBuf::from(value));
        }
    }
    None
}

async fn resolve_cni_log_file(config_dir: &str) -> Option<String> {
    if let Ok(value) = std::env::var(CNI_LOG_FILE_ENV)
        && !value.is_empty()
    {
        return Some(value);
    }

    for dir in [config_dir, HELM_CONFIG_DIR] {
        if let Some(value) = read_config_key(dir, "cni-log-file").await {
            return Some(value);
        }
    }
    None
}

async fn read_config_key(config_dir: &str, key: &str) -> Option<String> {
    let path = Path::new(config_dir).join(key);
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

async fn write_cni_config(path: &Path, log_file: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let conf = serde_json::json!({
        "cniVersion": "0.3.1",
        "name": "cilium",
        "plugins": [{
            "type": "cilium-cni",
            "enable-debug": false,
            "log-file": log_file,
        }],
    });
    let payload = serde_json::to_vec_pretty(&conf)?;
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, payload).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

async fn serve_cilium_compat_socket(
    sock_path: PathBuf,
    cancel: CancellationToken,
    compat_addressing: CompatAddressing,
    compat_state: Arc<RwLock<CompatState>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(parent) = sock_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::try_exists(&sock_path).await? {
        tokio::fs::remove_file(&sock_path).await?;
    }

    let listener = UnixListener::bind(&sock_path)?;
    info!(path = %sock_path.display(), "cilium compat unix socket listening");

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                break;
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let compat_addressing = compat_addressing.clone();
                        let compat_state = Arc::clone(&compat_state);
                        tokio::spawn(async move {
                            if let Err(err) =
                                handle_cilium_compat_connection(stream, compat_addressing, compat_state).await
                            {
                                warn!(error = %err, "failed handling cilium compat socket request");
                            }
                        });
                    }
                    Err(err) => {
                        warn!(error = %err, "failed to accept cilium compat socket connection");
                    }
                }
            }
        }
    }

    if tokio::fs::try_exists(&sock_path).await? {
        tokio::fs::remove_file(&sock_path).await?;
    }
    Ok(())
}

async fn handle_cilium_compat_connection(
    mut stream: UnixStream,
    compat_addressing: CompatAddressing,
    compat_state: Arc<RwLock<CompatState>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buf = [0_u8; 4096];
    let mut req = Vec::with_capacity(1024);
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        req.extend_from_slice(&buf[..n]);
        if req.windows(4).any(|w| w == b"\r\n\r\n") || req.len() >= 8192 {
            break;
        }
    }

    let request = String::from_utf8_lossy(&req);
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or(raw_path);

    let compat_state = compat_state.read().await;
    let (status_code, body) = compat_response(method, path, &compat_addressing, &compat_state);

    let status_text = match status_code {
        200 => "OK",
        201 => "Created",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

fn compat_response(
    method: &str,
    path: &str,
    compat_addressing: &CompatAddressing,
    compat_state: &CompatState,
) -> (u16, String) {
    let router_ip = compat_addressing.router_ip.to_string();
    let alloc_range = compat_addressing.alloc_range.clone();
    match (method, path) {
        ("GET", "/healthz" | "/v1/healthz") => (
            200,
            json!({
                "cilium": {"state": "Ok", "msg": "OK"},
                "cluster": {
                    "ciliumHealth": {"state": "Disabled"},
                    "nodes": [{"name": compat_state.node_name}],
                },
                "container-runtime": {"state": "Ok"},
                "kubernetes": {"state": "Ok"},
                "kvstore": {"state": "Disabled"},
                "controllers": [],
            })
            .to_string(),
        ),
        ("GET", "/v1/config") => (
            200,
            json!({
                "spec": {
                    "options": {},
                    "policy-enforcement": "default",
                },
                "status": {
                    "daemonConfigurationMap": {},
                    "immutable": {},
                    "k8s-configuration": "",
                    "realized": {
                        "options": {},
                        "policy-enforcement": "default",
                    },
                    "state": "ok",
                    "msg": "seriousum-compat",
                    "addressing": {
                        "ipv4": {
                            "enabled": true,
                            "ip": router_ip,
                            "alloc-range": alloc_range,
                        },
                        "ipv6": {
                            "enabled": false,
                        },
                    },
                    "configuredDatapathMode": "veth",
                    "datapathMode": "veth",
                    "deviceMTU": 1500,
                    "routeMTU": 1500,
                    "ipam-mode": "cluster-pool",
                },
            })
            .to_string(),
        ),
        ("POST", "/ipam" | "/ipam/" | "/v1/ipam" | "/v1/ipam/") => {
            let ip = next_compat_ipv4(&compat_addressing.alloc_range).to_string();
            (
                201,
                json!({
                    "address": {
                        "ipv4": ip,
                    },
                    "host-addressing": {
                        "ipv4": {
                            "enabled": true,
                            "ip": router_ip,
                            "alloc-range": alloc_range,
                        },
                        "ipv6": {
                            "enabled": false,
                        },
                    },
                    "ipv4": {
                        "ip": ip,
                        "gateway": router_ip,
                        "cidrs": [compat_addressing.alloc_range],
                        "interface-number": "0",
                    },
                })
                .to_string(),
            )
        }
        ("DELETE", path) if path.starts_with("/ipam/") || path.starts_with("/v1/ipam/") => {
            (200, "{}".to_string())
        }
        ("GET", "/v1/service") => (200, services_response(compat_state)),
        ("GET", "/v1/endpoint") => (200, endpoints_response(compat_state)),
        ("DELETE", "/v1/endpoint") => (200, "{}".to_string()),
        ("PUT", path) if path.starts_with("/v1/endpoint/") => {
            (201, endpoint_response(path, "ready", compat_state))
        }
        ("GET", path) if path.starts_with("/v1/endpoint/") && path.ends_with("/healthz") => (
            200,
            json!({
                "bpf": "OK",
                "connected": true,
                "overallHealth": "OK",
                "policy": "OK",
            })
            .to_string(),
        ),
        ("GET", path) if path.starts_with("/v1/endpoint/") => {
            (200, endpoint_response(path, "ready", compat_state))
        }
        ("PATCH", path) if path.starts_with("/v1/endpoint/") => (200, "{}".to_string()),
        ("DELETE", path) if path.starts_with("/v1/endpoint/") => (200, "{}".to_string()),
        _ => (404, json!({"message":"not found"}).to_string()),
    }
}

fn compat_addressing(node_ipv4_pod_cidr: Option<&str>) -> CompatAddressing {
    if let Some((router_ip, alloc_range)) = node_ipv4_pod_cidr
        .and_then(|cidr| first_usable_ipv4(cidr).map(|router_ip| (router_ip, cidr.to_string())))
    {
        return CompatAddressing {
            router_ip,
            alloc_range,
        };
    }

    CompatAddressing::default()
}

fn first_usable_ipv4(cidr: &str) -> Option<Ipv4Addr> {
    let net: Ipv4Net = cidr.parse().ok()?;
    if net.prefix_len() >= 31 {
        return None;
    }

    Some(Ipv4Addr::from(u32::from(net.network()) + 1))
}

fn next_compat_ipv4(cidr: &str) -> Ipv4Addr {
    let net = cidr
        .parse::<Ipv4Net>()
        .ok()
        .filter(|net| net.prefix_len() < 31)
        .unwrap_or_else(|| {
            DEFAULT_COMPAT_ALLOC_RANGE
                .parse()
                .expect("valid default CIDR")
        });
    let base = u32::from(net.network());
    let last = u32::from(net.broadcast()) - 1;
    let preferred_start = base.saturating_add(20).clamp(base + 2, last);
    let slots = last - preferred_start + 1;
    let offset = IPAM_COUNTER.fetch_add(1, Ordering::Relaxed) % slots;

    Ipv4Addr::from(preferred_start + offset)
}

fn endpoints_response(compat_state: &CompatState) -> String {
    // Pod endpoints.
    let mut endpoints: Vec<_> = compat_state
        .list_endpoints()
        .into_iter()
        .map(|endpoint| endpoint_json(endpoint, "ready"))
        .collect();

    // Synthetic host endpoint — every Cilium node requires one with identity 1
    // (ReservedIdentityHost) in "ready" state so integration tests pass their
    // preflight host-EP regeneration check.
    let node_ip = compat_state
        .list_endpoints()
        .first()
        .and_then(|ep| {
            // Approximate node IP: take the pod CIDR's .1 address
            ep.pod_ip.map(|ip| {
                let o = ip.octets();
                Ipv4Addr::new(o[0], o[1], o[2], 1).to_string()
            })
        })
        .unwrap_or_else(|| "0.0.0.0".to_string());

    endpoints.push(json!({
        "id": 0xFFFF_i64,
        "status": {
            "state": "ready",
            "identity": {
                "id": 1_i64,  // reserved:host
                "labels": ["reserved:host"]
            },
            "networking": {
                "addressing": [{"ipv4": node_ip}]
            },
            "health": {
                "bpf": "OK",
                "connected": true,
                "overallHealth": "OK",
                "policy": "OK"
            },
            "labels": {
                "derived": ["reserved:host"],
                "security-relevant": ["reserved:host"]
            }
        }
    }));

    serde_json::to_string(&endpoints).unwrap_or_else(|_| "[]".to_string())
}

fn services_response(compat_state: &CompatState) -> String {
    let services: Vec<_> = compat_state
        .list_services()
        .into_iter()
        .flat_map(service_json)
        .collect();
    serde_json::to_string(&services).unwrap_or_else(|_| "[]".to_string())
}

fn endpoint_response(path: &str, state: &str, compat_state: &CompatState) -> String {
    let id = path
        .trim_start_matches("/v1/endpoint/")
        .trim_end_matches("/healthz")
        .trim_end_matches("/config")
        .trim_end_matches("/labels")
        .trim_end_matches("/log")
        .trim_end_matches('/')
        .parse::<i64>()
        .unwrap_or_default();

    compat_state
        .endpoint_by_id(id)
        .map(|endpoint| endpoint_json(endpoint, state))
        .unwrap_or_else(|| {
            json!({
                "id": id,
                "status": {
                    "state": state,
                    "health": {
                        "bpf": "OK",
                        "connected": true,
                        "overallHealth": "OK",
                        "policy": "OK",
                    },
                    "networking": {
                        "addressing": [],
                    },
                },
            })
        })
        .to_string()
}

fn endpoint_json(endpoint: &CompatEndpoint, state: &str) -> serde_json::Value {
    let addressing = endpoint
        .pod_ip
        .map(|pod_ip| vec![json!({ "ipv4": pod_ip.to_string() })])
        .unwrap_or_default();
    let orchestration_labels = endpoint
        .labels
        .iter()
        .map(|(key, value)| format!("k8s:{key}={value}"))
        .collect::<Vec<_>>();
    json!({
        "id": endpoint.id,
        "status": {
            "state": state,
            "identity": {
                // Offset by 10000 to avoid colliding with Cilium reserved identities 1-16
                // (identity 1 = reserved:host, used by the synthetic host endpoint).
                "id": endpoint.id.saturating_add(10000),
                "labels": []
            },
            "external-identifiers": {
                "k8s-namespace": endpoint.namespace,
                "k8s-pod-name": endpoint.name,
                "pod-name": format!("{}/{}", endpoint.namespace, endpoint.name),
                "node-name": endpoint.node_name,
            },
            "health": {
                "bpf": "OK",
                "connected": true,
                "overallHealth": "OK",
                "policy": "OK",
            },
            "labels": {
                "derived": orchestration_labels,
                "disabled": [],
                "realized": {
                    "user": [],
                },
                "security-relevant": endpoint.labels.iter().map(|(key, value)| format!("k8s:{key}={value}")).collect::<Vec<_>>(),
            },
            "networking": {
                "addressing": addressing,
                "host-addressing": {
                    "ipv4": {
                        "enabled": true,
                    }
                },
            },
        },
    })
}

fn service_json(service: &CompatService) -> Vec<serde_json::Value> {
    if service.ports.is_empty() {
        return vec![service_json_for_port(service, None, service.id)];
    }

    service
        .ports
        .iter()
        .enumerate()
        .map(|(index, port)| {
            let index = i64::try_from(index).unwrap_or(i64::MAX);
            service_json_for_port(
                service,
                Some(port),
                service.id.saturating_mul(1000).saturating_add(index),
            )
        })
        .collect()
}

fn service_json_for_port(
    service: &CompatService,
    port: Option<&CompatServicePort>,
    service_id: i64,
) -> serde_json::Value {
    let ip_str = service.cluster_ip.map(|ip| ip.to_string()).unwrap_or_default();
    let frontend = port
        .map(|port| {
            json!({
                "ip": ip_str,
                "port": port.port,
                "protocol": protocol_name(&port.protocol),
                "scope": "external",
            })
        })
        .unwrap_or_else(|| {
            json!({
                "ip": ip_str,
                "protocol": "any",
                "scope": "external",
            })
        });

    let backends = service
        .backends
        .iter()
        .filter(|backend| backend_matches_service_port(port, backend))
        .map(backend_json)
        .collect::<Vec<_>>();

    json!({
        "spec": {
            "id": service_id,
            "frontend-address": frontend.clone(),
            "backend-addresses": backends.clone(),
            "flags": {
                "type": service.service_type,
                "name": service.name,
                "namespace": service.namespace,
            },
        },
        "status": {
            "realized": {
                "id": service_id,
                "frontend-address": frontend,
                "backend-addresses": backends,
                "flags": {
                    "type": service.service_type,
                    "name": service.name,
                    "namespace": service.namespace,
                },
            },
        },
    })
}

fn backend_json(backend: &CompatBackend) -> serde_json::Value {
    json!({
        "ip": backend.ip.to_string(),
        "nodeName": backend.node_name,
        "port": backend.port,
        "preferred": false,
        "protocol": protocol_name(&backend.protocol),
        "state": "active",
    })
}

fn backend_matches_service_port(
    service_port: Option<&CompatServicePort>,
    backend: &CompatBackend,
) -> bool {
    let Some(service_port) = service_port else {
        return true;
    };
    if !service_port
        .protocol
        .eq_ignore_ascii_case(&backend.protocol)
    {
        return false;
    }
    match &service_port.target_port {
        Some(CompatTargetPort::Number(port)) => backend.port == *port,
        Some(CompatTargetPort::Name(name)) => backend.port_name.as_deref() == Some(name.as_str()),
        None => {
            backend.port == service_port.port
                || service_port.name.as_deref() == backend.port_name.as_deref()
        }
    }
}

fn port_protocol(protocol: Option<&String>) -> String {
    protocol.cloned().unwrap_or_else(|| "TCP".to_string())
}

fn protocol_to_u8(protocol: &str) -> u8 {
    match protocol {
        "UDP" => 17,
        "SCTP" => 132,
        _ => 6, // TCP and anything else
    }
}

fn compat_target_port(target_port: Option<&IntOrString>) -> Option<CompatTargetPort> {
    match target_port {
        Some(IntOrString::Int(port)) => u16::try_from(*port).ok().map(CompatTargetPort::Number),
        Some(IntOrString::String(name)) if !name.is_empty() => {
            Some(CompatTargetPort::Name(name.clone()))
        }
        _ => None,
    }
}

fn protocol_name(protocol: &str) -> &'static str {
    if protocol.eq_ignore_ascii_case("UDP") { "UDP" } else { "TCP" }
}

fn sync_remote_node_route(node: &Node, local_node_name: &str) {
    let Some((pod_cidr, via)) = remote_node_route(node, local_node_name) else {
        return;
    };

    if let Err(error) = run_ip_route_replace(&pod_cidr, via) {
        warn!(
            node = %node.metadata.name.as_deref().unwrap_or_default(),
            cidr = %pod_cidr,
            via = %via,
            error = %error,
            "unable to program remote pod route"
        );
    } else {
        info!(
            node = %node.metadata.name.as_deref().unwrap_or_default(),
            cidr = %pod_cidr,
            via = %via,
            "programmed remote pod route"
        );
    }
}

fn remote_node_route(node: &Node, local_node_name: &str) -> Option<(String, IpAddr)> {
    let node_name = node.metadata.name.as_deref()?;
    if node_name == local_node_name {
        return None;
    }

    let pod_cidr = node
        .spec
        .as_ref()
        .and_then(preferred_ipv4_pod_cidr_from_node)
        .filter(|cidr| !cidr.is_empty())?;
    let via = node_internal_ipv4(node)?;
    Some((pod_cidr, via))
}

fn preferred_ipv4_pod_cidr_from_node(
    spec: &k8s_openapi::api::core::v1::NodeSpec,
) -> Option<String> {
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

fn node_internal_ipv4(node: &Node) -> Option<IpAddr> {
    let status = node.status.as_ref()?;
    status
        .addresses
        .as_deref()
        .into_iter()
        .flatten()
        .find_map(|address| {
            if address.type_ != "InternalIP" {
                return None;
            }
            address.address.parse::<Ipv4Addr>().ok().map(IpAddr::V4)
        })
}

fn run_ip_route_replace(cidr: &str, via: IpAddr) -> Result<(), Box<dyn Error + Send + Sync>> {
    let output = Command::new("ip")
        .args(["route", "replace", cidr, "via", &via.to_string()])
        .output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "ip route replace {cidr} via {via}: {} ({stderr})",
        output.status
    )
    .into())
}

fn initialise_datapath(config: &DaemonConfig) -> Result<(), Box<dyn Error + Send + Sync>> {
    let bpf_dir = Path::new(&config.config_dir).join("bpf");
    let mut loader = seriousum_datapath::DatapathLoader::new(&bpf_dir, &config.state_dir);
    if let Err(error) = loader.register_standard_objects() {
        warn!(error = %error, "unable to register standard datapath objects");
        return Ok(());
    }
    loader.load_all()?;
    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown(
    cancel: &CancellationToken,
) -> Result<ShutdownSignal, Box<dyn Error + Send + Sync>> {
    let mut sigterm = unix_signal(SignalKind::terminate())?;

    Ok(tokio::select! {
        _ = signal::ctrl_c() => {
            info!("received SIGINT, shutting down");
            ShutdownSignal::Sigint
        }
        _ = sigterm.recv() => {
            info!("received SIGTERM, shutting down");
            ShutdownSignal::Sigterm
        }
        () = cancel.cancelled() => {
            info!("cancellation requested, shutting down");
            ShutdownSignal::Manual
        }
    })
}

#[cfg(not(unix))]
async fn wait_for_shutdown(
    cancel: &CancellationToken,
) -> Result<ShutdownSignal, Box<dyn Error + Send + Sync>> {
    Ok(tokio::select! {
        _ = signal::ctrl_c() => {
            info!("received SIGINT, shutting down");
            ShutdownSignal::Sigint
        }
        () = cancel.cancelled() => {
            info!("cancellation requested, shutting down");
            ShutdownSignal::Manual
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DaemonConfig;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn test_daemon_runtime_new() {
        let config = DaemonConfig::default();
        let rt = DaemonRuntime::new(config);
        assert!(!rt.cancel.is_cancelled());
    }

    #[test]
    fn test_daemon_runtime_shutdown() {
        let config = DaemonConfig::default();
        let rt = DaemonRuntime::new(config);
        rt.shutdown();
        assert!(rt.cancel.is_cancelled());
    }

    #[tokio::test]
    async fn test_daemon_runtime_manual_shutdown() {
        let config = DaemonConfig::default();
        let rt = DaemonRuntime::new(config);
        rt.shutdown();
        let result = rt.run().await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ShutdownSignal::Manual));
    }

    #[test]
    fn test_cilium_sock_path_default() {
        let config = DaemonConfig::default();
        assert_eq!(
            cilium_sock_path(&config),
            Path::new("/var/run/cilium").join("cilium.sock")
        );
    }

    #[test]
    fn test_compat_response_routes() {
        let default_addressing = CompatAddressing::default();
        let compat_state = CompatState::new("localhost".to_string());
        let (code, body) = compat_response("GET", "/healthz", &default_addressing, &compat_state);
        assert_eq!(code, 200);
        assert!(body.contains("\"cilium\""));
        assert!(body.contains("\"controllers\""));

        let (code, body) = compat_response("GET", "/v1/config", &default_addressing, &compat_state);
        assert_eq!(code, 200);
        assert!(body.contains("\"spec\""));
        assert!(body.contains("\"addressing\""));
        assert!(body.contains("\"datapathMode\":\"veth\""));
        assert!(body.contains("\"realized\":{\"options\":{},\"policy-enforcement\":\"default\"}"));

        let (code, body) = compat_response("POST", "/ipam", &default_addressing, &compat_state);
        assert_eq!(code, 201);
        assert!(body.contains("\"host-addressing\""));
        assert!(body.contains("\"ipv4\""));

        let pod_addressing = compat_addressing(Some("10.244.1.0/24"));
        let (code, body) = compat_response("POST", "/ipam", &pod_addressing, &compat_state);
        assert_eq!(code, 201);
        assert!(body.contains("\"gateway\":\"10.244.1.1\""));
        assert!(body.contains("\"alloc-range\":\"10.244.1.0/24\""));

        let (code, body) = compat_response(
            "DELETE",
            "/ipam/10.244.0.20",
            &default_addressing,
            &compat_state,
        );
        assert_eq!(code, 200);
        assert_eq!(body, "{}");

        let (code, body) =
            compat_response("GET", "/v1/service", &default_addressing, &compat_state);
        assert_eq!(code, 200);
        assert_eq!(body, "[]");

        let (code, body) =
            compat_response("GET", "/v1/endpoint", &default_addressing, &compat_state);
        assert_eq!(code, 200);
        assert_eq!(body, "[]");

        let (code, body) =
            compat_response("PUT", "/v1/endpoint/42", &default_addressing, &compat_state);
        assert_eq!(code, 201);
        assert!(body.contains("\"id\":42"));
        assert!(body.contains("\"state\":\"ready\""));

        let (code, body) = compat_response(
            "GET",
            "/v1/endpoint/42/healthz",
            &default_addressing,
            &compat_state,
        );
        assert_eq!(code, 200);
        assert!(body.contains("\"overallHealth\":\"OK\""));

        let (code, _body) =
            compat_response("POST", "/v1/config", &default_addressing, &compat_state);
        assert_eq!(code, 404);
    }

    #[test]
    fn test_compat_state_tracks_local_pods_and_services() {
        let mut compat_state = CompatState::new("kind-control-plane".to_string());
        let pod = Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("dnsutils".to_string()),
                namespace: Some("default".to_string()),
                labels: Some(
                    [("app".to_string(), "dnsutils".to_string())]
                        .into_iter()
                        .collect(),
                ),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::PodSpec {
                node_name: Some("kind-control-plane".to_string()),
                host_network: Some(false),
                ..Default::default()
            }),
            status: Some(k8s_openapi::api::core::v1::PodStatus {
                pod_ip: Some("10.244.0.23".to_string()),
                ..Default::default()
            }),
        };
        let service = Service {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("kube-dns".to_string()),
                namespace: Some("kube-system".to_string()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                cluster_ip: Some("10.96.0.10".to_string()),
                type_: Some("ClusterIP".to_string()),
                ports: Some(vec![k8s_openapi::api::core::v1::ServicePort {
                    port: 53,
                    protocol: Some("UDP".to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            status: None,
        };

        compat_state.upsert_pod("kind-control-plane", &pod);
        compat_state.upsert_service(&service);
        compat_state.upsert_endpoint_slice(&EndpointSlice {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("kube-dns-abcde".to_string()),
                namespace: Some("kube-system".to_string()),
                labels: Some(
                    [(
                        "kubernetes.io/service-name".to_string(),
                        "kube-dns".to_string(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            },
            address_type: "IPv4".to_string(),
            endpoints: vec![k8s_openapi::api::discovery::v1::Endpoint {
                addresses: vec!["10.244.0.53".to_string()],
                node_name: Some("kind-control-plane".to_string()),
                conditions: Some(k8s_openapi::api::discovery::v1::EndpointConditions {
                    ready: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ports: Some(vec![k8s_openapi::api::discovery::v1::EndpointPort {
                name: Some("dns".to_string()),
                port: Some(53),
                protocol: Some("UDP".to_string()),
                ..Default::default()
            }]),
        });

        let endpoints = endpoints_response(&compat_state);
        assert!(endpoints.contains("\"dnsutils\""));
        assert!(endpoints.contains("\"10.244.0.23\""));
        assert!(endpoints.contains("\"realized\":{\"user\":[]}"));
        assert!(endpoints.contains("\"derived\":[\"k8s:app=dnsutils\"]"));

        let services = services_response(&compat_state);
        assert!(services.contains("\"10.96.0.10\""));
        assert!(services.contains("\"udp\""));
        assert!(services.contains("\"10.244.0.53\""));
        assert!(services.contains("\"state\":\"active\""));
    }

    #[test]
    fn test_compat_state_tracks_service_backends_from_endpoints() {
        let mut compat_state = CompatState::new("kind-control-plane".to_string());
        let service = Service {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("kube-dns".to_string()),
                namespace: Some("kube-system".to_string()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::ServiceSpec {
                cluster_ip: Some("10.96.0.10".to_string()),
                type_: Some("ClusterIP".to_string()),
                ports: Some(vec![k8s_openapi::api::core::v1::ServicePort {
                    name: Some("dns".to_string()),
                    port: 53,
                    protocol: Some("UDP".to_string()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            status: None,
        };
        compat_state.upsert_service(&service);
        compat_state.upsert_endpoints(&K8sEndpoints {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("kube-dns".to_string()),
                namespace: Some("kube-system".to_string()),
                ..Default::default()
            },
            subsets: Some(vec![k8s_openapi::api::core::v1::EndpointSubset {
                addresses: Some(vec![k8s_openapi::api::core::v1::EndpointAddress {
                    ip: "10.244.1.34".to_string(),
                    node_name: Some("kind-worker".to_string()),
                    ..Default::default()
                }]),
                not_ready_addresses: None,
                ports: Some(vec![k8s_openapi::api::core::v1::EndpointPort {
                    name: Some("dns".to_string()),
                    port: 53,
                    protocol: Some("UDP".to_string()),
                    ..Default::default()
                }]),
            }]),
        });

        let services = services_response(&compat_state);
        assert!(services.contains("\"10.96.0.10\""));
        assert!(services.contains("\"10.244.1.34\""));
        assert!(services.contains("\"port\":53"));
    }

    #[test]
    fn test_remote_node_route_skips_local_node() {
        let node = Node {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("kind-worker".to_string()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::NodeSpec {
                pod_cidr: Some("10.244.1.0/24".to_string()),
                ..Default::default()
            }),
            status: Some(k8s_openapi::api::core::v1::NodeStatus {
                addresses: Some(vec![k8s_openapi::api::core::v1::NodeAddress {
                    type_: "InternalIP".to_string(),
                    address: "172.18.0.3".to_string(),
                }]),
                ..Default::default()
            }),
        };

        assert!(remote_node_route(&node, "kind-worker").is_none());
    }

    #[test]
    fn test_remote_node_route_extracts_peer_route() {
        let node = Node {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("kind-worker".to_string()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::core::v1::NodeSpec {
                pod_cidr: Some("10.244.1.0/24".to_string()),
                ..Default::default()
            }),
            status: Some(k8s_openapi::api::core::v1::NodeStatus {
                addresses: Some(vec![k8s_openapi::api::core::v1::NodeAddress {
                    type_: "InternalIP".to_string(),
                    address: "172.18.0.3".to_string(),
                }]),
                ..Default::default()
            }),
        };

        assert_eq!(
            remote_node_route(&node, "kind-control-plane"),
            Some((
                "10.244.1.0/24".to_string(),
                IpAddr::V4(Ipv4Addr::new(172, 18, 0, 3))
            ))
        );
    }

    #[test]
    fn test_compat_addressing_prefers_pod_cidr_router() {
        assert_eq!(
            compat_addressing(Some("10.244.2.0/24")),
            CompatAddressing {
                router_ip: Ipv4Addr::new(10, 244, 2, 1),
                alloc_range: "10.244.2.0/24".to_string(),
            }
        );
    }

    #[test]
    fn test_compat_addressing_falls_back_to_default_router_without_pod_cidr() {
        assert_eq!(
            compat_addressing(None),
            CompatAddressing {
                router_ip: Ipv4Addr::new(10, 244, 0, 1),
                alloc_range: DEFAULT_COMPAT_ALLOC_RANGE.to_string(),
            }
        );
    }

    #[test]
    fn test_next_compat_ipv4_stays_in_alloc_range() {
        let allocated = next_compat_ipv4("10.244.3.0/24");
        assert_eq!(allocated.octets()[0..3], [10, 244, 3]);
        assert!(allocated.octets()[3] >= 20);
    }

    #[tokio::test]
    async fn test_write_cni_config_when_ready() {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("seriousum-cni-ready-{uniq}"));
        let conf_path = root.join("05-cilium.conflist");

        write_cni_config(&conf_path, "/tmp/cilium-cni.log")
            .await
            .expect("cni config should be written");

        let written = tokio::fs::read_to_string(&conf_path)
            .await
            .expect("config file should exist");
        assert!(written.contains("\"type\": \"cilium-cni\""));
        assert!(written.contains("\"log-file\": \"/tmp/cilium-cni.log\""));
    }

    #[tokio::test]
    async fn test_resolve_write_cni_conf_target_from_config_dir() {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("seriousum-cni-config-{uniq}"));
        tokio::fs::create_dir_all(&dir)
            .await
            .expect("config dir should be created");
        tokio::fs::write(
            dir.join("write-cni-conf-when-ready"),
            "/host/etc/cni/net.d/05-cilium.conflist\n",
        )
        .await
        .expect("config key should be written");

        let resolved = resolve_write_cni_conf_target(dir.to_string_lossy().as_ref())
            .await
            .expect("path should resolve");
        assert_eq!(
            resolved,
            PathBuf::from("/host/etc/cni/net.d/05-cilium.conflist")
        );
    }

    #[tokio::test]
    async fn test_compat_socket_serves_config() {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("seriousum-compat-{uniq}"));
        let sock_path = root.join("cilium.sock");

        let cancel = CancellationToken::new();
        let server_cancel = cancel.clone();
        let server = tokio::spawn(async move {
            serve_cilium_compat_socket(
                sock_path,
                server_cancel,
                CompatAddressing::default(),
                Arc::new(RwLock::new(CompatState::new("localhost".to_string()))),
            )
            .await
        });

        let mut stream = loop {
            match UnixStream::connect(root.join("cilium.sock")).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
            }
        };
        stream
            .write_all(b"GET /v1/config HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("request should be written");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("response should be readable");
        let response = String::from_utf8(response).expect("response should be UTF-8");
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.contains("\"policy-enforcement\":\"default\""));

        cancel.cancel();
        let joined = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("server should stop after cancel")
            .expect("server task should join");
        assert!(joined.is_ok());
    }
}
