// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Egress gateway manager

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, error};

use crate::endpoint::EndpointMetadata;
use crate::error::Result;
use crate::gateway::GatewayConfig;
use crate::policy::PolicyConfig;
use crate::reconcile::Reconciler;
use crate::types::{EndpointID, EventBitmap, Labels, Node, PolicyID};

/// Main egress gateway manager
pub struct Manager {
    /// Policy configurations indexed by policy ID
    policies: Arc<RwLock<HashMap<PolicyID, PolicyConfig>>>,
    /// Endpoint metadata indexed by endpoint ID
    endpoints: Arc<RwLock<HashMap<EndpointID, EndpointMetadata>>>,
    /// Node list sorted by name
    nodes: Arc<RwLock<Vec<Node>>>,
    /// Node labels indexed by node IP
    node_labels: Arc<RwLock<HashMap<String, Labels>>>,
    /// Event bitmap tracking changes since last reconciliation
    events: Arc<RwLock<EventBitmap>>,
    /// All caches synced flag
    all_caches_synced: Arc<RwLock<bool>>,
    /// Reconciliation counter
    reconciliation_count: Arc<std::sync::atomic::AtomicU64>,
}

impl Manager {
    /// Create a new egress gateway manager
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            nodes: Arc::new(RwLock::new(Vec::new())),
            node_labels: Arc::new(RwLock::new(HashMap::new())),
            events: Arc::new(RwLock::new(EventBitmap::new())),
            all_caches_synced: Arc::new(RwLock::new(false)),
            reconciliation_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Add or update a policy
    pub fn add_policy(&self, policy: PolicyConfig) -> Result<()> {
        let policy_id = policy.id.clone();

        // Validate policy
        policy.validate()?;

        // Update matched endpoints
        let mut policy = policy;
        let all_endpoints = self.endpoints.read().clone();
        let node_labels = self.node_labels.read().clone();
        policy.update_matched_endpoints(&all_endpoints, &node_labels);

        // Store policy
        self.policies.write().insert(policy_id.clone(), policy);

        // Set event flag
        self.events.write().set_add_policy();

        debug!("Added policy: {}", policy_id);
        Ok(())
    }

    /// Delete a policy
    pub fn delete_policy(&self, policy_id: &PolicyID) -> Result<()> {
        self.policies.write().remove(policy_id);
        self.events.write().set_delete_policy();
        debug!("Deleted policy: {}", policy_id);
        Ok(())
    }

    /// Add or update an endpoint
    pub fn add_endpoint(&self, endpoint: &EndpointMetadata) -> Result<()> {
        // Validate endpoint
        endpoint.validate()?;

        let endpoint_id = endpoint.id;
        self.endpoints.write().insert(endpoint_id, endpoint.clone());

        // Update all policies' matched endpoints
        self.update_all_policies_matched_endpoints();

        // Set event flag
        self.events.write().set_update_endpoint();

        debug!("Added endpoint: {:?}", endpoint_id);
        Ok(())
    }

    /// Delete an endpoint
    pub fn delete_endpoint(&self, endpoint_id: EndpointID) -> Result<()> {
        self.endpoints.write().remove(&endpoint_id);

        // Update all policies' matched endpoints
        self.update_all_policies_matched_endpoints();

        // Set event flag
        self.events.write().set_delete_endpoint();

        debug!("Deleted endpoint: {:?}", endpoint_id);
        Ok(())
    }

    /// Add or update a node
    pub fn add_node(&self, node: Node) -> Result<()> {
        // Store node labels by IP
        self.node_labels
            .write()
            .insert(node.ip.to_string(), node.labels.clone());

        // Add or update node in sorted list
        let mut nodes = self.nodes.write();
        let pos = nodes.binary_search_by(|n| n.name.cmp(&node.name));

        match pos {
            Ok(idx) => {
                nodes[idx] = node;
                debug!("Updated node");
            }
            Err(idx) => {
                nodes.insert(idx, node);
                debug!("Added node");
            }
        }

        // Update all policies' matched endpoints and regenerate gateway configs
        drop(nodes);
        self.update_all_policies_matched_endpoints();
        self.regenerate_all_gateway_configs();

        // Set event flag
        self.events.write().set_update_node();

        Ok(())
    }

