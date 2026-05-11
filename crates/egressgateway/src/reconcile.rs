// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Reconciliation logic for egress gateway policies

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;

/// BPF policy map entry for IPv4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BpfPolicyKeyV4 {
    /// Source IP (endpoint IP)
    pub source_ip: Ipv4Addr,
    /// Destination CIDR
    pub dest_cidr: ipnet::Ipv4Net,
}

impl BpfPolicyKeyV4 {
    /// Create new BPF policy key
    pub fn new(source_ip: Ipv4Addr, dest_cidr: ipnet::Ipv4Net) -> Self {
        Self {
            source_ip,
            dest_cidr,
        }
    }
}

/// BPF policy map value for IPv4
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BpfPolicyValueV4 {
    /// Egress IP
    pub egress_ip: Ipv4Addr,
    /// Gateway IP
    pub gateway_ip: Ipv4Addr,
}

impl BpfPolicyValueV4 {
    /// Create new BPF policy value
    pub fn new(egress_ip: Ipv4Addr, gateway_ip: Ipv4Addr) -> Self {
        Self { egress_ip, gateway_ip }
    }

    /// Check if value matches expected values
    pub fn matches(&self, egress_ip: Ipv4Addr, gateway_ip: Ipv4Addr) -> bool {
        self.egress_ip == egress_ip && self.gateway_ip == gateway_ip
    }
}

/// BPF policy map entry for IPv6
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BpfPolicyKeyV6 {
    /// Source IP (endpoint IP)
    pub source_ip: std::net::Ipv6Addr,
    /// Destination CIDR
    pub dest_cidr: ipnet::Ipv6Net,
}

impl BpfPolicyKeyV6 {
    /// Create new BPF policy key
    pub fn new(source_ip: std::net::Ipv6Addr, dest_cidr: ipnet::Ipv6Net) -> Self {
        Self {
            source_ip,
            dest_cidr,
        }
    }
}

/// BPF policy map value for IPv6
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BpfPolicyValueV6 {
    /// Egress IP
    pub egress_ip: std::net::Ipv6Addr,
    /// Gateway IP (IPv4 for backwards compat)
    pub gateway_ip: Ipv4Addr,
    /// Interface index
    pub ifindex: u32,
}

impl BpfPolicyValueV6 {
    /// Create new BPF policy value
    pub fn new(egress_ip: std::net::Ipv6Addr, gateway_ip: Ipv4Addr, ifindex: u32) -> Self {
        Self {
            egress_ip,
            gateway_ip,
            ifindex,
        }
    }

    /// Check if value matches expected values
    pub fn matches(
        &self,
        egress_ip: std::net::Ipv6Addr,
        gateway_ip: Ipv4Addr,
        ifindex: u32,
    ) -> bool {
        self.egress_ip == egress_ip && self.gateway_ip == gateway_ip && self.ifindex == ifindex
    }
}

/// Reconciler for syncing policy state with BPF maps
pub struct Reconciler {
    /// Pending IPv4 rules to add/update
    pub pending_ipv4_rules: HashMap<BpfPolicyKeyV4, BpfPolicyValueV4>,
    /// IPv4 rules to delete
    pub ipv4_rules_to_delete: HashSet<BpfPolicyKeyV4>,
    /// Pending IPv6 rules to add/update
    pub pending_ipv6_rules: HashMap<BpfPolicyKeyV6, BpfPolicyValueV6>,
    /// IPv6 rules to delete
    pub ipv6_rules_to_delete: HashSet<BpfPolicyKeyV6>,
}

impl Reconciler {
    /// Create a new reconciler
    pub fn new() -> Self {
        Self {
            pending_ipv4_rules: HashMap::new(),
            ipv4_rules_to_delete: HashSet::new(),
            pending_ipv6_rules: HashMap::new(),
            ipv6_rules_to_delete: HashSet::new(),
        }
    }

