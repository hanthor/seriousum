// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Egress gateway policy configuration

use std::collections::HashMap;
use std::net::IpAddr;

use crate::endpoint::EndpointMetadata;
use crate::error::{Error, Result};
use crate::gateway::GatewayConfig;
use crate::types::{EndpointID, LabelSelector, Labels, PolicyID};

/// Policy gateway configuration
#[derive(Debug, Clone)]
pub struct PolicyGateway {
    /// Node selector
    pub node_selector: LabelSelector,
    /// Interface name
    pub interface: Option<String>,
    /// Egress IP
    pub egress_ip: Option<IpAddr>,
}

impl PolicyGateway {
    /// Create new policy gateway
    pub fn new(node_selector: LabelSelector) -> Self {
        Self {
            node_selector,
            interface: None,
            egress_ip: None,
        }
    }
}

/// Parsed egress gateway policy configuration
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// Policy identifier
    pub id: PolicyID,
    /// Endpoint selectors (namespace + pod)
    pub endpoint_selectors: Vec<LabelSelector>,
    /// Node selectors
    pub node_selectors: Vec<LabelSelector>,
    /// Destination CIDRs
    pub destination_cidrs: Vec<ipnet::IpNet>,
    /// Excluded CIDRs
    pub excluded_cidrs: Vec<ipnet::IpNet>,
    /// Policy-level gateway configurations
    pub policy_gateways: Vec<PolicyGateway>,
    /// Runtime gateway configurations
    pub gateway_configs: Vec<GatewayConfig>,
    /// Matched endpoints
    pub matched_endpoints: HashMap<EndpointID, EndpointMetadata>,
    /// Whether IPv6 is needed
    pub ipv6_needed: bool,
}

impl PolicyConfig {
    /// Create a new policy config
    pub fn new(id: PolicyID) -> Self {
        Self {
            id,
            endpoint_selectors: Vec::new(),
            node_selectors: Vec::new(),
            destination_cidrs: Vec::new(),
            excluded_cidrs: Vec::new(),
            policy_gateways: Vec::new(),
            gateway_configs: Vec::new(),
            matched_endpoints: HashMap::new(),
            ipv6_needed: false,
        }
    }

    /// Add endpoint selector
    pub fn add_endpoint_selector(mut self, selector: LabelSelector) -> Self {
        self.endpoint_selectors.push(selector);
        self
    }

    /// Add node selector
    pub fn add_node_selector(mut self, selector: LabelSelector) -> Self {
        self.node_selectors.push(selector);
        self
    }

    /// Add destination CIDR
    pub fn add_destination_cidr(mut self, cidr: ipnet::IpNet) -> Result<Self> {
        if cidr.addr().is_ipv6() {
            self.ipv6_needed = true;
        }
        self.destination_cidrs.push(cidr);
        Ok(self)
    }

    /// Add excluded CIDR
    pub fn add_excluded_cidr(mut self, cidr: ipnet::IpNet) -> Result<Self> {
        self.excluded_cidrs.push(cidr);
        Ok(self)
    }

    /// Add policy gateway
    pub fn add_policy_gateway(mut self, gateway: PolicyGateway) -> Self {
        self.policy_gateways.push(gateway);
        self
    }

