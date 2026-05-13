use std::error::Error;
use std::net::SocketAddr;

use tokio::signal;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal as unix_signal};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::health::{SharedHealth, new_health, serve, set_ready, set_stopping};
use crate::{DaemonConfig, DaemonPhase, DaemonStatus};

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

        if self.config.enable_k8s_integration {
            match cilium_k8s::K8sWatcher::new().await {
                Ok((watcher, _rx)) => {
                    info!("kubernetes watcher connected");
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
            let addr = SocketAddr::from(([0, 0, 0, 0], 9876));
            if let Err(err) = serve(addr, health, health_cancel).await {
                error!(error = %err, "health server error");
            }
        });

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
}