    /// Add IPv4 rule
    pub fn add_ipv4_rule(
        &mut self,
        source_ip: Ipv4Addr,
        dest_cidr: ipnet::Ipv4Net,
        egress_ip: Ipv4Addr,
        gateway_ip: Ipv4Addr,
    ) {
        let key = BpfPolicyKeyV4::new(source_ip, dest_cidr);
        let value = BpfPolicyValueV4::new(egress_ip, gateway_ip);
        self.pending_ipv4_rules.insert(key, value);
        self.ipv4_rules_to_delete.remove(&key);
    }

    /// Remove IPv4 rule
    pub fn remove_ipv4_rule(&mut self, source_ip: Ipv4Addr, dest_cidr: ipnet::Ipv4Net) {
        let key = BpfPolicyKeyV4::new(source_ip, dest_cidr);
        self.pending_ipv4_rules.remove(&key);
        self.ipv4_rules_to_delete.insert(key);
    }

    /// Add IPv6 rule
    pub fn add_ipv6_rule(
        &mut self,
        source_ip: std::net::Ipv6Addr,
        dest_cidr: ipnet::Ipv6Net,
        egress_ip: std::net::Ipv6Addr,
        gateway_ip: Ipv4Addr,
        ifindex: u32,
    ) {
        let key = BpfPolicyKeyV6::new(source_ip, dest_cidr);
        let value = BpfPolicyValueV6::new(egress_ip, gateway_ip, ifindex);
        self.pending_ipv6_rules.insert(key, value);
        self.ipv6_rules_to_delete.remove(&key);
    }

    /// Remove IPv6 rule
    pub fn remove_ipv6_rule(&mut self, source_ip: std::net::Ipv6Addr, dest_cidr: ipnet::Ipv6Net) {
        let key = BpfPolicyKeyV6::new(source_ip, dest_cidr);
        self.pending_ipv6_rules.remove(&key);
        self.ipv6_rules_to_delete.insert(key);
    }

    /// Clear all pending changes
    pub fn clear(&mut self) {
        self.pending_ipv4_rules.clear();
        self.ipv4_rules_to_delete.clear();
        self.pending_ipv6_rules.clear();
        self.ipv6_rules_to_delete.clear();
    }
}

impl Default for Reconciler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_bpf_policy_key_v4() {
        let key1 = BpfPolicyKeyV4::new(
            Ipv4Addr::new(10, 0, 0, 1),
            ipnet::Ipv4Net::from_str("10.0.0.0/8").unwrap(),
        );
        let key2 = BpfPolicyKeyV4::new(
            Ipv4Addr::new(10, 0, 0, 1),
            ipnet::Ipv4Net::from_str("10.0.0.0/8").unwrap(),
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_bpf_policy_value_v4_matches() {
        let val = BpfPolicyValueV4::new(Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(192, 168, 1, 1));
        assert!(val.matches(Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!val.matches(Ipv4Addr::new(10, 0, 0, 2), Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn test_reconciler_add_ipv4_rule() {
        let mut reconciler = Reconciler::new();
        reconciler.add_ipv4_rule(
            Ipv4Addr::new(10, 0, 0, 1),
            ipnet::Ipv4Net::from_str("10.0.0.0/8").unwrap(),
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(192, 168, 1, 1),
        );

        assert_eq!(reconciler.pending_ipv4_rules.len(), 1);
    }

    #[test]
    fn test_reconciler_remove_ipv4_rule() {
        let mut reconciler = Reconciler::new();
        reconciler.add_ipv4_rule(
            Ipv4Addr::new(10, 0, 0, 1),
            ipnet::Ipv4Net::from_str("10.0.0.0/8").unwrap(),
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(192, 168, 1, 1),
        );

        reconciler.remove_ipv4_rule(
            Ipv4Addr::new(10, 0, 0, 1),
            ipnet::Ipv4Net::from_str("10.0.0.0/8").unwrap(),
        );

        assert!(reconciler.pending_ipv4_rules.is_empty());
        assert_eq!(reconciler.ipv4_rules_to_delete.len(), 1);
    }
}
