//! REST API server setup and routing.

use crate::handlers::{self, HandlerState};
use axum::{
    Router,
    routing::{delete, get, patch, put},
};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

/// REST API server.
pub struct Server {
    addr: SocketAddr,
    state: HandlerState,
}

impl Server {
    /// Create a new API server.
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            state: HandlerState::new(),
        }
    }

    /// Create a new API server with custom state.
    pub fn with_state(addr: SocketAddr, state: HandlerState) -> Self {
        Self { addr, state }
    }

    /// Build the router with all routes.
    pub fn router(&self) -> Router {
        let state = self.state.clone();

        Router::new()
            // Health / Status endpoints
            .route("/healthz", get(handlers::get_healthz))
            // Configuration endpoints
            .route("/config", get(handlers::get_config))
            .route("/config", patch(handlers::patch_config))
            // Cluster endpoints
            .route("/cluster/nodes", get(handlers::get_cluster_nodes))
            // Endpoint endpoints - collection
            .route("/endpoint", get(handlers::list_endpoints))
            // Endpoint endpoints - single
            .route("/endpoint/:id", get(handlers::get_endpoint))
            .route("/endpoint/:id", put(handlers::create_endpoint))
            .route("/endpoint/:id", delete(handlers::delete_endpoint))
            // Endpoint configuration
            .route("/endpoint/:id/config", get(handlers::get_endpoint_config))
            .route(
                "/endpoint/:id/config",
                patch(handlers::patch_endpoint_config),
            )
            // Endpoint labels
            .route("/endpoint/:id/labels", get(handlers::get_endpoint_labels))
            .route(
                "/endpoint/:id/labels",
                patch(handlers::patch_endpoint_labels),
            )
            .layer(TraceLayer::new_for_http())
            .layer(CorsLayer::permissive())
            .with_state(state)
    }

    /// Start the server.
    pub async fn start(&self) -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind(self.addr).await?;

        info!("API server listening on {}", self.addr);

        axum::serve(listener, self.router())
            .await
            .map_err(std::io::Error::other)
    }

    /// Get the server address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get a reference to the handler state.
    pub fn state(&self) -> &HandlerState {
        &self.state
    }
}

// ============================================================================
// OpenAPI / Swagger spec generation
// ============================================================================

/// OpenAPI spec information.
pub struct OpenApiInfo {
    /// API title
    pub title: String,
    /// API version
    pub version: String,
    /// API description
    pub description: String,
}

impl Default for OpenApiInfo {
    fn default() -> Self {
        Self {
            title: "Cilium Agent API".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "REST API for Cilium agent control and management".to_string(),
        }
    }
}

impl OpenApiInfo {
    /// Generate a minimal OpenAPI 3.0 spec.
    pub fn to_spec(&self) -> serde_json::Value {
        serde_json::json!({
            "openapi": "3.0.0",
            "info": {
                "title": self.title,
                "version": self.version,
                "description": self.description
            },
            "servers": [
                {
                    "url": "http://localhost:8080",
                    "description": "Local development server"
                }
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn server_creates_with_default_addr() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let server = Server::new(addr);
        assert_eq!(server.addr(), addr);
    }

    #[test]
    fn openapi_info_default_has_values() {
        let info = OpenApiInfo::default();
        assert!(!info.title.is_empty());
        assert!(!info.version.is_empty());
        assert!(!info.description.is_empty());
    }

    #[test]
    fn openapi_spec_generates_valid_json() {
        let info = OpenApiInfo::default();
        let spec = info.to_spec();

        assert_eq!(spec["openapi"], "3.0.0");
        assert!(spec["info"]["title"].is_string());
    }

    #[test]
    fn server_router_builds() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        let server = Server::new(addr);
        let _ = server.router();
    }
}
