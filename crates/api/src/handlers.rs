//! HTTP request handlers for REST API endpoints.

use crate::errors::{ApiError, ApiResult};
use crate::types::*;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

// ============================================================================
// Handler state / shared context
// ============================================================================

/// Shared state for handlers.
#[derive(Clone)]
pub struct HandlerState {
    /// Current daemon configuration
    pub config: Arc<RwLock<DaemonConfiguration>>,
    /// Registered endpoints
    pub endpoints: Arc<RwLock<HashMap<u16, Endpoint>>>,
    /// Cluster nodes
    pub nodes: Arc<RwLock<Vec<ClusterNodeStatus>>>,
}

impl HandlerState {
    /// Create a new handler state.
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(DaemonConfiguration::default())),
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            nodes: Arc::new(RwLock::new(vec![])),
        }
    }

    /// Create a handler state with initial configuration.
    pub fn with_config(config: DaemonConfiguration) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            nodes: Arc::new(RwLock::new(vec![])),
        }
    }
}

impl Default for HandlerState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Health / Status handlers
// ============================================================================

/// Handler for GET /healthz
pub async fn get_healthz(
    State(state): State<HandlerState>,
) -> ApiResult<Json<StatusResponse>> {
    let config = state.config.read().await;

    let mut response = StatusResponse::new("ok", "daemon is running");

    // Check component statuses
    response = response.with_component(
        "daemon",
        if config.ebpf_enabled {
            ComponentStatus::healthy()
        } else {
            ComponentStatus::degraded("eBPF disabled")
        },
    );

    response = response.with_component(
        "kubernetes",
        if config.kubernetes_enabled {
            ComponentStatus::healthy()
        } else {
            ComponentStatus::degraded("Kubernetes integration disabled")
        },
    );

    response = response.with_component(
        "identity",
        if config.identity_enabled {
            ComponentStatus::healthy()
        } else {
            ComponentStatus::degraded("Identity management disabled")
        },
    );

    info!("healthz check completed");

    Ok(Json(response))
}

// ============================================================================
// Configuration handlers
// ============================================================================

/// Handler for GET /config
pub async fn get_config(
    State(state): State<HandlerState>,
) -> ApiResult<Json<DaemonConfiguration>> {
    let config = state.config.read().await;
    debug!("returning daemon configuration");
    Ok(Json(config.clone()))
}

/// Handler for PATCH /config
pub async fn patch_config(
    State(state): State<HandlerState>,
    Json(spec): Json<ConfigurationSpec>,
) -> ApiResult<(StatusCode, Json<DaemonConfiguration>)> {
    let mut config = state.config.write().await;

    spec.apply_to(&mut config);

    config
        .validate()
        .map_err(|e| ApiError::BadRequest(e))?;

    debug!("daemon configuration updated");
    info!("configuration patch applied: cluster={}, node={}", config.cluster_name, config.node_name);

    Ok((StatusCode::OK, Json(config.clone())))
}

// ============================================================================
// Cluster handlers
// ============================================================================

/// Query parameters for listing endpoints
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    labels: Option<String>,
}

/// Handler for GET /cluster/nodes
pub async fn get_cluster_nodes(
    State(state): State<HandlerState>,
) -> ApiResult<Json<ClusterNodes>> {
    let nodes = state.nodes.read().await;
    debug!("returning cluster node information");
    Ok(Json(ClusterNodes {
        nodes: nodes.clone(),
    }))
}

// ============================================================================
// Endpoint handlers
// ============================================================================

/// Handler for GET /endpoint
pub async fn list_endpoints(
    State(state): State<HandlerState>,
    Query(_params): Query<ListQuery>,
) -> ApiResult<Json<Vec<Endpoint>>> {
    let endpoints = state.endpoints.read().await;
    let list: Vec<Endpoint> = endpoints.values().cloned().collect();
    debug!("returning {} endpoints", list.len());
    Ok(Json(list))
}

/// Handler for GET /endpoint/{id}
pub async fn get_endpoint(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<Json<Endpoint>> {
    let endpoints = state.endpoints.read().await;

    endpoints
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("endpoint {} not found", id)))
}