    /// Delete a node
    pub fn delete_node(&self, node_name: &str) -> Result<()> {
        let mut nodes = self.nodes.write();

        if let Ok(idx) = nodes.binary_search_by(|n| n.name.as_str().cmp(node_name)) {
            let node = nodes.remove(idx);
            self.node_labels.write().remove(&node.ip.to_string());
            debug!("Deleted node: {}", node_name);
        }

        // Update all policies
        drop(nodes);
        self.update_all_policies_matched_endpoints();
        self.regenerate_all_gateway_configs();

        // Set event flag
        self.events.write().set_delete_node();

        Ok(())
    }

    /// Mark K8s sync as complete
    pub fn set_caches_synced(&self) {
        *self.all_caches_synced.write() = true;
        self.events.write().set_k8s_sync_done();
    }

    /// Check if caches are synced
    pub fn are_caches_synced(&self) -> bool {
        *self.all_caches_synced.read()
    }

    /// Get all policies
    pub fn get_policies(&self) -> Vec<PolicyConfig> {
        self.policies
            .read()
            .values()
            .cloned()
            .collect()
    }

    /// Get all endpoints
    pub fn get_endpoints(&self) -> Vec<EndpointMetadata> {
        self.endpoints
            .read()
            .values()
            .cloned()
            .collect()
    }

    /// Get all nodes
    pub fn get_nodes(&self) -> Vec<Node> {
        self.nodes.read().clone()
    }

    /// Get number of matched endpoints for a policy
    pub fn get_matched_endpoints_count(&self, policy_id: &PolicyID) -> usize {
        self.policies
            .read()
            .get(policy_id)
            .map(|p| p.matched_endpoints.len())
            .unwrap_or(0)
    }

    /// Perform reconciliation
    pub fn reconcile(&self) -> Result<Reconciler> {
        if !self.are_caches_synced() {
            return Ok(Reconciler::new());
        }

        let events = self.events.read();

        // Update matched endpoints if needed
        if events.is_k8s_sync_done() || events.has_endpoint_events() || events.has_node_events() {
            drop(events);
            self.update_all_policies_matched_endpoints();
        } else {
            drop(events);
        }

        // Regenerate gateway configs if needed
        let events = self.events.read();
        if events.is_k8s_sync_done()
            || events.has_policy_events()
            || events.has_node_events()
        {
            drop(events);
            self.regenerate_all_gateway_configs();
        } else {
            drop(events);
        }

        // Generate reconciler
        let mut reconciler = Reconciler::new();

        for policy in self.policies.read().values() {
            policy.for_each_endpoint_and_cidr(|endpoint_ip, dest_cidr, is_excluded, gwc| {
                let gateway_ip = if is_excluded {
                    crate::types::SpecialIPs::EXCLUDED_CIDR_IPV4
                } else if let std::net::IpAddr::V4(v4) = gwc.gateway_ip {
                    v4
                } else {
                    crate::types::SpecialIPs::GATEWAY_NOT_FOUND_IPV4
                };

                match endpoint_ip {
                    std::net::IpAddr::V4(source_ipv4) => {
                        if let ipnet::IpNet::V4(dest_cidr_v4) = dest_cidr {
                            reconciler.add_ipv4_rule(source_ipv4, dest_cidr_v4, gwc.egress_ipv4, gateway_ip);
                        }
                    }
                    std::net::IpAddr::V6(source_ipv6) => {
                        if let ipnet::IpNet::V6(dest_cidr_v6) = dest_cidr {
                            reconciler.add_ipv6_rule(
                                source_ipv6,
                                dest_cidr_v6,
                                gwc.egress_ipv6,
                                gateway_ip,
                                gwc.interface_index,
                            );
                        }
                    }
                }
            });
        }

        // Clear event flags
        self.events.write().clear();

        // Increment reconciliation counter
        self.reconciliation_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(reconciler)
    }

