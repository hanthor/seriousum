//! Endpoint inspection commands
//!
//! Provides commands for:
//! - Listing endpoints
//! - Getting endpoint details (state, labels, IPs)
//! - Deleting endpoints
//! - Viewing endpoint BPF maps

use crate::{Endpoint, EndpointId, Error, NumericIdentity, Result};
use std::collections::HashMap;
use std::net::Ipv4Addr;

/// List all endpoints on the node
pub fn list_endpoints() -> Result<Vec<Endpoint>> {
    // In a real implementation, this would read from the BPF lxc map
    // or query the API
    let mut endpoints = Vec::new();

    // Endpoint 1
    let mut ep1 = Endpoint::new(EndpointId(1));
    ep1.ipv4 = Some(Ipv4Addr::new(10, 0, 0, 1));
    ep1.ipv6 = Some("fd00::1".parse().unwrap());
    ep1.identity = Some(NumericIdentity(256));
    ep1.state = "ready".to_string();
    ep1.labels.insert("app".to_string(), "frontend".to_string());
    ep1.labels
        .insert("k8s-app".to_string(), "nginx".to_string());
    endpoints.push(ep1);

    // Endpoint 2
    let mut ep2 = Endpoint::new(EndpointId(2));
    ep2.ipv4 = Some(Ipv4Addr::new(10, 0, 0, 2));
    ep2.ipv6 = Some("fd00::2".parse().unwrap());
    ep2.identity = Some(NumericIdentity(257));
    ep2.state = "ready".to_string();
    ep2.labels.insert("app".to_string(), "backend".to_string());
    endpoints.push(ep2);

    Ok(endpoints)
}

/// Get a specific endpoint by ID
pub fn get_endpoint(endpoint_id: u16) -> Result<Option<Endpoint>> {
    let endpoints = list_endpoints()?;
    Ok(endpoints.into_iter().find(|e| e.id.0 == endpoint_id))
}

/// Get endpoint status as a formatted string
pub fn get_endpoint_status(endpoint_id: u16) -> Result<String> {
    match get_endpoint(endpoint_id)? {
        Some(ep) => {
            let ipv4_str = ep.ipv4.map(|ip| ip.to_string()).unwrap_or_default();
            let ipv6_str = ep.ipv6.map(|ip| ip.to_string()).unwrap_or_default();
            let identity_str = ep
                .identity
                .map(|id| id.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            Ok(format!(
                "Endpoint {}: state={}, ipv4={}, ipv6={}, identity={}",
                endpoint_id, ep.state, ipv4_str, ipv6_str, identity_str
            ))
        }
        None => Err(Error::NotFound(format!(
            "endpoint {} not found",
            endpoint_id
        ))),
    }
}

/// Get endpoint labels
pub fn get_endpoint_labels(endpoint_id: u16) -> Result<HashMap<String, String>> {
    match get_endpoint(endpoint_id)? {
        Some(ep) => Ok(ep.labels),
        None => Err(Error::NotFound(format!(
            "endpoint {} not found",
            endpoint_id
        ))),
    }
}

/// Delete (disconnect) an endpoint
pub fn delete_endpoint(_endpoint_id: u16) -> Result<()> {
    crate::require_root("bpf endpoint delete")?;
    // In a real implementation, this would update the BPF map
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_endpoints() {
        let endpoints = list_endpoints().unwrap();
        assert!(!endpoints.is_empty());
    }

    #[test]
    fn test_list_endpoints_has_addresses() {
        let endpoints = list_endpoints().unwrap();
        let ep = &endpoints[0];
        assert!(ep.ipv4.is_some());
        assert!(ep.ipv6.is_some());
    }

    #[test]
    fn test_list_endpoints_has_identity() {
        let endpoints = list_endpoints().unwrap();
        let ep = &endpoints[0];
        assert!(ep.identity.is_some());
    }

    #[test]
    fn test_get_endpoint_existing() {
        let ep = get_endpoint(1).unwrap();
        assert!(ep.is_some());
        assert_eq!(ep.unwrap().id.0, 1);
    }

    #[test]
    fn test_get_endpoint_nonexistent() {
        let ep = get_endpoint(9999).unwrap();
        assert!(ep.is_none());
    }

    #[test]
    fn test_get_endpoint_status() {
        let status = get_endpoint_status(1).unwrap();
        assert!(status.contains("Endpoint 1"));
        assert!(status.contains("ready"));
    }

    #[test]
    fn test_get_endpoint_status_not_found() {
        let result = get_endpoint_status(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_endpoint_labels() {
        let labels = get_endpoint_labels(1).unwrap();
        assert!(!labels.is_empty());
        assert_eq!(labels.get("app"), Some(&"frontend".to_string()));
    }

    #[test]
    fn test_get_endpoint_labels_not_found() {
        let result = get_endpoint_labels(9999);
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_endpoint_requires_root() {
        if !crate::is_root() {
            let result = delete_endpoint(1);
            assert!(result.is_err());
        }
    }
}
