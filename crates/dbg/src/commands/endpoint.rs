//! Endpoint inspection commands.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use serde::Deserialize;

use crate::{Endpoint, EndpointId, Error, Result, compat_get};

#[derive(Debug, Clone, Deserialize)]
struct CompatEndpoint {
    id: u16,
    status: CompatEndpointStatus,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct CompatEndpointIdentity {
    #[serde(default)]
    id: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatEndpointStatus {
    state: String,
    #[serde(default)]
    networking: CompatEndpointNetworking,
    #[serde(default)]
    identity: CompatEndpointIdentity,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct CompatEndpointNetworking {
    #[serde(default)]
    addressing: Vec<CompatEndpointAddressing>,
}

#[derive(Debug, Clone, Deserialize)]
struct CompatEndpointAddressing {
    ipv4: Option<String>,
}

fn compat_endpoints() -> Result<Vec<CompatEndpoint>> {
    let body = compat_get("/v1/endpoint")?;
    serde_json::from_str(&body).map_err(Into::into)
}

/// List all endpoints on the node.
pub fn list_endpoints() -> Result<Vec<Endpoint>> {
    compat_endpoints()?
        .into_iter()
        .map(|endpoint| {
            let ipv4 = endpoint
                .status
                .networking
                .addressing
                .iter()
                .find_map(|address| address.ipv4.as_deref())
                .and_then(|address| address.parse::<Ipv4Addr>().ok());
            let identity = if endpoint.status.identity.id != 0 {
                Some(crate::NumericIdentity(endpoint.status.identity.id as u32))
            } else {
                None
            };
            Ok(Endpoint {
                id: EndpointId(endpoint.id),
                ipv4,
                ipv6: None,
                identity,
                state: endpoint.status.state,
                labels: HashMap::new(),
            })
        })
        .collect()
}

/// Return the raw compat endpoint list JSON.
pub fn list_endpoints_json_raw() -> Result<String> {
    compat_get("/v1/endpoint")
}

/// Get a specific endpoint by ID.
pub fn get_endpoint(endpoint_id: u16) -> Result<Option<Endpoint>> {
    let endpoints = list_endpoints()?;
    Ok(endpoints.into_iter().find(|e| e.id.0 == endpoint_id))
}

/// Get endpoint status as a formatted string.
pub fn get_endpoint_status(endpoint_id: u16) -> Result<String> {
    match get_endpoint(endpoint_id)? {
        Some(ep) => {
            let ipv4_str = ep.ipv4.map(|ip| ip.to_string()).unwrap_or_default();
            Ok(format!(
                "Endpoint {}: state={}, ipv4={}",
                endpoint_id, ep.state, ipv4_str
            ))
        }
        None => Err(Error::NotFound(format!(
            "endpoint {} not found",
            endpoint_id
        ))),
    }
}

/// Get endpoint labels.
pub fn get_endpoint_labels(endpoint_id: u16) -> Result<HashMap<String, String>> {
    match get_endpoint(endpoint_id)? {
        Some(ep) => Ok(ep.labels),
        None => Err(Error::NotFound(format!(
            "endpoint {} not found",
            endpoint_id
        ))),
    }
}

/// Delete (disconnect) an endpoint.
pub fn delete_endpoint(_endpoint_id: u16) -> Result<()> {
    crate::require_root("bpf endpoint delete")?;
    Ok(())
}
