use std::error::Error;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

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
use crate::{DaemonConfig, DaemonPhase, DaemonStatus};

const CILIUM_SOCK_FILE: &str = "cilium.sock";

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
    status: RwLock<DaemonStatus>,
    health: SharedHealth,
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
            status: RwLock::new(DaemonStatus::new(node_name)),
            health: new_health(),
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

        if self.config.enable_k8s_integration {
            match cilium_k8s::K8sWatcher::new().await {
                Ok((watcher, _rx)) => {
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
                    match watcher.remove_agent_not_ready_taint(&node_name).await {
                        Ok(true) => {
                            info!(node = %node_name, "removed bootstrap-blocking node taints")
                        }
                        Ok(false) => {
                            info!(node = %node_name, "bootstrap-blocking taints not present on node")
                        }
                        Err(error) => {
                            warn!(node = %node_name, error = %error, "unable to remove bootstrap-blocking taints")
                        }
                    };
                    let watcher = std::sync::Arc::new(watcher);
                    std::mem::drop(watcher.clone().watch_nodes());
                    std::mem::drop(watcher.clone().watch_pods());
                    std::mem::drop(watcher.clone().watch_services());
                    info!("kubernetes watchers started (nodes, pods, services)");
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
            tokio::spawn(async move {
                if let Err(err) = serve_cilium_compat_socket(sock_path, sock_cancel).await {
                    error!(error = %err, "cilium compat unix socket server error");
                }
            });
        }

        {
            let mut status = self.status.write().await;
            status.phase = DaemonPhase::Running;
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

async fn serve_cilium_compat_socket(
    sock_path: PathBuf,
    cancel: CancellationToken,
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
                        tokio::spawn(async move {
                            if let Err(err) = handle_cilium_compat_connection(stream).await {
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

    let (status_code, body) = compat_response(method, path);

    let status_text = if status_code == 200 {
        "OK"
    } else {
        "Not Found"
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

fn compat_response(method: &str, path: &str) -> (u16, String) {
    match (method, path) {
        ("GET", "/healthz") | ("GET", "/v1/healthz") => (
            200,
            json!({
                "cilium": {"state": "Ok", "msg": "OK"},
                "cluster": {
                    "ciliumHealth": {"state": "Disabled"},
                    "nodes": [{"name": "localhost"}],
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
                    "state": "ok",
                    "msg": "seriousum-compat",
                },
            })
            .to_string(),
        ),
        ("GET", "/v1/service") => (200, "[]".to_string()),
        ("GET", "/v1/endpoint") => (200, "[]".to_string()),
        ("DELETE", "/v1/endpoint") => (200, "{}".to_string()),
        ("PUT", path) if path.starts_with("/v1/endpoint/") => {
            (201, endpoint_response(path, "ready"))
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
            (200, endpoint_response(path, "ready"))
        }
        ("PATCH", path) if path.starts_with("/v1/endpoint/") => (200, "{}".to_string()),
        ("DELETE", path) if path.starts_with("/v1/endpoint/") => (200, "{}".to_string()),
        _ => (404, json!({"message":"not found"}).to_string()),
    }
}

fn endpoint_response(path: &str, state: &str) -> String {
    let id = path
        .trim_start_matches("/v1/endpoint/")
        .trim_end_matches("/healthz")
        .trim_end_matches("/config")
        .trim_end_matches("/labels")
        .trim_end_matches("/log")
        .trim_end_matches('/')
        .parse::<i64>()
        .unwrap_or_default();

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
    .to_string()
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
        let (code, body) = compat_response("GET", "/healthz");
        assert_eq!(code, 200);
        assert!(body.contains("\"cilium\""));
        assert!(body.contains("\"controllers\""));

        let (code, body) = compat_response("GET", "/v1/config");
        assert_eq!(code, 200);
        assert!(body.contains("\"spec\""));

        let (code, body) = compat_response("GET", "/v1/service");
        assert_eq!(code, 200);
        assert_eq!(body, "[]");

        let (code, body) = compat_response("GET", "/v1/endpoint");
        assert_eq!(code, 200);
        assert_eq!(body, "[]");

        let (code, body) = compat_response("PUT", "/v1/endpoint/42");
        assert_eq!(code, 201);
        assert!(body.contains("\"id\":42"));
        assert!(body.contains("\"state\":\"ready\""));

        let (code, body) = compat_response("GET", "/v1/endpoint/42/healthz");
        assert_eq!(code, 200);
        assert!(body.contains("\"overallHealth\":\"OK\""));

        let (code, _body) = compat_response("POST", "/v1/config");
        assert_eq!(code, 404);
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
        let server =
            tokio::spawn(async move { serve_cilium_compat_socket(sock_path, server_cancel).await });

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
