//! HTTP request handlers for REST API endpoints.

use crate::errors::{ApiError, ApiResult};
use crate::types::{
    ClusterMeshStatus, ClusterNodeStatus, ClusterNodes, ConfigurationSpec, DaemonConfiguration,
    Endpoint, EndpointChangeRequest, EndpointConfiguration, EndpointConfigurationSpec,
    EndpointConfigurationStatus, EndpointList, IpamStatus, K8sStatus, LabelConfigurationSpec,
    State as HealthState, StatusResponse,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
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
    /// Current daemon configuration.
    pub config: Arc<RwLock<DaemonConfiguration>>,
    /// Registered endpoints.
    pub endpoints: Arc<RwLock<HashMap<u16, Endpoint>>>,
    /// Cluster nodes.
    pub nodes: Arc<RwLock<Vec<ClusterNodeStatus>>>,
}

impl HandlerState {
    /// Create a new handler state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(DaemonConfiguration::default())),
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            nodes: Arc::new(RwLock::new(vec![])),
        }
    }

    /// Create a handler state with initial configuration.
    #[must_use]
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

/// Handler for `GET /healthz`.
pub async fn get_healthz(State(state): State<HandlerState>) -> ApiResult<Json<StatusResponse>> {
    let config = state.config.read().await;

    let daemon_state = if config.ebpf_enabled {
        HealthState::Ok
    } else {
        HealthState::Warning
    };
    let daemon_message = if config.ebpf_enabled {
        "daemon is running"
    } else {
        "daemon is running with eBPF disabled"
    };

    let kubernetes = if config.kubernetes_enabled {
        K8sStatus {
            state: HealthState::Ok,
            msg: None,
            k8s_api_versions: vec![],
        }
    } else {
        K8sStatus {
            state: HealthState::Disabled,
            msg: Some("Kubernetes integration disabled".to_string()),
            k8s_api_versions: vec![],
        }
    };

    let response = StatusResponse::new(daemon_state, daemon_message)
        .with_kubernetes(kubernetes)
        .with_cluster_mesh(ClusterMeshStatus {
            num_global_services: 0,
            state: Some(HealthState::Disabled),
        })
        .with_ipam(IpamStatus::default());

    info!("healthz check completed");

    Ok(Json(response))
}

// ============================================================================
// Configuration handlers
// ============================================================================

/// Handler for `GET /config`.
pub async fn get_config(State(state): State<HandlerState>) -> ApiResult<Json<DaemonConfiguration>> {
    let config = state.config.read().await;
    debug!("returning daemon configuration");
    Ok(Json(config.clone()))
}

/// Handler for `PATCH /config`.
pub async fn patch_config(
    State(state): State<HandlerState>,
    Json(spec): Json<ConfigurationSpec>,
) -> ApiResult<(StatusCode, Json<DaemonConfiguration>)> {
    let mut config = state.config.write().await;

    spec.apply_to(&mut config);
    config.validate().map_err(ApiError::BadRequest)?;

    debug!("daemon configuration updated");
    info!(
        "configuration patch applied: cluster={}, node={}",
        config.cluster_name, config.node_name
    );

    Ok((StatusCode::OK, Json(config.clone())))
}

// ============================================================================
// Cluster handlers
// ============================================================================

/// Query parameters for listing endpoints.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    labels: Option<String>,
}

/// Handler for `GET /cluster/nodes`.
pub async fn get_cluster_nodes(State(state): State<HandlerState>) -> ApiResult<Json<ClusterNodes>> {
    let nodes = state.nodes.read().await;
    debug!("returning cluster node information");
    Ok(Json(ClusterNodes {
        nodes: nodes.clone(),
    }))
}

// ============================================================================
// Endpoint handlers
// ============================================================================

/// Handler for `GET /endpoint`.
pub async fn list_endpoints(
    State(state): State<HandlerState>,
    Query(_params): Query<ListQuery>,
) -> ApiResult<Json<EndpointList>> {
    let endpoints = state.endpoints.read().await;
    let list: EndpointList = endpoints.values().cloned().collect();
    debug!("returning {} endpoints", list.len());
    Ok(Json(list))
}

/// Handler for `GET /endpoint/{id}`.
pub async fn get_endpoint(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<Json<Endpoint>> {
    let endpoints = state.endpoints.read().await;

    endpoints
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("endpoint {id} not found")))
}

/// Handler for `PUT /endpoint/{id}`.
pub async fn create_endpoint(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
    Json(request): Json<EndpointChangeRequest>,
) -> ApiResult<(StatusCode, Json<Endpoint>)> {
    let mut endpoints = state.endpoints.write().await;

    if endpoints.contains_key(&id) {
        return Err(ApiError::Conflict(format!("endpoint {id} already exists")));
    }

    let mut endpoint = Endpoint::new(i64::from(id));

    if let Some(state) = request.state {
        endpoint = endpoint.with_state(state);
    }
    if let Some(addressing) = request.addressing {
        endpoint = endpoint.with_addressing(addressing);
    }
    endpoint.labels = request.labels;

    endpoints.insert(id, endpoint.clone());

    info!("endpoint {} created", id);

    Ok((StatusCode::CREATED, Json(endpoint)))
}

