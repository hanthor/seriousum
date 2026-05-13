// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Pure-data port of Cilium's `pkg/egressgateway` policy matching logic.
//!
//! This crate intentionally focuses on the core value types and in-memory resolution
//! rules used to decide which egress IP should be applied for a pod and destination.

#![deny(unsafe_code, unused_imports)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Matches endpoints by exact label equality.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LabelSelector {
    /// Labels that must all be present on the target endpoint.
    pub match_labels: HashMap<String, String>,
}

impl LabelSelector {
    /// Returns `true` when every configured label is present with the same value.
    pub fn matches(&self, labels: &HashMap<String, String>) -> bool {
        self.match_labels
            .iter()
            .all(|(key, value)| labels.get(key) == Some(value))
    }
}

/// Internal representation of an egress gateway policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressPolicy {
    /// Policy name.
    pub name: String,
    /// Kubernetes namespace that owns the policy.
    pub namespace: String,
    /// Endpoint selectors evaluated with OR semantics.
    pub endpoint_selectors: Vec<LabelSelector>,
    /// Destination CIDRs covered by the policy.
    pub destination_cidrs: Vec<IpNet>,
    /// Node selected as the egress gateway.
    pub egress_node: String,
    /// Source IP used for SNAT when the policy matches.
    pub egress_ip: Option<IpAddr>,
    /// CIDRs that explicitly bypass egress gateway handling.
    pub excluded_cidrs: Vec<IpNet>,
}

/// Endpoint metadata used during policy resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressEndpoint {
    /// Stable endpoint identifier.
    pub id: u64,
    /// Kubernetes namespace that owns the pod.
    pub namespace: String,
    /// Pod name.
    pub pod_name: String,
    /// Pod IP addresses.
    pub pod_ips: Vec<IpAddr>,
    /// Kubernetes labels available for selector matching.
    pub labels: HashMap<String, String>,
}

/// BPF-facing representation of a resolved egress rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressRule {
    /// Source pod IP.
    pub source_ip: IpAddr,
    /// Destination CIDR programmed into the datapath.
    pub destination_cidr: IpNet,
    /// SNAT IP applied to matching traffic.
    pub egress_ip: IpAddr,
    /// Gateway node that owns the egress IP.
    pub gateway_node: String,
}

/// In-memory manager for egress gateway policies and endpoints.
///
/// Policies are stored in insertion order so first-match resolution stays stable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EgressGatewayManager {
    policies: Vec<EgressPolicy>,
    endpoints: HashMap<u64, EgressEndpoint>,
}

impl EgressGatewayManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a policy or replaces an existing policy with the same namespace/name.
    pub fn add_policy(&mut self, policy: EgressPolicy) -> Option<EgressPolicy> {
        if let Some(existing_policy) = self
            .policies
            .iter_mut()
            .find(|existing| same_policy(existing, &policy.namespace, &policy.name))
        {
            debug!(namespace = %policy.namespace, name = %policy.name, "replacing egress policy");
            return Some(std::mem::replace(existing_policy, policy));
        }

        debug!(namespace = %policy.namespace, name = %policy.name, "adding egress policy");
        self.policies.push(policy);
        None
    }

    /// Deletes a policy by namespace and name.
    pub fn delete_policy(&mut self, namespace: &str, name: &str) -> Option<EgressPolicy> {
        let index = self
            .policies
            .iter()
            .position(|policy| same_policy(policy, namespace, name))?;

        debug!(namespace, name, "deleting egress policy");
        Some(self.policies.remove(index))
    }

    /// Looks up a policy by namespace and name.
    pub fn get_policy(&self, namespace: &str, name: &str) -> Option<&EgressPolicy> {
        self.policies
            .iter()
            .find(|policy| same_policy(policy, namespace, name))
    }

    /// Adds or replaces endpoint metadata.
    pub fn add_endpoint(&mut self, endpoint: EgressEndpoint) -> Option<EgressEndpoint> {
        debug!(endpoint_id = endpoint.id, namespace = %endpoint.namespace, pod_name = %endpoint.pod_name, "adding egress endpoint");
        self.endpoints.insert(endpoint.id, endpoint)
    }

    /// Deletes endpoint metadata by endpoint identifier.
    pub fn delete_endpoint(&mut self, endpoint_id: u64) -> Option<EgressEndpoint> {
        debug!(endpoint_id, "deleting egress endpoint");
        self.endpoints.remove(&endpoint_id)
    }

    /// Returns all policies whose endpoint selectors match the endpoint labels.
    pub fn policies_for_endpoint(&self, endpoint: &EgressEndpoint) -> Vec<&EgressPolicy> {
        self.policies
            .iter()
            .filter(|policy| policy_matches_endpoint(policy, endpoint))
            .collect()
    }

    /// Resolves the egress IP for an endpoint and destination IP.
    ///
    /// The first matching policy whose destination CIDRs contain `dest_ip` wins.
    /// Excluded CIDRs short-circuit to `None`.
    pub fn resolve_egress_ip(&self, endpoint: &EgressEndpoint, dest_ip: IpAddr) -> Option<IpAddr> {
        for policy in self.policies_for_endpoint(endpoint) {
            if !policy
                .destination_cidrs
                .iter()
                .any(|cidr| cidr.contains(&dest_ip))
            {
                continue;
            }

            if policy
                .excluded_cidrs
                .iter()
                .any(|cidr| cidr.contains(&dest_ip))
            {
                debug!(namespace = %policy.namespace, name = %policy.name, destination = %dest_ip, "destination matched excluded CIDR");
                return None;
            }

            debug!(namespace = %policy.namespace, name = %policy.name, destination = %dest_ip, "resolved egress IP from policy");
            return policy.egress_ip;
        }

        debug!(endpoint_id = endpoint.id, destination = %dest_ip, "no egress policy matched destination");
        None
    }
}

