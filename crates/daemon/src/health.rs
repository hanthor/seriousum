//! Minimal health and readiness HTTP server.
//!
//! Exposes:
//!   GET /healthz          — liveness (always OK if agent is running)
//!   GET /readyz           — readiness (OK once agent has fully started)
//!   GET /api/v1/healthz   — Cilium-compatible health endpoint

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// Agent readiness state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadinessState {
    /// Agent is starting up.
    Starting,
    /// Agent is ready to serve traffic.
    Ready,
    /// Agent is shutting down.
    Stopping,
}

/// Shared health status.
#[derive(Debug)]
pub struct HealthStatus {
    /// Current readiness state.
    pub state: ReadinessState,
    /// Human-readable status message.
    pub message: String,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            state: ReadinessState::Starting,
            message: "agent starting".to_string(),
        }
    }
}

/// Shared health state handle.
pub type SharedHealth = Arc<RwLock<HealthStatus>>;

/// Create a new shared health status handle.
pub fn new_health() -> SharedHealth {
    Arc::new(RwLock::new(HealthStatus::default()))
}

/// Mark the agent as ready.
pub async fn set_ready(health: &SharedHealth, message: impl Into<String>) {
    let mut h = health.write().await;
    h.state = ReadinessState::Ready;
    h.message = message.into();
    info!("agent marked ready");
}

/// Mark the agent as stopping.
pub async fn set_stopping(health: &SharedHealth) {
    let mut h = health.write().await;
    h.state = ReadinessState::Stopping;
    h.message = "agent stopping".to_string();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Liveness,
    Readiness,
    Compatible,
}

fn parse_endpoint(request: &str) -> Option<Endpoint> {
    let request_line = request.lines().next()?;
    let mut parts = request_line.split_whitespace();

    match (parts.next(), parts.next()) {
        (Some("GET"), Some("/healthz")) => Some(Endpoint::Liveness),
        (Some("GET"), Some("/readyz")) => Some(Endpoint::Readiness),
        (Some("GET"), Some("/api/v1/healthz")) => Some(Endpoint::Compatible),
        _ => None,
    }
}

fn state_name(state: &ReadinessState) -> &'static str {
    match state {
        ReadinessState::Starting => "starting",
        ReadinessState::Ready => "ok",
        ReadinessState::Stopping => "stopping",
    }
}

fn endpoint_response(endpoint: Endpoint, health: &HealthStatus) -> (u16, String) {
    match endpoint {
        Endpoint::Liveness => {
            let body = json!({
                "status": "ok",
                "state": state_name(&health.state),
                "msg": health.message,
            })
            .to_string();
            (200, body)
        }
        Endpoint::Readiness | Endpoint::Compatible => match health.state {
            ReadinessState::Ready => {
                let body = json!({
                    "status": "ok",
                    "msg": health.message,
                })
                .to_string();
                (200, body)
            }
            ReadinessState::Starting => (503, json!({ "status": "starting" }).to_string()),
            ReadinessState::Stopping => (503, json!({ "status": "stopping" }).to_string()),
        },
    }
}

/// Serve health endpoints on the given address until cancelled.
pub async fn serve(
    addr: SocketAddr,
    health: SharedHealth,
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;
    info!(addr = %addr, "health server listening");

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                info!("health server shutting down");
                return Ok(());
            }
            result = listener.accept() => {
                match result {
                    Ok((mut stream, _peer)) => {
                        let health = health.clone();
                        tokio::spawn(async move {
                            let mut buf = [0u8; 512];
                            let bytes_read = match stream.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(bytes_read) => bytes_read,
                            };

                            let request = String::from_utf8_lossy(&buf[..bytes_read]);
                            let (status, body) = {
                                let h = health.read().await;
                                match parse_endpoint(request.as_ref()) {
                                    Some(endpoint) => endpoint_response(endpoint, &h),
                                    None => (404u16, json!({ "status": "not found" }).to_string()),
                                }
                            };

                            let status_text = match status {
                                200 => "OK",
                                404 => "Not Found",
                                _ => "Service Unavailable",
                            };
                            let response = format!(
                                "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );

                            if let Err(err) = stream.write_all(response.as_bytes()).await {
                                error!(error = %err, "health response write error");
                            }
                        });
                    }
                    Err(err) => error!(error = %err, "health server accept error"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_default_is_starting() {
        let h = new_health();
        let state = h.read().await;
        assert_eq!(state.state, ReadinessState::Starting);
    }

    #[tokio::test]
    async fn test_set_ready() {
        let h = new_health();
        set_ready(&h, "all good").await;
        let state = h.read().await;
        assert_eq!(state.state, ReadinessState::Ready);
        assert_eq!(state.message, "all good");
    }

    #[tokio::test]
    async fn test_set_stopping() {
        let h = new_health();
        set_ready(&h, "ready").await;
        set_stopping(&h).await;
        let state = h.read().await;
        assert_eq!(state.state, ReadinessState::Stopping);
    }

    #[test]
    fn test_liveness_endpoint_is_ok_while_starting() {
        let health = HealthStatus::default();
        let (status, body) = endpoint_response(Endpoint::Liveness, &health);
        assert_eq!(status, 200);
        assert!(body.contains("\"status\":\"ok\""));
    }

    #[test]
    fn test_readiness_endpoint_requires_ready() {
        let health = HealthStatus::default();
        let (status, body) = endpoint_response(Endpoint::Readiness, &health);
        assert_eq!(status, 503);
        assert!(body.contains("starting"));
    }
}