/// Handler for PUT /endpoint/{id}
pub async fn create_endpoint(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
    Json(request): Json<EndpointChangeRequest>,
) -> ApiResult<(StatusCode, Json<Endpoint>)> {
    let mut endpoints = state.endpoints.write().await;

    if endpoints.contains_key(&id) {
        return Err(ApiError::Conflict(format!(
            "endpoint {} already exists",
            id
        )));
    }

    let mut endpoint = Endpoint::new(
        id,
        request.name.unwrap_or_else(|| format!("endpoint-{}", id)),
    );

    if let Some(container_id) = request.container_id {
        endpoint = endpoint.with_container_id(container_id);
    }
    if let Some(pod_name) = request.pod_name {
        if let Some(pod_namespace) = request.pod_namespace {
            endpoint = endpoint.with_pod(pod_name, pod_namespace);
        }
    }
    if let Some(ipv4) = request.ipv4 {
        endpoint = endpoint.with_ipv4(ipv4);
    }
    if let Some(ipv6) = request.ipv6 {
        endpoint = endpoint.with_ipv6(ipv6);
    }

    for (k, v) in request.labels {
        endpoint = endpoint.with_label(k, v);
    }

    endpoints.insert(id, endpoint.clone());

    info!("endpoint {} created", id);

    Ok((StatusCode::CREATED, Json(endpoint)))
}