    /// Validate policy configuration
    pub fn validate(&self) -> Result<()> {
        if self.destination_cidrs.is_empty() {
            return Err(Error::PolicyError(
                "policy must have at least one destination CIDR".to_string(),
            ));
        }

        if self.policy_gateways.is_empty() {
            return Err(Error::PolicyError(
                "policy must have at least one gateway".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if endpoint matches policy's endpoint selectors
    pub fn matches_endpoint(&self, endpoint: &EndpointMetadata) -> bool {
        let labels = &endpoint.labels;
        for selector in &self.endpoint_selectors {
            if selector.matches(labels) {
                return true;
            }
        }
        false
    }

    /// Check if node labels match policy's node selectors
    pub fn matches_node_labels(&self, node_labels: &Labels) -> bool {
        if self.node_selectors.is_empty() {
            return true;
        }

        for selector in &self.node_selectors {
            if selector.matches(node_labels) {
                return true;
            }
        }
        false
    }

    /// Update matched endpoints based on all available endpoints
    pub fn update_matched_endpoints(
        &mut self,
        all_endpoints: &HashMap<EndpointID, EndpointMetadata>,
        node_labels_map: &HashMap<String, Labels>,
    ) {
        self.matched_endpoints.clear();

        for (ep_id, endpoint) in all_endpoints {
            if self.matches_endpoint(endpoint) {
                let node_labels = node_labels_map
                    .get(&endpoint.node_ip)
                    .cloned()
                    .unwrap_or_default();

                if self.matches_node_labels(&node_labels) {
                    self.matched_endpoints.insert(*ep_id, endpoint.clone());
                }
            }
        }
    }

    /// Iterate through each combination of endpoint and CIDR
    pub fn for_each_endpoint_and_cidr<F>(&self, mut f: F)
    where
        F: FnMut(IpAddr, ipnet::IpNet, bool, &GatewayConfig),
    {
        // Sort gateways by IP for consistent assignment across nodes
        let mut sorted_gateways = self.gateway_configs.clone();
        sorted_gateways.sort_by_key(|a| a.gateway_ip.to_string());

        for endpoint in self.matched_endpoints.values() {
            // Select gateway for this endpoint
            let gateway = if sorted_gateways.len() > 1 {
                let hash = compute_endpoint_hash(endpoint.id);
                let index = (hash as usize) % sorted_gateways.len();
                &sorted_gateways[index]
            } else {
                &sorted_gateways[0]
            };

            // For each IP in the endpoint
            for endpoint_ip in &endpoint.ips {
                // For each destination CIDR
                for cidr in &self.destination_cidrs {
                    f(*endpoint_ip, *cidr, false, gateway);
                }

                // For each excluded CIDR
                for cidr in &self.excluded_cidrs {
                    f(*endpoint_ip, *cidr, true, gateway);
                }
            }
        }
    }
}

/// Compute a hash for endpoint load distribution
fn compute_endpoint_hash(endpoint_id: EndpointID) -> u64 {
    use fnv::FnvHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = FnvHasher::default();
    endpoint_id.0.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_policy_config_creation() {
        let policy = PolicyConfig::new(PolicyID::new("test", "default"));
        assert_eq!(policy.id.name, "test");
        assert_eq!(policy.endpoint_selectors.len(), 0);
    }

    #[test]
    fn test_policy_config_add_cidr() {
        let cidr = ipnet::IpNet::from_str("10.0.0.0/8").unwrap();
        let policy = PolicyConfig::new(PolicyID::new("test", "default"))
            .add_destination_cidr(cidr)
            .unwrap();
        assert_eq!(policy.destination_cidrs.len(), 1);
    }

    #[test]
    fn test_policy_config_validation() {
        let policy = PolicyConfig::new(PolicyID::new("test", "default"));
        assert!(policy.validate().is_err());

        let cidr = ipnet::IpNet::from_str("10.0.0.0/8").unwrap();
        let selector = LabelSelector::new();
        let gateway = PolicyGateway::new(selector);

        let valid_policy = PolicyConfig::new(PolicyID::new("test", "default"))
            .add_destination_cidr(cidr)
            .unwrap()
            .add_policy_gateway(gateway);

        assert!(valid_policy.validate().is_ok());
    }

    #[test]
    fn test_policy_matches_endpoint() {
        let mut endpoint_labels = HashMap::new();
        endpoint_labels.insert("app".to_string(), "web".to_string());

        let endpoint = EndpointMetadata::new(
            EndpointID(1),
            endpoint_labels,
            vec![],
            "192.168.1.1".to_string(),
        );

        let selector = LabelSelector::new().with_match_label("app", "web");

        let policy =
            PolicyConfig::new(PolicyID::new("test", "default")).add_endpoint_selector(selector);

        assert!(policy.matches_endpoint(&endpoint));
    }

    #[test]
    fn test_policy_matches_node_labels() {
        let mut node_labels = HashMap::new();
        node_labels.insert("node-type".to_string(), "gateway".to_string());

        let selector = LabelSelector::new().with_match_label("node-type", "gateway");

        let policy =
            PolicyConfig::new(PolicyID::new("test", "default")).add_node_selector(selector);

        assert!(policy.matches_node_labels(&node_labels));
    }
}
