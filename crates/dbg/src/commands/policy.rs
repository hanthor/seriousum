//! Policy inspection and manipulation commands
//!
//! Provides commands for:
//! - Listing policy maps
//! - Getting policy entries
//! - Adding/deleting policy rules
//! - Flushing policy maps

use crate::{NumericIdentity, PolicyEntry, Result, TrafficDirection};

/// List all policy maps in the system
pub fn list_all_policy_maps() -> Result<Vec<(u16, String)>> {
    // In a real implementation, this would scan /sys/kernel/debug/tracing/events/
    // to find cilium_policy_* maps
    Ok(vec![
        (
            1,
            "/sys/kernel/debug/tracing/events/cilium_policy_0001".to_string(),
        ),
        (
            2,
            "/sys/kernel/debug/tracing/events/cilium_policy_0002".to_string(),
        ),
        (
            42,
            "/sys/kernel/debug/tracing/events/cilium_policy_002a".to_string(),
        ),
    ])
}

/// Get policy entries for an endpoint
pub fn get_endpoint_policies(_endpoint_id: u16) -> Result<Vec<PolicyEntry>> {
    // In a real implementation, this would open the BPF map for this endpoint
    // and read all entries
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
            traffic_direction: TrafficDirection::Ingress,
            identity: NumericIdentity(256),
            port: 443,
            protocol: "tcp".to_string(),
            proxy_port: 8443,
            bytes: 5000,
            packets: 200,
            is_deny: false,
        },
        PolicyEntry {
            policy_id: 3,
            traffic_direction: TrafficDirection::Egress,
            identity: NumericIdentity::WORLD,
            port: 0,
            protocol: "any".to_string(),
            proxy_port: 0,
            bytes: 0,
            packets: 0,
            is_deny: true,
        },
    ])
}

/// Get allow/deny policy rules for an endpoint
pub fn get_policy_decisions(endpoint_id: u16) -> Result<Vec<(String, bool)>> {
    let policies = get_endpoint_policies(endpoint_id)?;
    let decisions = policies
        .into_iter()
        .map(|p| {
            let rule_desc = format!(
                "{} {} {}:{}",
                p.traffic_direction, p.identity, p.port, p.protocol
            );
            (rule_desc, !p.is_deny)
        })
        .collect();
    Ok(decisions)
}

/// Add a policy rule to an endpoint
pub fn add_policy_rule(
    _endpoint_id: u16,
    _direction: TrafficDirection,
    _identity: NumericIdentity,
    _port: u16,
    _protocol: &str,
    _allow: bool,
) -> Result<()> {
    crate::require_root("bpf policy add")?;
    // In a real implementation, this would open the BPF map and insert
    Ok(())
}

/// Remove a policy rule from an endpoint
pub fn remove_policy_rule(
    _endpoint_id: u16,
    _direction: TrafficDirection,
    _identity: NumericIdentity,
) -> Result<()> {
    crate::require_root("bpf policy delete")?;
    // In a real implementation, this would open the BPF map and delete
    Ok(())
}

/// Dump all policy maps for inspection
pub fn dump_all_policies() -> Result<Vec<(u16, Vec<PolicyEntry>)>> {
    let maps = list_all_policy_maps()?;
    let mut result = Vec::new();

    for (endpoint_id, _path) in maps {
        match get_endpoint_policies(endpoint_id) {
            Ok(policies) => result.push((endpoint_id, policies)),
            Err(_) => continue, // Skip if unable to read
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_all_policy_maps() {
        let maps = list_all_policy_maps().unwrap();
        assert!(!maps.is_empty());
    }

    #[test]
    fn test_get_endpoint_policies() {
        let policies = get_endpoint_policies(42).unwrap();
        assert!(!policies.is_empty());
    }

    #[test]
    fn test_get_endpoint_policies_includes_allow_and_deny() {
        let policies = get_endpoint_policies(42).unwrap();
        let has_allow = policies.iter().any(|p| !p.is_deny);
        let has_deny = policies.iter().any(|p| p.is_deny);
        assert!(has_allow);
        assert!(has_deny);
    }

    #[test]
    fn test_get_policy_decisions() {
        let decisions = get_policy_decisions(42).unwrap();
        assert!(!decisions.is_empty());
    }

    #[test]
    fn test_add_policy_rule_requires_root() {
        if !crate::is_root() {
            let result = add_policy_rule(
                42,
                TrafficDirection::Ingress,
                NumericIdentity::WORLD,
                80,
                "tcp",
                true,
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_remove_policy_rule_requires_root() {
        if !crate::is_root() {
            let result = remove_policy_rule(42, TrafficDirection::Ingress, NumericIdentity::WORLD);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_dump_all_policies() {
        let all_policies = dump_all_policies().unwrap();
        assert!(!all_policies.is_empty());
    }
}
