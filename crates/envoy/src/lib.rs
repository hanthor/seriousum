//! Core Envoy xDS data models ported from `cilium/pkg/envoy`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tracing::debug;

/// Supported xDS resource kinds used by Cilium's Envoy integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// Envoy listener resources.
    Listener,
    /// Envoy route configuration resources.
    RouteConfiguration,
    /// Envoy cluster resources.
    Cluster,
    /// Envoy endpoint resources.
    ClusterLoadAssignment,
    /// Envoy TLS secret resources.
    Secret,
}

impl ResourceType {
    /// Returns the canonical Envoy v3 type URL for this resource type.
    #[must_use]
    pub fn type_url(&self) -> &'static str {
        match self {
            Self::Listener => "type.googleapis.com/envoy.config.listener.v3.Listener",
            Self::RouteConfiguration => {
                "type.googleapis.com/envoy.config.route.v3.RouteConfiguration"
            }
            Self::Cluster => "type.googleapis.com/envoy.config.cluster.v3.Cluster",
            Self::ClusterLoadAssignment => {
                "type.googleapis.com/envoy.config.endpoint.v3.ClusterLoadAssignment"
            }
            Self::Secret => "type.googleapis.com/envoy.extensions.transport_sockets.tls.v3.Secret",
        }
    }
}

/// Minimal xDS resource metadata tracked by the local cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    /// Resource name, unique within its resource type.
    pub name: String,
    /// Resource kind.
    pub resource_type: ResourceType,
    /// Cache version that last modified this resource.
    pub version: u64,
    /// Whether the resource is a tombstone.
    pub deleted: bool,
}

/// In-memory cache of versioned xDS resources.
#[derive(Debug, Clone)]
pub struct VersionedResourceCache {
    resources: HashMap<ResourceType, HashMap<String, Resource>>,
    version: u64,
}

impl VersionedResourceCache {
    /// Creates an empty resource cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            version: 1,
        }
    }

    /// Adds or replaces a resource and returns the new cache version.
    pub fn upsert(&mut self, mut resource: Resource) -> u64 {
        self.version += 1;
        resource.version = self.version;
        resource.deleted = false;

        debug!(
            resource_name = %resource.name,
            type_url = resource.resource_type.type_url(),
            version = resource.version,
            "upserting Envoy xDS resource"
        );

        self.resources
            .entry(resource.resource_type)
            .or_default()
            .insert(resource.name.clone(), resource);

        self.version
    }

    /// Marks an existing resource as deleted and returns whether the cache changed.
    pub fn delete(&mut self, resource_type: &ResourceType, name: &str) -> bool {
        let Some(resources) = self.resources.get_mut(resource_type) else {
            return false;
        };

        let Some(resource) = resources.get_mut(name) else {
            return false;
        };

        if resource.deleted {
            return false;
        }

        self.version += 1;
        resource.version = self.version;
        resource.deleted = true;

        debug!(
            resource_name = %name,
            type_url = resource_type.type_url(),
            version = resource.version,
            "tombstoning Envoy xDS resource"
        );

        true
    }

    /// Returns all cached resources of the requested type, including tombstones.
    #[must_use]
    pub fn get_resources(&self, resource_type: &ResourceType) -> Vec<&Resource> {
        let Some(resources) = self.resources.get(resource_type) else {
            return Vec::new();
        };

        let mut snapshot: Vec<_> = resources.values().collect();
        snapshot.sort_by(|left, right| left.name.cmp(&right.name));
        snapshot
    }

    /// Returns the current cache version.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns the count of non-deleted resources for the given type.
    #[must_use]
    pub fn count(&self, resource_type: &ResourceType) -> usize {
        self.resources.get(resource_type).map_or(0, |resources| {
            resources
                .values()
                .filter(|resource| !resource.deleted)
                .count()
        })
    }
}

impl Default for VersionedResourceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cilium Envoy network policy resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Policy name.
    pub name: String,
    /// Kubernetes namespace containing the policy.
    pub namespace: String,
    /// Endpoint IPs covered by this policy.
    pub endpoint_ips: Vec<IpAddr>,
    /// Numeric endpoint identifier.
    pub endpoint_id: u64,
    /// Ingress L7 policies keyed by port and protocol.
    pub ingress_per_port_policies: Vec<PortNetworkPolicy>,
    /// Egress L7 policies keyed by port and protocol.
    pub egress_per_port_policies: Vec<PortNetworkPolicy>,
    /// Backing conntrack map name used by the policy.
    pub conntrack_map_name: String,
}

/// L7 policy attached to a single port/protocol tuple.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortNetworkPolicy {
    /// Layer 4 port number.
    pub port: u16,
    /// Layer 4 protocol for this policy.
    pub protocol: PortProtocol,
    /// L7 rules enforced for this port.
    pub rules: Vec<PortNetworkPolicyRule>,
}

/// Protocol selectors used by per-port network policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PortProtocol {
    /// TCP traffic.
    TCP,
    /// UDP traffic.
    UDP,
    /// Any transport protocol.
    Any,
}

/// Single rule inside a per-port network policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortNetworkPolicyRule {
    /// Numeric identities allowed by this rule.
    pub remote_policies: Vec<u64>,
    /// Optional downstream TLS context.
    pub downstream_tls_context: Option<TlsContext>,
    /// Optional upstream TLS context.
    pub upstream_tls_context: Option<TlsContext>,
    /// HTTP header matching rules.
    pub http_rules: Vec<HttpRule>,
}

/// TLS configuration attached to a policy rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsContext {
    /// Server names accepted by the TLS context.
    pub server_names: Vec<String>,
    /// Optional validation context secret reference.
    pub validation_context: Option<String>,
}