/// Handler for `DELETE /endpoint/{id}`.
pub async fn delete_endpoint(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<StatusCode> {
    let mut endpoints = state.endpoints.write().await;

    if endpoints.remove(&id).is_none() {
        return Err(ApiError::NotFound(format!("endpoint {id} not found")));
    }

    info!("endpoint {} deleted", id);

    Ok(StatusCode::OK)
}

/// Handler for `GET /endpoint/{id}/config`.
pub async fn get_endpoint_config(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<Json<EndpointConfigurationStatus>> {
    let endpoints = state.endpoints.read().await;

    if !endpoints.contains_key(&id) {
        return Err(ApiError::NotFound(format!("endpoint {id} not found")));
    }

    let config = EndpointConfiguration::default();
    debug!("returning configuration for endpoint {}", id);

    Ok(Json(EndpointConfigurationStatus {
        configuration: config,
    }))
}

/// Handler for `PATCH /endpoint/{id}/config`.
pub async fn patch_endpoint_config(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
    Json(spec): Json<EndpointConfigurationSpec>,
) -> ApiResult<(StatusCode, Json<EndpointConfigurationStatus>)> {
    let endpoints = state.endpoints.read().await;

    if !endpoints.contains_key(&id) {
        return Err(ApiError::NotFound(format!("endpoint {id} not found")));
    }

    let mut config = EndpointConfiguration::default();
    for (key, value) in spec.options {
        config.options.insert(key, value);
    }

    info!("endpoint {} configuration updated", id);

    Ok((
        StatusCode::OK,
        Json(EndpointConfigurationStatus {
            configuration: config,
        }),
    ))
}

/// Handler for `GET /endpoint/{id}/labels`.
pub async fn get_endpoint_labels(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
) -> ApiResult<Json<LabelConfigurationSpec>> {
    let endpoints = state.endpoints.read().await;

    let endpoint = endpoints
        .get(&id)
        .ok_or_else(|| ApiError::NotFound(format!("endpoint {id} not found")))?;

    debug!("returning labels for endpoint {}", id);

    Ok(Json(LabelConfigurationSpec {
        user: endpoint.labels.clone(),
    }))
}

/// Handler for `PATCH /endpoint/{id}/labels`.
pub async fn patch_endpoint_labels(
    State(state): State<HandlerState>,
    Path(id): Path<u16>,
    Json(labels): Json<LabelConfigurationSpec>,
) -> ApiResult<(StatusCode, Json<LabelConfigurationSpec>)> {
    let mut endpoints = state.endpoints.write().await;

    let endpoint = endpoints
        .get_mut(&id)
        .ok_or_else(|| ApiError::NotFound(format!("endpoint {id} not found")))?;

    endpoint.labels.clone_from(&labels.user);

    info!("endpoint {} labels updated", id);

    Ok((StatusCode::OK, Json(labels)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EndpointAddressing, EndpointState};

    #[test]
    fn handler_state_creates_default() {
        let state = HandlerState::new();
        assert!(state.endpoints.blocking_read().is_empty());
    }

    #[tokio::test]
    async fn get_healthz_succeeds() {
        let state = HandlerState::new();
        let result = get_healthz(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        assert_eq!(
            response.cilium.as_ref().map(|status| status.state),
            Some(HealthState::Ok)
        );
        assert_eq!(
            response.kubernetes.as_ref().map(|status| status.state),
            Some(HealthState::Ok)
        );
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
            state: Some(EndpointState::Ready),
            addressing: Some(EndpointAddressing {
                ipv4: Some("10.0.0.1".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = create_endpoint(State(state.clone()), Path(1u16), Json(request)).await;

        assert!(result.is_ok());
        let (status, endpoint) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(endpoint.0.id, 1);
        assert_eq!(endpoint.0.state, Some(EndpointState::Ready));
        assert_eq!(
            endpoint
                .0
                .addressing
                .as_ref()
                .and_then(|addressing| addressing.ipv4.as_deref()),
            Some("10.0.0.1")
        );
    }

    #[tokio::test]
    async fn create_endpoint_fails_if_exists() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            state: Some(EndpointState::Ready),
            ..Default::default()
        };

        let _ = create_endpoint(State(state.clone()), Path(1u16), Json(request.clone())).await;
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
            state: Some(EndpointState::Ready),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request))
            .await
            .ok();

        let result = get_endpoint(State(state), Path(1u16)).await;

        assert!(result.is_ok());
        let endpoint = result.unwrap().0;
        assert_eq!(endpoint.id, 1);
        assert_eq!(endpoint.state, Some(EndpointState::Ready));
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
            state: Some(EndpointState::Ready),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request))
            .await
            .ok();

        let result = delete_endpoint(State(state.clone()), Path(1u16)).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), StatusCode::OK);

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
            state: Some(EndpointState::Ready),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request))
            .await
            .ok();

        let result = get_endpoint_config(State(state), Path(1u16)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_endpoint_labels_succeeds() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            state: Some(EndpointState::Ready),
            labels: vec!["app=backend".to_string()],
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request))
            .await
            .ok();

        let result = get_endpoint_labels(State(state), Path(1u16)).await;

        assert!(result.is_ok());
        let labels = result.unwrap().0;
        assert_eq!(labels.user.first().map(String::as_str), Some("app=backend"));
    }

    #[tokio::test]
    async fn patch_endpoint_labels_succeeds() {
        let state = HandlerState::new();
        let request = EndpointChangeRequest {
            state: Some(EndpointState::Ready),
            ..Default::default()
        };

        create_endpoint(State(state.clone()), Path(1u16), Json(request))
            .await
            .ok();

        let new_labels = LabelConfigurationSpec {
            user: vec!["env=prod".to_string(), "tier=backend".to_string()],
        };

        let result =
            patch_endpoint_labels(State(state.clone()), Path(1u16), Json(new_labels)).await;

        assert!(result.is_ok());

        let result = get_endpoint_labels(State(state), Path(1u16)).await;
        let labels = result.unwrap().0;
        assert_eq!(labels.user.len(), 2);
    }
}
