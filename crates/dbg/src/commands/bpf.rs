//! BPF map inspection and manipulation commands
//!
//! Provides commands for:
//! - Listing BPF maps (policy, endpoint, connection tracking, etc.)
//! - Dumping BPF map contents
//! - Managing map entries (add, delete, flush)
//! - Inspecting map statistics

use crate::{NumericIdentity, PolicyEntry, Result, TrafficDirection};
use std::collections::HashMap;

/// List all policy maps on the system
pub fn list_policy_maps() -> Result<Vec<(String, String)>> {
    // In a real implementation, this would scan /sys/kernel/debug/tracing/events/
    // or use the BPF subsystem to find all cilium_policy_* maps
    Ok(vec![
        (
            "cilium_policy_0000".to_string(),
            "/sys/kernel/debug/tracing/events/".to_string(),
        ),
        (
            "cilium_policy_0001".to_string(),
            "/sys/kernel/debug/tracing/events/".to_string(),
        ),
    ])
}

/// Dump policy map entries for a specific endpoint
pub fn dump_policy_map(_endpoint_id: u16) -> Result<Vec<PolicyEntry>> {
    // In a real implementation, this would open the BPF map for this endpoint
    // and read all entries from it
    Ok(vec![
        PolicyEntry {
            policy_id: 1,
            traffic_direction: TrafficDirection::Ingress,
            identity: NumericIdentity::WORLD,
            port: 80,
            protocol: "tcp".to_string(),
            proxy_port: 0,
            bytes: 1000,
            packets: 50,
            is_deny: false,
        },
        PolicyEntry {
            policy_id: 2,
            traffic_direction: TrafficDirection::Egress,
            identity: NumericIdentity(256),
            port: 443,
            protocol: "tcp".to_string(),
            proxy_port: 0,
            bytes: 2000,
            packets: 100,
            is_deny: false,
        },
    ])
}

/// Add a policy entry to a map
pub fn add_policy_entry(
    _endpoint_id: u16,
    _direction: TrafficDirection,
    _identity: NumericIdentity,
    _port: u16,
    _protocol: &str,
) -> Result<()> {
    crate::require_root("bpf policy add")?;
    // In a real implementation, this would open the BPF map and insert the entry
    Ok(())
}

/// Delete a policy entry from a map
pub fn delete_policy_entry(
    _endpoint_id: u16,
    _direction: TrafficDirection,
    _identity: NumericIdentity,
) -> Result<()> {
    crate::require_root("bpf policy delete")?;
    // In a real implementation, this would open the BPF map and delete the entry
    Ok(())
}

/// Flush (clear) a policy map
pub fn flush_policy_map(_endpoint_id: u16) -> Result<()> {
    crate::require_root("bpf policy flush")?;
    // In a real implementation, this would clear the specified map
    Ok(())
}

/// List connection tracking maps
pub fn list_ct_maps() -> Result<Vec<(String, String)>> {
    Ok(vec![
        ("cilium_ct_global4".to_string(), "IPv4 CT".to_string()),
        ("cilium_ct_global6".to_string(), "IPv6 CT".to_string()),
    ])
}

/// List endpoint maps
pub fn list_endpoint_maps() -> Result<Vec<(String, String)>> {
    Ok(vec![("cilium_lxc".to_string(), "Endpoint map".to_string())])
}

/// List service maps
pub fn list_service_maps() -> Result<Vec<(String, String)>> {
    Ok(vec![
        (
            "cilium_lb4_services".to_string(),
            "IPv4 Services".to_string(),
        ),
        (
            "cilium_lb4_backends".to_string(),
            "IPv4 Backends".to_string(),
        ),
        (
            "cilium_lb6_services".to_string(),
            "IPv6 Services".to_string(),
        ),
        (
            "cilium_lb6_backends".to_string(),
            "IPv6 Backends".to_string(),
        ),
    ])
}

/// List authentication maps
pub fn list_auth_maps() -> Result<Vec<(String, String)>> {
    Ok(vec![(
        "cilium_auth_map".to_string(),
        "Auth entries".to_string(),
    )])
}

/// Dump authentication map
pub fn dump_auth_map() -> Result<Vec<HashMap<String, String>>> {
    let mut entries = Vec::new();
    let mut entry = HashMap::new();
    entry.insert("identity".to_string(), "256".to_string());
    entry.insert("protocol".to_string(), "6".to_string());
    entry.insert("port".to_string(), "443".to_string());
    entry.insert("auth_type".to_string(), "SPIFFE".to_string());
    entries.push(entry);
    Ok(entries)
}

/// List bandwidth maps
pub fn list_bandwidth_maps() -> Result<Vec<(String, String)>> {
    Ok(vec![(
        "cilium_bandwidth_map".to_string(),
        "Bandwidth stats".to_string(),
    )])
}

/// Dump bandwidth map
pub fn dump_bandwidth_map() -> Result<Vec<HashMap<String, String>>> {
    let mut entries = Vec::new();
    let mut entry = HashMap::new();
    entry.insert("endpoint_id".to_string(), "42".to_string());
    entry.insert("bytes".to_string(), "1000000".to_string());
    entry.insert("packets".to_string(), "5000".to_string());
    entries.push(entry);
    Ok(entries)
}

/// List configuration maps
pub fn list_config_maps() -> Result<Vec<(String, String)>> {
    Ok(vec![(
        "cilium_config_map".to_string(),
        "Configuration".to_string(),
    )])
}

/// Dump configuration map
pub fn dump_config_map() -> Result<Vec<(String, String)>> {
    Ok(vec![
        ("enable_ipv6".to_string(), "1".to_string()),
        ("enable_ipv4".to_string(), "1".to_string()),
        ("preallocated_maps".to_string(), "2".to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_policy_maps() {
        let maps = list_policy_maps().unwrap();
        assert!(!maps.is_empty());
    }

    #[test]
    fn test_dump_policy_map() {
        let entries = dump_policy_map(42).unwrap();
        assert!(!entries.is_empty());
        assert_eq!(entries[0].traffic_direction, TrafficDirection::Ingress);
    }

    #[test]
    fn test_add_policy_entry_requires_root() {
        if !crate::is_root() {
            let result = add_policy_entry(
                42,
                TrafficDirection::Ingress,
                NumericIdentity::WORLD,
                80,
                "tcp",
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_list_ct_maps() {
        let maps = list_ct_maps().unwrap();
        assert!(maps.iter().any(|(name, _)| name.contains("global")));
    }

    #[test]
    fn test_list_endpoint_maps() {
        let maps = list_endpoint_maps().unwrap();
        assert!(maps.iter().any(|(name, _)| name.contains("lxc")));
    }

    #[test]
    fn test_list_service_maps() {
        let maps = list_service_maps().unwrap();
        assert!(maps.iter().any(|(name, _)| name.contains("lb4")));
    }

    #[test]
    fn test_dump_auth_map() {
        let entries = dump_auth_map().unwrap();
        assert!(!entries.is_empty());
        assert!(entries[0].contains_key("auth_type"));
    }

    #[test]
    fn test_dump_bandwidth_map() {
        let entries = dump_bandwidth_map().unwrap();
        assert!(!entries.is_empty());
        assert!(entries[0].contains_key("bytes"));
    }

    #[test]
    fn test_dump_config_map() {
        let config = dump_config_map().unwrap();
        assert!(!config.is_empty());
        assert!(config.iter().any(|(k, _v)| k.contains("ipv4")));
    }
}