fn same_policy(policy: &EgressPolicy, namespace: &str, name: &str) -> bool {
    policy.namespace == namespace && policy.name == name
}

fn policy_matches_endpoint(policy: &EgressPolicy, endpoint: &EgressEndpoint) -> bool {
    policy
        .endpoint_selectors
        .iter()
        .any(|selector| selector.matches(&endpoint.labels))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ip(input: &str) -> IpAddr {
        match input.parse() {
            Ok(ip) => ip,
            Err(error) => panic!("failed to parse IP {input}: {error}"),
        }
    }

    fn parse_net(input: &str) -> IpNet {
        match input.parse() {
            Ok(cidr) => cidr,
            Err(error) => panic!("failed to parse CIDR {input}: {error}"),
        }
    }

    fn labels(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn sample_endpoint() -> EgressEndpoint {
        EgressEndpoint {
            id: 7,
            namespace: "default".to_string(),
            pod_name: "frontend-0".to_string(),
            pod_ips: vec![parse_ip("10.0.0.10")],
            labels: labels(&[("app", "frontend"), ("tier", "web")]),
        }
    }

    fn sample_policy() -> EgressPolicy {
        EgressPolicy {
            name: "egress-policy".to_string(),
            namespace: "default".to_string(),
            endpoint_selectors: vec![LabelSelector {
                match_labels: labels(&[("app", "frontend")]),
            }],
            destination_cidrs: vec![parse_net("1.1.1.0/24")],
            egress_node: "node-a".to_string(),
            egress_ip: Some(parse_ip("192.0.2.10")),
            excluded_cidrs: Vec::new(),
        }
    }

    #[test]
    fn label_selector_matches_positive_and_negative_cases() {
        let selector = LabelSelector {
            match_labels: labels(&[("app", "frontend"), ("tier", "web")]),
        };

        assert!(selector.matches(&labels(&[("app", "frontend"), ("tier", "web")])));
        assert!(!selector.matches(&labels(&[("app", "frontend"), ("tier", "api")])));
        assert!(!selector.matches(&labels(&[("app", "frontend")])));
    }

    #[test]
    fn manager_adds_gets_and_deletes_policy() {
        let mut manager = EgressGatewayManager::new();
        let policy = sample_policy();

        assert!(manager.add_policy(policy.clone()).is_none());
        assert_eq!(
            manager.get_policy("default", "egress-policy"),
            Some(&policy)
        );
        assert_eq!(
            manager.delete_policy("default", "egress-policy"),
            Some(policy)
        );
        assert!(manager.get_policy("default", "egress-policy").is_none());
    }

    #[test]
    fn policies_for_endpoint_returns_matching_policies() {
        let mut manager = EgressGatewayManager::new();
        manager.add_policy(sample_policy());

        let policies = manager.policies_for_endpoint(&sample_endpoint());
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name, "egress-policy");
    }

    #[test]
    fn resolve_egress_ip_returns_policy_egress_ip_for_matching_destination() {
        let mut manager = EgressGatewayManager::new();
        manager.add_policy(sample_policy());

        let resolved = manager.resolve_egress_ip(&sample_endpoint(), parse_ip("1.1.1.9"));
        assert_eq!(resolved, Some(parse_ip("192.0.2.10")));
    }

    #[test]
    fn resolve_egress_ip_returns_none_for_excluded_cidr() {
        let mut manager = EgressGatewayManager::new();
        let mut policy = sample_policy();
        policy.excluded_cidrs = vec![parse_net("1.1.1.128/25")];
        manager.add_policy(policy);

        let resolved = manager.resolve_egress_ip(&sample_endpoint(), parse_ip("1.1.1.200"));
        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_egress_ip_returns_none_when_no_policy_matches() {
        let mut manager = EgressGatewayManager::new();
        let mut policy = sample_policy();
        policy.endpoint_selectors = vec![LabelSelector {
            match_labels: labels(&[("app", "database")]),
        }];
        manager.add_policy(policy);

        let resolved = manager.resolve_egress_ip(&sample_endpoint(), parse_ip("1.1.1.9"));
        assert_eq!(resolved, None);
    }

    #[test]
    fn manager_adds_and_deletes_endpoints() {
        let mut manager = EgressGatewayManager::new();
        let endpoint = sample_endpoint();

        assert!(manager.add_endpoint(endpoint.clone()).is_none());
        assert_eq!(manager.delete_endpoint(endpoint.id), Some(endpoint));
        assert!(manager.delete_endpoint(999).is_none());
    }
}