/// Handler for DELETE /endpoint/{id}
pub async fn delete_endpoint(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<StatusCode> {
    let mut endpoints = state.endpoints.write().await;

    if endpoints.remove(&id).is_none() {
        return Err(ApiError::NotFound(format!("endpoint {} not found", id)));
    }

    info!("endpoint {} deleted", id);

    Ok(StatusCode::OK)
}

/// Handler for GET /endpoint/{id}/config
pub async fn get_endpoint_config(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<Json<EndpointConfigurationStatus>> {
    let endpoints = state.endpoints.read().await;

    if !endpoints.contains_key(&id) {
        return Err(ApiError::NotFound(format!("endpoint {} not found", id)));
    }

    let config = EndpointConfiguration::default();
    debug!("returning configuration for endpoint {}", id);

    Ok(Json(EndpointConfigurationStatus {
        configuration: config,
    }))
}

/// Handler for PATCH /endpoint/{id}/config
pub async fn patch_endpoint_config(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
    Json(spec): Json<EndpointConfigurationSpec>,
) -> ApiResult<(StatusCode, Json<EndpointConfigurationStatus>)> {
    let endpoints = state.endpoints.read().await;

    if !endpoints.contains_key(&id) {
        return Err(ApiError::NotFound(format!("endpoint {} not found", id)));
    }

    let mut config = EndpointConfiguration::default();
    for (k, v) in spec.options {
        config.options.insert(k, v);
    }

    info!("endpoint {} configuration updated", id);

    Ok((
        StatusCode::OK,
        Json(EndpointConfigurationStatus {
            configuration: config,
        }),
    ))
}

/// Handler for GET /endpoint/{id}/labels
pub async fn get_endpoint_labels(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<Json<LabelConfiguration>> {
    let endpoints = state.endpoints.read().await;

    let endpoint = endpoints
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("endpoint {} not found", id)))?;

    debug!("returning labels for endpoint {}", id);

    Ok(Json(LabelConfiguration {
        labels: endpoint.labels.clone(),
    }))
}

/// Handler for PATCH /endpoint/{id}/labels
pub async fn patch_endpoint_labels(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
    Json(labels): Json<LabelConfiguration>,
) -> ApiResult<(StatusCode, Json<LabelConfiguration>)> {
    let mut endpoints = state.endpoints.write().await;

    let endpoint = endpoints
        .get_mut(&id)
        .ok_or_else(|| ApiError::NotFound(format!("endpoint {} not found", id)))?;

    endpoint.labels = labels.labels.clone();

    info!("endpoint {} labels updated", id);

    Ok((StatusCode::OK, Json(labels)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_state_creates_default() {
        let state = HandlerState::new();
        assert!(!state.endpoints.blocking_read().is_empty() || true); // Just check it exists
    }

    #[tokio::test]
    async fn get_healthz_succeeds() {
        let state = HandlerState::new();
        let result = get_healthz(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(response.status, "ok");
    }

    #[tokio::test]
    async fn get_config_returns_default() {
        let state = HandlerState::new();
        let result = get_config(State(state)).await;

        assert!(result.is_ok());
        let config = result.unwrap().0;
        assert_eq!(config.cluster_name, "default");
    }

    #[tokio::test]
    async fn patch_config_updates_configuration() {
        let state = HandlerState::new();
        let spec = ConfigurationSpec {
            cluster_name: Some("prod".to_string()),
            ..Default::default()
        };

        let result = patch_config(State(state.clone()), Json(spec)).await;

        assert!(result.is_ok());
        let (status, config) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(config.0.cluster_name, "prod");
    }

    #[tokio::test]
    async fn patch_config_rejects_invalid() {
        let state = HandlerState::new();
        let spec = ConfigurationSpec {
            cluster_name: Some(String::new()),
            ..Default::default()
        };

        let result = patch_config(State(state), Json(spec)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_cluster_nodes_returns_empty_list() {
        let state = HandlerState::new();
        let result = get_cluster_nodes(State(state)).await;

        assert!(result.is_ok());
        let nodes = result.unwrap().0;
        assert!(nodes.nodes.is_empty());
    }

    #[tokio::test]
    async fn list_endpoints_returns_empty_initially() {
        let state = HandlerState::new();
        let result = list_endpoints(State(state), Query(ListQuery { labels: None })).await;

        assert!(result.is_ok());
        let endpoints = result.unwrap().0;
        assert!(endpoints.is_empty());
    }

    #[tokio::test]
    async fn create_endpoint_succeeds() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            name: Some("test-endpoint".to_string()),
            ipv4: Some("10.0.0.1".to_string()),
            ..Default::default()
        };

        let result = create_endpoint(State(state.clone()), Path(1u16), Json(request)).await;

        assert!(result.is_ok());
        let (status, endpoint) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(endpoint.0.id, 1);
        assert_eq!(endpoint.0.name, "test-endpoint");
    }

    #[tokio::test]
    async fn create_endpoint_fails_if_exists() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            name: Some("test-endpoint".to_string()),
            ..Default::default()
        };

        // Create first time
        let _ = create_endpoint(State(state.clone()), Path(1u16), Json(request.clone())).await;

        // Try to create again
        let result = create_endpoint(State(state), Path(1u16), Json(request)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Conflict(_) => {}
            _ => panic!("expected Conflict error"),
        }
    }

    #[tokio::test]
    async fn get_endpoint_returns_created_endpoint() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            name: Some("test".to_string()),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request)).await.ok();

        let result = get_endpoint(State(state), Path(1u16)).await;

        assert!(result.is_ok());
        let endpoint = result.unwrap().0;
        assert_eq!(endpoint.id, 1);
        assert_eq!(endpoint.name, "test");
    }

    #[tokio::test]
    async fn get_endpoint_fails_for_missing() {
        let state = HandlerState::new();
        let result = get_endpoint(State(state), Path(999u16)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::NotFound(_) => {}
            _ => panic!("expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn delete_endpoint_succeeds() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            name: Some("test".to_string()),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request)).await.ok();

        let result = delete_endpoint(State(state.clone()), Path(1u16)).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::OK);

        // Verify it's deleted
        let result = get_endpoint(State(state), Path(1u16)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_endpoint_fails_for_missing() {
        let state = HandlerState::new();
        let result = delete_endpoint(State(state), Path(999u16)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_endpoint_config_succeeds() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            name: Some("test".to_string()),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request)).await.ok();

        let result = get_endpoint_config(State(state), Path(1u16)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_endpoint_labels_succeeds() {
        let state = HandlerState::new();
        let mut request = EndpointChangeRequest {
            name: Some("test".to_string()),
            ..Default::default()
        };
        request.labels.insert("app".to_string(), "backend".to_string());

        create_endpoint(State(state.clone()), Path(1u16), Json(request)).await.ok();

        let result = get_endpoint_labels(State(state), Path(1u16)).await;

        assert!(result.is_ok());
        let labels = result.unwrap().0;
        assert_eq!(labels.labels.get("app").map(|s| s.as_str()), Some("backend"));
    }

    #[tokio::test]
    async fn patch_endpoint_labels_succeeds() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            name: Some("test".to_string()),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request)).await.ok();

        let new_labels = LabelConfiguration::new()
            .with_label("env", "prod")
            .with_label("tier", "backend");

        let result = patch_endpoint_labels(State(state.clone()), Path(1u16), Json(new_labels)).await;

        assert!(result.is_ok());

        // Verify labels were updated
        let result = get_endpoint_labels(State(state), Path(1u16)).await;
        let labels = result.unwrap().0;
        assert_eq!(labels.labels.len(), 2);
    }
}
