//! Core node types and in-memory stores ported from Cilium's `pkg/node`.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};

/// Core node representation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Node name.
    pub name: String,
    /// Cluster name.
    pub cluster: String,
    /// Known node addresses.
    pub ip_addresses: Vec<NodeAddress>,
    /// Primary IPv4 allocation CIDR.
    pub ipv4_alloc_cidr: Option<IpNet>,
    /// Primary IPv6 allocation CIDR.
    pub ipv6_alloc_cidr: Option<IpNet>,
    /// IPv4 health endpoint address.
    pub ipv4_health_addr: Option<Ipv4Addr>,
    /// IPv6 health endpoint address.
    pub ipv6_health_addr: Option<Ipv6Addr>,
    /// IPv4 ingress address.
    pub ipv4_ingress_addr: Option<Ipv4Addr>,
    /// IPv6 ingress address.
    pub ipv6_ingress_addr: Option<Ipv6Addr>,
    /// Cluster identifier.
    pub cluster_id: u32,
    /// Origin of the node information.
    pub source: NodeSource,
    /// Node labels.
    pub labels: HashMap<String, String>,
    /// Node annotations.
    pub annotations: HashMap<String, String>,
    /// WireGuard public key.
    pub wire_guard_pub_key: Option<String>,
}

impl Node {
    /// Returns the preferred node IP for the requested address family.
    #[must_use]
    pub fn get_node_ip(&self, ipv6: bool) -> Option<IpAddr> {
        let mut external = None;

        for address in &self.ip_addresses {
            if !family_matches(address.ip, ipv6) {
                continue;
            }

            match address.addr_type {
                NodeAddressType::InternalIP => return Some(address.ip),
                NodeAddressType::ExternalIP => {
                    if external.is_none() {
                        external = Some(address.ip);
                    }
                }
            }
        }

        external
    }

    /// Returns the first IPv4 internal node address.
    #[must_use]
    pub fn get_internal_ip(&self) -> Option<IpAddr> {
        self.ip_addresses.iter().find_map(|address| match address {
            NodeAddress {
                addr_type: NodeAddressType::InternalIP,
                ip: IpAddr::V4(ip),
            } => Some(IpAddr::V4(*ip)),
            _ => None,
        })
    }

    /// Returns the first IPv4 external node address.
    #[must_use]
    pub fn get_external_ip(&self) -> Option<IpAddr> {
        self.ip_addresses.iter().find_map(|address| match address {
            NodeAddress {
                addr_type: NodeAddressType::ExternalIP,
                ip: IpAddr::V4(ip),
            } => Some(IpAddr::V4(*ip)),
            _ => None,
        })
    }

    /// Returns the IPv4 allocation range.
    #[must_use]
    pub fn get_ipv4_alloc_range(&self) -> Option<&IpNet> {
        self.ipv4_alloc_cidr.as_ref()
    }

    /// Returns the IPv6 allocation range.
    #[must_use]
    pub fn get_ipv6_alloc_range(&self) -> Option<&IpNet> {
        self.ipv6_alloc_cidr.as_ref()
    }

    /// Returns true when the node represents the local node.
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.source == NodeSource::Local
    }

    /// Compares all fields except the source.
    #[must_use]
    pub fn deep_equal(&self, other: &Node) -> bool {
        self.name == other.name
            && self.cluster == other.cluster
            && self.ip_addresses == other.ip_addresses
            && self.ipv4_alloc_cidr == other.ipv4_alloc_cidr
            && self.ipv6_alloc_cidr == other.ipv6_alloc_cidr
            && self.ipv4_health_addr == other.ipv4_health_addr
            && self.ipv6_health_addr == other.ipv6_health_addr
            && self.ipv4_ingress_addr == other.ipv4_ingress_addr
            && self.ipv6_ingress_addr == other.ipv6_ingress_addr
            && self.cluster_id == other.cluster_id
            && self.labels == other.labels
            && self.annotations == other.annotations
            && self.wire_guard_pub_key == other.wire_guard_pub_key
    }
}

/// Node address entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeAddress {
    /// Address category.
    pub addr_type: NodeAddressType,
    /// IP value.
    pub ip: IpAddr,
}

/// Address category for a node IP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeAddressType {
    /// Internal node address.
    InternalIP,
    /// External node address.
    ExternalIP,
}