/// HTTP rule data used by L7 policy generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRule {
    /// Header matchers required by the rule.
    pub headers: Vec<HttpHeaderMatch>,
}

/// Header matcher used by HTTP rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpHeaderMatch {
    /// Header name.
    pub name: String,
    /// Expected header value.
    pub value: String,
    /// Whether the value should be treated as a regex.
    pub regex: bool,
}

/// Shared cache of active Envoy network policies.
#[derive(Debug, Clone)]
pub struct NetworkPolicyCache {
    policies: Arc<RwLock<HashMap<String, NetworkPolicy>>>,
}

impl NetworkPolicyCache {
    /// Creates an empty network policy cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Inserts or replaces a network policy.
    pub fn upsert(&self, policy: NetworkPolicy) {
        let key = Self::policy_key(&policy.namespace, &policy.name);
        debug!(policy = %key, "upserting Envoy network policy");
        write_lock(&self.policies).insert(key, policy);
    }

    /// Removes a network policy and returns whether one was present.
    pub fn delete(&self, namespace: &str, name: &str) -> bool {
        let key = Self::policy_key(namespace, name);
        let removed = write_lock(&self.policies).remove(&key).is_some();

        if removed {
            debug!(policy = %key, "deleting Envoy network policy");
        }

        removed
    }

    /// Returns a cached network policy by namespace/name.
    #[must_use]
    pub fn get(&self, namespace: &str, name: &str) -> Option<NetworkPolicy> {
        read_lock(&self.policies)
            .get(&Self::policy_key(namespace, name))
            .cloned()
    }

    /// Returns all cached network policies in a stable order.
    #[must_use]
    pub fn get_all(&self) -> Vec<NetworkPolicy> {
        let mut policies: Vec<_> = read_lock(&self.policies).values().cloned().collect();
        policies.sort_by(|left, right| {
            left.namespace
                .cmp(&right.namespace)
                .then_with(|| left.name.cmp(&right.name))
        });
        policies
    }

    /// Returns the number of cached network policies.
    #[must_use]
    pub fn count(&self) -> usize {
        read_lock(&self.policies).len()
    }

    fn policy_key(namespace: &str, name: &str) -> String {
        format!("{namespace}/{name}")
    }
}

impl Default for NetworkPolicyCache {
    fn default() -> Self {
        Self::new()
    }
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

    #[test]
    fn test_resource_type_url() {
        assert!(ResourceType::Listener.type_url().contains("Listener"));
        assert!(ResourceType::Cluster.type_url().contains("Cluster"));
    }

    #[test]
    fn test_versioned_cache_upsert_bumps_version() {
        let mut cache = VersionedResourceCache::new();
        let v0 = cache.version();
        cache.upsert(Resource {
            name: "l1".into(),
            resource_type: ResourceType::Listener,
            version: 0,
            deleted: false,
        });
        assert!(cache.version() > v0);
        assert_eq!(cache.count(&ResourceType::Listener), 1);
    }

    #[test]
    fn test_versioned_cache_delete() {
        let mut cache = VersionedResourceCache::new();
        cache.upsert(Resource {
            name: "c1".into(),
            resource_type: ResourceType::Cluster,
            version: 0,
            deleted: false,
        });
        assert_eq!(cache.count(&ResourceType::Cluster), 1);
        assert!(cache.delete(&ResourceType::Cluster, "c1"));
        assert_eq!(cache.count(&ResourceType::Cluster), 0);
    }

    #[test]
    fn test_versioned_cache_get_resources_keeps_tombstone() {
        let mut cache = VersionedResourceCache::new();
        cache.upsert(Resource {
            name: "route-a".into(),
            resource_type: ResourceType::RouteConfiguration,
            version: 0,
            deleted: false,
        });

        assert!(cache.delete(&ResourceType::RouteConfiguration, "route-a"));

        let resources = cache.get_resources(&ResourceType::RouteConfiguration);
        assert_eq!(resources.len(), 1);
        assert!(resources[0].deleted);
    }

    #[test]
    fn test_network_policy_cache_upsert_get_delete() {
        let cache = NetworkPolicyCache::new();
        let policy = NetworkPolicy {
            name: "p1".into(),
            namespace: "default".into(),
            endpoint_ips: vec![],
            endpoint_id: 1,
            ingress_per_port_policies: vec![],
            egress_per_port_policies: vec![],
            conntrack_map_name: "ct".into(),
        };
        cache.upsert(policy);
        assert!(cache.get("default", "p1").is_some());
        assert_eq!(cache.count(), 1);
        assert!(cache.delete("default", "p1"));
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn test_network_policy_cache_get_all_is_stable() {
        let cache = NetworkPolicyCache::new();
        cache.upsert(NetworkPolicy {
            name: "b".into(),
            namespace: "ns-a".into(),
            endpoint_ips: vec![],
            endpoint_id: 2,
            ingress_per_port_policies: vec![],
            egress_per_port_policies: vec![],
            conntrack_map_name: "ct-b".into(),
        });
        cache.upsert(NetworkPolicy {
            name: "a".into(),
            namespace: "ns-a".into(),
            endpoint_ips: vec![],
            endpoint_id: 1,
            ingress_per_port_policies: vec![],
            egress_per_port_policies: vec![],
            conntrack_map_name: "ct-a".into(),
        });

        let policies = cache.get_all();
        assert_eq!(policies.len(), 2);
        assert_eq!(policies[0].name, "a");
        assert_eq!(policies[1].name, "b");
    }
}