    /// Get reconciliation count
    pub fn get_reconciliation_count(&self) -> u64 {
        self.reconciliation_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    // Helper methods

    fn update_all_policies_matched_endpoints(&self) {
        let all_endpoints = self.endpoints.read().clone();
        let node_labels = self.node_labels.read().clone();

        for policy in self.policies.write().values_mut() {
            policy.update_matched_endpoints(&all_endpoints, &node_labels);
        }
    }

    fn regenerate_all_gateway_configs(&self) {
        let nodes = self.nodes.read().clone();

        for policy in self.policies.write().values_mut() {
            policy.gateway_configs.clear();

            for policy_gw in &policy.policy_gateways {
                let mut gateway_config = GatewayConfig::new();

                for node in &nodes {
                    if policy_gw.node_selector.matches(&node.labels) {
                        gateway_config.set_gateway_ip(node.ip);

                        // If local node, derive gateway config from system
                        if node.is_local
                            && let Err(e) = Self::derive_gateway_config(&mut gateway_config, policy_gw, policy.ipv6_needed)
                        {
                            error!("Failed to derive gateway config: {}", e);
                        }

                        break;
                    }
                }

                if gateway_config.is_valid() {
                    policy.gateway_configs.push(gateway_config);
                }
            }

            // Always have at least one gateway config entry
            if policy.gateway_configs.is_empty() {
                policy.gateway_configs.push(GatewayConfig::new());
            }
        }
    }

    fn derive_gateway_config(
        _gateway_config: &mut GatewayConfig,
        _policy_gw: &crate::policy::PolicyGateway,
        _ipv6_needed: bool,
    ) -> Result<()> {
        // This would normally derive interface and IP info from the system
        // For now, this is a placeholder that would be implemented with actual
        // netlink/system calls to get interface info
        Ok(())
    }
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyGateway;
    use crate::types::LabelSelector;
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn test_manager_creation() {
        let manager = Manager::new();
        assert!(!manager.are_caches_synced());
    }

    #[test]
    fn test_manager_set_caches_synced() {
        let manager = Manager::new();
        manager.set_caches_synced();
        assert!(manager.are_caches_synced());
    }

    #[test]
    fn test_manager_add_endpoint() {
        let manager = Manager::new();

        let endpoint = EndpointMetadata::new(
            EndpointID(1),
            std::collections::HashMap::new(),
            vec![IpAddr::from_str("10.0.0.1").unwrap()],
            "192.168.1.1".to_string(),
        );

        assert!(manager.add_endpoint(&endpoint).is_ok());
        assert_eq!(manager.get_endpoints().len(), 1);
    }

    #[test]
    fn test_manager_add_policy() {
        let manager = Manager::new();

        let cidr = ipnet::IpNet::from_str("10.0.0.0/8").unwrap();
        let selector = LabelSelector::new();
        let gateway = PolicyGateway::new(selector);

        let policy = PolicyConfig::new(PolicyID::new("test", "default"))
            .add_destination_cidr(cidr)
            .unwrap()
            .add_policy_gateway(gateway);

        assert!(manager.add_policy(policy).is_ok());
        assert_eq!(manager.get_policies().len(), 1);
    }

    #[test]
    fn test_manager_add_node() {
        let manager = Manager::new();

        let node = Node::new(
            "node1",
            std::collections::HashMap::new(),
            IpAddr::from_str("192.168.1.1").unwrap(),
            true,
        );

        assert!(manager.add_node(node).is_ok());
        assert_eq!(manager.get_nodes().len(), 1);
    }

    #[test]
    fn test_manager_delete_node() {
        let manager = Manager::new();

        let node = Node::new(
            "node1",
            std::collections::HashMap::new(),
            IpAddr::from_str("192.168.1.1").unwrap(),
            true,
        );

        manager.add_node(node).unwrap();
        assert_eq!(manager.get_nodes().len(), 1);

        manager.delete_node("node1").unwrap();
        assert_eq!(manager.get_nodes().len(), 0);
    }

    #[test]
    fn test_manager_reconciliation() {
        let manager = Manager::new();
        manager.set_caches_synced();

        let result = manager.reconcile();
        assert!(result.is_ok());
    }
}