/// Source of node information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NodeSource {
    /// Local node state.
    Local,
    /// Data loaded from the kvstore.
    KVStore,
    /// Data loaded from Kubernetes.
    Kubernetes,
    /// Data loaded from the CiliumNode custom resource.
    CustomResource,
    /// Generated node information.
    Generated,
    /// Restored node information.
    Restored,
    /// Unspecified source.
    #[default]
    Unspec,
}

/// In-memory node registry.
#[derive(Debug, Clone, Default)]
pub struct NodeManager {
    nodes: Arc<RwLock<HashMap<String, Node>>>,
    local_node: Arc<RwLock<Option<Node>>>,
}

impl NodeManager {
    /// Creates a new node manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            local_node: Arc::new(RwLock::new(None)),
        }
    }

    /// Inserts or updates a node in the registry.
    pub fn node_updated(&self, node: Node) {
        tracing::debug!(cluster = %node.cluster, name = %node.name, "updating node");
        write_lock(&self.nodes).insert(node_key(&node.cluster, &node.name), node);
    }

    /// Removes a node from the registry.
    pub fn node_deleted(&self, cluster: &str, name: &str) {
        tracing::debug!(cluster, name, "deleting node");
        write_lock(&self.nodes).remove(&node_key(cluster, name));
    }

    /// Returns a node by cluster and name.
    #[must_use]
    pub fn get_node(&self, cluster: &str, name: &str) -> Option<Node> {
        read_lock(&self.nodes)
            .get(&node_key(cluster, name))
            .cloned()
    }

    /// Returns a stable snapshot of all known nodes.
    #[must_use]
    pub fn get_nodes(&self) -> Vec<Node> {
        let mut nodes: Vec<Node> = read_lock(&self.nodes).values().cloned().collect();
        nodes.sort_by(compare_nodes);
        nodes
    }

    /// Stores the current local node.
    pub fn set_local_node(&self, node: Node) {
        tracing::debug!(cluster = %node.cluster, name = %node.name, "setting local node");
        *write_lock(&self.local_node) = Some(node);
    }

    /// Returns the current local node snapshot.
    #[must_use]
    pub fn get_local_node(&self) -> Option<Node> {
        read_lock(&self.local_node).clone()
    }

    /// Returns the number of tracked nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        read_lock(&self.nodes).len()
    }
}

/// Local store for the current node state.
#[derive(Debug, Clone)]
pub struct LocalNodeStore {
    node: Arc<RwLock<Node>>,
}

impl LocalNodeStore {
    /// Creates a new local node store.
    #[must_use]
    pub fn new(node: Node) -> Self {
        Self {
            node: Arc::new(RwLock::new(node)),
        }
    }

    /// Returns the current node snapshot.
    #[must_use]
    pub fn get(&self) -> Node {
        read_lock(&self.node).clone()
    }

    /// Updates the current node in place.
    pub fn update<F: FnOnce(&mut Node)>(&self, f: F) {
        let mut node = write_lock(&self.node);
        f(&mut node);
    }

    /// Returns the preferred local IPv4 node IP.
    #[must_use]
    pub fn get_ipv4_node_ip(&self) -> Option<Ipv4Addr> {
        match self.get().get_node_ip(false) {
            Some(IpAddr::V4(ip)) => Some(ip),
            _ => None,
        }
    }

    /// Returns the preferred local IPv6 node IP.
    #[must_use]
    pub fn get_ipv6_node_ip(&self) -> Option<Ipv6Addr> {
        match self.get().get_node_ip(true) {
            Some(IpAddr::V6(ip)) => Some(ip),
            _ => None,
        }
    }
}

/// Returns the canonical `cluster/name` key for a node.
#[must_use]
pub fn node_key(cluster: &str, name: &str) -> String {
    format!("{cluster}/{name}")
}

fn family_matches(ip: IpAddr, ipv6: bool) -> bool {
    matches!((ip, ipv6), (IpAddr::V4(_), false) | (IpAddr::V6(_), true))
}

fn compare_nodes(left: &Node, right: &Node) -> Ordering {
    left.cluster
        .cmp(&right.cluster)
        .then_with(|| left.name.cmp(&right.name))
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node(name: &str, cluster: &str) -> Node {
        Node {
            name: name.into(),
            cluster: cluster.into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_node_get_node_ip_prefers_internal() {
        let mut node = Node {
            name: "n1".into(),
            cluster: "default".into(),
            ..Default::default()
        };
        node.ip_addresses = vec![
            NodeAddress {
                addr_type: NodeAddressType::ExternalIP,
                ip: "1.2.3.4".parse().unwrap(),
            },
            NodeAddress {
                addr_type: NodeAddressType::InternalIP,
                ip: "10.0.0.1".parse().unwrap(),
            },
        ];
        assert_eq!(node.get_node_ip(false), Some("10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_node_getters_return_expected_addresses() {
        let mut node = test_node("n1", "default");
        node.ip_addresses = vec![
            NodeAddress {
                addr_type: NodeAddressType::ExternalIP,
                ip: "203.0.113.10".parse().unwrap(),
            },
            NodeAddress {
                addr_type: NodeAddressType::InternalIP,
                ip: "10.0.0.10".parse().unwrap(),
            },
            NodeAddress {
                addr_type: NodeAddressType::ExternalIP,
                ip: "2001:db8::20".parse().unwrap(),
            },
            NodeAddress {
                addr_type: NodeAddressType::InternalIP,
                ip: "2001:db8::10".parse().unwrap(),
            },
        ];

        assert_eq!(node.get_internal_ip(), Some("10.0.0.10".parse().unwrap()));
        assert_eq!(
            node.get_external_ip(),
            Some("203.0.113.10".parse().unwrap())
        );
        assert_eq!(
            node.get_node_ip(true),
            Some("2001:db8::10".parse().unwrap())
        );
    }

    #[test]
    fn test_node_manager_add_get_delete() {
        let mgr = NodeManager::new();
        let node = Node {
            name: "n1".into(),
            cluster: "c1".into(),
            ..Default::default()
        };
        mgr.node_updated(node.clone());
        assert_eq!(mgr.node_count(), 1);
        assert!(mgr.get_node("c1", "n1").is_some());
        mgr.node_deleted("c1", "n1");
        assert_eq!(mgr.node_count(), 0);
    }

    #[test]
    fn test_node_manager_local_node_round_trip() {
        let mgr = NodeManager::new();
        let mut node = test_node("local", "c1");
        node.source = NodeSource::Local;

        mgr.set_local_node(node.clone());

        assert_eq!(mgr.get_local_node(), Some(node));
    }

    #[test]
    fn test_local_node_store_update() {
        let store = LocalNodeStore::new(test_node("n1", "c1"));

        store.update(|node| {
            node.source = NodeSource::Local;
            node.ip_addresses = vec![
                NodeAddress {
                    addr_type: NodeAddressType::ExternalIP,
                    ip: "198.51.100.1".parse().unwrap(),
                },
                NodeAddress {
                    addr_type: NodeAddressType::InternalIP,
                    ip: "10.0.0.1".parse().unwrap(),
                },
                NodeAddress {
                    addr_type: NodeAddressType::InternalIP,
                    ip: "2001:db8::1".parse().unwrap(),
                },
            ];
            node.labels.insert("role".into(), "worker".into());
        });

        let snapshot = store.get();
        assert!(snapshot.is_local());
        assert_eq!(snapshot.labels.get("role"), Some(&"worker".to_string()));
        assert_eq!(store.get_ipv4_node_ip(), Some("10.0.0.1".parse().unwrap()));
        assert_eq!(
            store.get_ipv6_node_ip(),
            Some("2001:db8::1".parse().unwrap())
        );
    }

    #[test]
    fn test_node_deep_equal_ignores_source() {
        let a = Node {
            name: "x".into(),
            cluster: "c".into(),
            source: NodeSource::Local,
            ..Default::default()
        };
        let mut b = a.clone();
        b.source = NodeSource::KVStore;
        assert!(a.deep_equal(&b));
        b.name = "y".into();
        assert!(!a.deep_equal(&b));
    }

    #[test]
    fn test_node_alloc_range_accessors() {
        let node = Node {
            ipv4_alloc_cidr: Some("10.1.0.0/16".parse().unwrap()),
            ipv6_alloc_cidr: Some("2001:db8::/96".parse().unwrap()),
            ..Default::default()
        };

        assert_eq!(
            node.get_ipv4_alloc_range(),
            Some(&"10.1.0.0/16".parse().unwrap())
        );
        assert_eq!(
            node.get_ipv6_alloc_range(),
            Some(&"2001:db8::/96".parse().unwrap())
        );
    }

    #[test]
    fn test_node_key_uses_cluster_and_name() {
        assert_eq!(node_key("cluster-a", "node-a"), "cluster-a/node-a");
    }

    #[test]
    fn test_node_default_starts_empty() {
        let node = Node::default();

        assert_eq!(node.source, NodeSource::Unspec);
        assert!(node.ip_addresses.is_empty());
        assert!(node.labels.is_empty());
        assert!(node.annotations.is_empty());
    }
}
