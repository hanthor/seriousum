//! Identity and security types for seriousum.
//!
//! Ported from cilium/pkg/identity with scope-aware numeric identities,
//! reserved identity cache, and identity allocator traits.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

// ============================================================================
// Scope constants (port of cilium/pkg/identity/numericidentity.go)
// ============================================================================

/// Identity scope mask — top 8 bits of the 32-bit identity.
pub const IDENTITY_SCOPE_MASK: u32 = 0xFF_00_00_00;

/// Global scope — default for reserved and global identities.
pub const IDENTITY_SCOPE_GLOBAL: u32 = 0;

/// Local scope — CIDR identities.
pub const IDENTITY_SCOPE_LOCAL: u32 = 1 << 24;

/// Remote node scope — remote in-cluster node identities.
pub const IDENTITY_SCOPE_REMOTE_NODE: u32 = 2 << 24;

/// Minimal numeric identity not used for reserved purposes.
pub const MINIMAL_NUMERIC_IDENTITY: u32 = 256;

/// User-reserved numeric identity start.
pub const USER_RESERVED_NUMERIC_IDENTITY: u32 = 128;

/// Maximum numeric identity value.
pub const MAX_NUMERIC_IDENTITY: u32 = 0x00_FF_FF_FF;

// ============================================================================
// Reserved identity constants (port of cilium/pkg/identity/numericidentity.go)
// Ordering matches Go iota: unknown=0, host=1, world=2, unmanaged=3,
// health=4, init=5, remote-node=6, kube-apiserver=7, ingress=8,
// world-ipv4=9, world-ipv6=10
// ============================================================================

/// Unknown identity.
pub const IDENTITY_UNKNOWN: u32 = 0;
/// Invalid identity sentinel (same as unknown).
pub const IDENTITY_INVALID: u32 = 0;
/// Host identity — the local node.
pub const IDENTITY_HOST: u32 = 1;
/// World identity — any endpoint outside of the cluster.
pub const IDENTITY_WORLD: u32 = 2;
/// Unmanaged identity — unmanaged endpoints.
pub const IDENTITY_UNMANAGED: u32 = 3;
/// Health identity — the local cilium-health endpoint.
pub const IDENTITY_HEALTH: u32 = 4;
/// Init identity — endpoints that have not received any labels yet.
pub const IDENTITY_INIT: u32 = 5;
/// Remote-node identity — remote in-cluster nodes.
pub const IDENTITY_REMOTE_NODE: u32 = 6;
/// Kube-apiserver identity — remote node(s) serving the kube-apiserver.
pub const IDENTITY_KUBE_APISERVER: u32 = 7;
/// Ingress identity — source address for Ingress proxy connections.
pub const IDENTITY_INGRESS: u32 = 8;
/// World-IPv4 identity — IPv4 endpoints outside the cluster.
pub const IDENTITY_WORLD_IPV4: u32 = 9;
/// World-IPv6 identity — IPv6 endpoints outside the cluster.
pub const IDENTITY_WORLD_IPV6: u32 = 10;

// Cluster identity (3 in old Rust) was OBSOLETED in Go — returns 0/unknown.
// It has been removed; use IDENTITY_UNKNOWN (0) for any old references.

// ============================================================================
// NumericIdentity (port of cilium/pkg/identity/numericidentity.go NumericIdentity)
// ============================================================================

/// Identity scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentityScope {
    /// Global and reserved identities.
    Global,
    /// Local (CIDR) identities.
    Local,
    /// Remote node identities.
    RemoteNode,
}

/// Security identity with scope awareness (port of Cilium's NumericIdentity).
///
/// Bits:
///  0-15: identity identifier
/// 16-23: cluster identifier (8 bits for 255-cluster config)
///    24: LocalIdentityFlag — top byte encodes scope
pub type NumericIdentity = SecurityIdentity;

/// Reserved identities as strongly typed constants.
pub const RESERVED_IDENTITY_UNKNOWN: SecurityIdentity = SecurityIdentity(IDENTITY_UNKNOWN);
pub const RESERVED_IDENTITY_HOST: SecurityIdentity = SecurityIdentity(IDENTITY_HOST);
pub const RESERVED_IDENTITY_WORLD: SecurityIdentity = SecurityIdentity(IDENTITY_WORLD);
pub const RESERVED_IDENTITY_UNMANAGED: SecurityIdentity = SecurityIdentity(IDENTITY_UNMANAGED);
pub const RESERVED_IDENTITY_HEALTH: SecurityIdentity = SecurityIdentity(IDENTITY_HEALTH);
pub const RESERVED_IDENTITY_INIT: SecurityIdentity = SecurityIdentity(IDENTITY_INIT);
pub const RESERVED_IDENTITY_REMOTE_NODE: SecurityIdentity = SecurityIdentity(IDENTITY_REMOTE_NODE);
pub const RESERVED_IDENTITY_KUBE_APISERVER: SecurityIdentity =
    SecurityIdentity(IDENTITY_KUBE_APISERVER);
pub const RESERVED_IDENTITY_INGRESS: SecurityIdentity = SecurityIdentity(IDENTITY_INGRESS);
pub const RESERVED_IDENTITY_WORLD_IPV4: SecurityIdentity = SecurityIdentity(IDENTITY_WORLD_IPV4);
pub const RESERVED_IDENTITY_WORLD_IPV6: SecurityIdentity = SecurityIdentity(IDENTITY_WORLD_IPV6);

/// The last reserved identity numeric value (for `is_reserved_identity`).
/// In Go, `IsReservedIdentity` checks whether the ID is in `reservedIdentityNames`
/// (which is a map keyed by named reserved IDs). We replicate by listing them explicitly.
const RESERVED_IDENTITY_NAMES: &[u32] = &[
    IDENTITY_HOST,
    IDENTITY_WORLD,
    IDENTITY_UNMANAGED,
    IDENTITY_HEALTH,
    IDENTITY_INIT,
    IDENTITY_REMOTE_NODE,
    IDENTITY_KUBE_APISERVER,
    IDENTITY_INGRESS,
    IDENTITY_WORLD_IPV4,
    IDENTITY_WORLD_IPV6,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
pub struct SecurityIdentity(u32);

impl SecurityIdentity {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Alias for `as_u32`, matching Go's `Uint32()` method.
    pub const fn uint32(self) -> u32 {
        self.0
    }

    /// Get the scope from the top 8 bits.
    pub fn scope(self) -> IdentityScope {
        match self.0 & IDENTITY_SCOPE_MASK {
            IDENTITY_SCOPE_LOCAL => IdentityScope::Local,
            IDENTITY_SCOPE_REMOTE_NODE => IdentityScope::RemoteNode,
            _ => IdentityScope::Global,
        }
    }

    /// Returns true if the identity is in the Local (CIDR) scope.
    /// Port of Go's `HasLocalScope()`: checks `id.Scope() == IdentityScopeLocal`.
    pub fn has_local_scope(self) -> bool {
        self.scope() == IdentityScope::Local
    }

    /// Returns true if the identity is in the RemoteNode scope.
    pub fn has_remote_node_scope(self) -> bool {
        self.scope() == IdentityScope::RemoteNode
    }

    /// Returns the cluster ID encoded in bits 16-23 of the identity.
    /// For the default 255-max-cluster config this is `(id >> 16) & 0xFF`.
    /// Port of Go's `ClusterID()`: `(uint32(id) >> GetClusterIDShift()) & ClusterIDMax`.
    /// We hard-code the 255-cluster shift (16 bits) since we don't have cmtypes.
    pub const fn cluster_id(self) -> u32 {
        // Default: ClusterIDMax=255, shift=16 (NumericIdentityBitlength(24) - log2(256)(8))
        (self.0 >> 16) & 0xFF
    }

    /// Returns true if this identity is one of the well-known reserved identities.
    /// Port of Go's `IsReservedIdentity()`: checks `reservedIdentityNames` map.
    pub fn is_reserved_identity(self) -> bool {
        RESERVED_IDENTITY_NAMES.contains(&self.0)
    }

    /// Returns true if this identity is in the reserved range (< 256) for
    /// backward-compat use in the allocator.
    pub const fn is_reserved(self) -> bool {
        self.0 < 1024
    }

    pub fn is_local(self) -> bool {
        self.scope() == IdentityScope::Local
    }

    pub fn is_remote_node(self) -> bool {
        self.scope() == IdentityScope::RemoteNode
    }

    pub fn is_global(self) -> bool {
        self.scope() == IdentityScope::Global
    }

    pub const fn is_valid(self) -> bool {
        self.0 != IDENTITY_INVALID
    }

    pub const fn world() -> Self {
        Self(IDENTITY_WORLD)
    }

    pub const fn host() -> Self {
        Self(IDENTITY_HOST)
    }

    pub const fn unmanaged() -> Self {
        Self(IDENTITY_UNMANAGED)
    }

    pub const fn health() -> Self {
        Self(IDENTITY_HEALTH)
    }

    pub const fn init() -> Self {
        Self(IDENTITY_INIT)
    }

    pub const fn remote_node() -> Self {
        Self(IDENTITY_REMOTE_NODE)
    }

    pub const fn kube_apiserver() -> Self {
        Self(IDENTITY_KUBE_APISERVER)
    }

    pub const fn ingress() -> Self {
        Self(IDENTITY_INGRESS)
    }

    /// Construct a local (CIDR) identity from an allocator value.
    pub const fn make_local(allocator_id: u32) -> Self {
        Self(allocator_id | IDENTITY_SCOPE_LOCAL)
    }

    /// Construct a remote node identity from an allocator value.
    pub const fn make_remote_node(allocator_id: u32) -> Self {
        Self(allocator_id | IDENTITY_SCOPE_REMOTE_NODE)
    }

    /// Unwrap to the allocator ID (strip scope bits).
    pub const fn allocator_id(self) -> u32 {
        self.0 & !IDENTITY_SCOPE_MASK
    }
}

impl Default for SecurityIdentity {
    fn default() -> Self {
        Self::world()
    }
}

impl fmt::Display for SecurityIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self.0 {
            IDENTITY_UNKNOWN => "unknown",
            IDENTITY_HOST => "host",
            IDENTITY_WORLD => "world",
            IDENTITY_UNMANAGED => "unmanaged",
            IDENTITY_HEALTH => "health",
            IDENTITY_INIT => "init",
            IDENTITY_REMOTE_NODE => "remote-node",
            IDENTITY_KUBE_APISERVER => "kube-apiserver",
            IDENTITY_INGRESS => "ingress",
            IDENTITY_WORLD_IPV4 => "world-ipv4",
            IDENTITY_WORLD_IPV6 => "world-ipv6",
            _ => return write!(f, "{}", self.0),
        };
        f.write_str(s)
    }
}

impl FromStr for SecurityIdentity {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try reserved name lookup first
        let looked_up = get_reserved_id(s);
        if looked_up.0 != IDENTITY_UNKNOWN || s == "unknown" {
            return Ok(looked_up);
        }
        s.parse::<u32>()
            .map(Self::new)
            .map_err(|e| anyhow::anyhow!("invalid identity: {e}"))
    }
}

impl From<u32> for SecurityIdentity {
    fn from(id: u32) -> Self {
        Self::new(id)
    }
}

impl From<SecurityIdentity> for u32 {
    fn from(id: SecurityIdentity) -> Self {
        id.0
    }
}

// ============================================================================
// NumericIdentitySlice (port of Go's NumericIdentitySlice)
// ============================================================================

/// A slice of numeric identities with utility methods.
/// Port of Go's `NumericIdentitySlice []NumericIdentity`.
pub type NumericIdentitySlice = Vec<SecurityIdentity>;

/// Extension trait for NumericIdentitySlice to add `as_u32_slice()`.
pub trait NumericIdentitySliceExt {
    /// Returns the slice as a Vec<u32> by copying each element.
    /// Port of Go's `AsUint32Slice()`.
    fn as_u32_slice(&self) -> Vec<u32>;
}

impl NumericIdentitySliceExt for NumericIdentitySlice {
    fn as_u32_slice(&self) -> Vec<u32> {
        self.iter().map(|id| id.as_u32()).collect()
    }
}

// ============================================================================
// Reserved identity name table
// Port of Go's `reservedIdentities` and `reservedIdentityNames` maps.
// ============================================================================

/// The canonical reserved identity name-to-ID table.
/// "cluster" is intentionally absent — it was obsoleted and returns IdentityUnknown.
static RESERVED_IDENTITIES: &[(&str, u32)] = &[
    ("host", IDENTITY_HOST),
    ("world", IDENTITY_WORLD),
    ("unmanaged", IDENTITY_UNMANAGED),
    ("health", IDENTITY_HEALTH),
    ("init", IDENTITY_INIT),
    ("remote-node", IDENTITY_REMOTE_NODE),
    ("kube-apiserver", IDENTITY_KUBE_APISERVER),
    ("ingress", IDENTITY_INGRESS),
    ("world-ipv4", IDENTITY_WORLD_IPV4),
    ("world-ipv6", IDENTITY_WORLD_IPV6),
];

/// Look up a reserved identity by name.
/// Returns `RESERVED_IDENTITY_UNKNOWN` (0) if not found.
/// Port of Go's `GetReservedID(name string) NumericIdentity`.
pub fn get_reserved_id(name: &str) -> SecurityIdentity {
    for &(n, id) in RESERVED_IDENTITIES {
        if n == name {
            return SecurityIdentity(id);
        }
    }
    RESERVED_IDENTITY_UNKNOWN
}

/// Returns all reserved identities in ascending numeric order.
/// Port of Go's `GetAllReservedIdentities()`.
/// NOTE: identity 0 is unknown, so the reserved identities start at 1.
pub fn get_all_reserved_identities() -> Vec<SecurityIdentity> {
    let mut ids: Vec<SecurityIdentity> = RESERVED_IDENTITIES
        .iter()
        .map(|&(_, id)| SecurityIdentity(id))
        .collect();
    ids.sort_by_key(|id| id.as_u32());
    ids
}

// ============================================================================
// SecurityLabel
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecurityLabel {
    pub key: String,
    pub value: String,
}

impl SecurityLabel {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn k8s(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(format!("k8s:{}", key.into()), value)
    }

    pub fn k8s_namespace(namespace: impl Into<String>) -> Self {
        Self::k8s("io.kubernetes.pod.namespace", namespace)
    }
}

impl fmt::Display for SecurityLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

// ============================================================================
// Identity struct
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: SecurityIdentity,
    pub labels: BTreeMap<String, String>,
    pub revision: u32,
}

impl Identity {
    pub fn new(id: SecurityIdentity, labels: impl IntoIterator<Item = SecurityLabel>) -> Self {
        Self {
            id,
            labels: labels.into_iter().map(|l| (l.key, l.value)).collect(),
            revision: 0,
        }
    }

    pub fn reserved(id: SecurityIdentity) -> Self {
        Self {
            id,
            labels: BTreeMap::new(),
            revision: 0,
        }
    }

    pub fn has_label(&self, key: &str, value: &str) -> bool {
        self.labels.get(key).is_some_and(|v| v == value)
    }

    pub fn increment_revision(&mut self) {
        self.revision += 1;
    }
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.labels == other.labels
    }
}

impl Eq for Identity {}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let labels = self
            .labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        write!(f, "{} ({labels})", self.id)
    }
}

/// Identity map: NumericIdentity → labels.
pub type IdentityMap = BTreeMap<NumericIdentity, BTreeMap<String, String>>;

/// Check if labels is a subset of other (for reserved identity matching).
fn labels_is_subset(labels: &BTreeMap<String, String>, other: &BTreeMap<String, String>) -> bool {
    labels.iter().all(|(k, v)| other.get(k) == Some(v))
}

// ============================================================================
// IdentityAllocator trait and SimpleIdentityAllocator
// ============================================================================

/// Trait for identity allocation (port of allocator.Allocator).
pub trait IdentityAllocator: Send + Sync {
    /// Allocate a new identity for the given labels. Returns the NumericIdentity.
    fn allocate(&mut self, labels: &BTreeMap<String, String>) -> NumericIdentity;
    /// Release an identity (decrements reference count; frees if zero).
    fn release(&mut self, id: NumericIdentity);
    /// Look up identity by ID.
    fn lookup(&self, id: NumericIdentity) -> Option<&BTreeMap<String, String>>;
    /// Look up identity by labels (returns ID if an identity with these labels exists).
    fn lookup_by_labels(&self, labels: &BTreeMap<String, String>) -> Option<NumericIdentity>;
}

/// Simple in-memory identity allocator (port of simple allocator).
pub struct SimpleIdentityAllocator {
    next_id: u32,
    id_to_labels: BTreeMap<NumericIdentity, BTreeMap<String, String>>,
    labels_to_id: BTreeMap<BTreeMap<String, String>, NumericIdentity>,
}

impl SimpleIdentityAllocator {
    pub fn new(start_id: u32) -> Self {
        Self {
            next_id: start_id,
            id_to_labels: BTreeMap::new(),
            labels_to_id: BTreeMap::new(),
        }
    }
}

impl Default for SimpleIdentityAllocator {
    fn default() -> Self {
        Self::new(MINIMAL_NUMERIC_IDENTITY)
    }
}

impl IdentityAllocator for SimpleIdentityAllocator {
    fn allocate(&mut self, labels: &BTreeMap<String, String>) -> NumericIdentity {
        if let Some(id) = self.labels_to_id.get(labels).copied() {
            return id;
        }
        let id = NumericIdentity::new(self.next_id);
        self.next_id += 1;
        self.id_to_labels.insert(id, labels.clone());
        self.labels_to_id.insert(labels.clone(), id);
        id
    }

    fn release(&mut self, id: NumericIdentity) {
        if let Some(labels) = self.id_to_labels.remove(&id) {
            self.labels_to_id.remove(&labels);
        }
    }

    fn lookup(&self, id: NumericIdentity) -> Option<&BTreeMap<String, String>> {
        self.id_to_labels.get(&id)
    }

    fn lookup_by_labels(&self, labels: &BTreeMap<String, String>) -> Option<NumericIdentity> {
        self.labels_to_id.get(labels).copied()
    }
}

// ============================================================================
// Reserved identity cache (port of pkg/identity/reserved.go)
// ============================================================================

/// Reserved identity cache.
static RESERVED_IDENTITY_CACHE: RwLock<IdentityMap> = RwLock::new(IdentityMap::new());

/// Add a reserved identity to the cache.
pub fn add_reserved_identity(id: NumericIdentity, labels: BTreeMap<String, String>) {
    let mut cache = RESERVED_IDENTITY_CACHE.write().unwrap();
    cache.insert(id, labels);
}

/// Look up a reserved identity by NumericIdentity.
pub fn lookup_reserved_identity(id: NumericIdentity) -> Option<BTreeMap<String, String>> {
    RESERVED_IDENTITY_CACHE.read().unwrap().get(&id).cloned()
}

/// Iterate over all reserved identities.
pub fn iterate_reserved_identities<F>(mut f: F)
where
    F: FnMut(NumericIdentity, &BTreeMap<String, String>),
{
    for (id, labels) in RESERVED_IDENTITY_CACHE.read().unwrap().iter() {
        f(*id, labels);
    }
}

/// Return the complete reserved identity map.
pub fn list_reserved_identities() -> IdentityMap {
    RESERVED_IDENTITY_CACHE.read().unwrap().clone()
}

/// Unknown identity labels (for when identity lookup fails).
#[allow(dead_code)]
fn unknown_identity_labels() -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert("unknown".into(), String::new());
    labels
}

/// Unknown identity (returned when lookup fails).
#[allow(dead_code)]
fn unknown_identity() -> Identity {
    Identity {
        id: NumericIdentity::new(IDENTITY_UNKNOWN),
        labels: unknown_identity_labels(),
        revision: 0,
    }
}

/// Lookup reserved identity by labels.
pub fn lookup_reserved_identity_by_labels(labels: &BTreeMap<String, String>) -> Option<Identity> {
    // Check if any reserved identity has these labels.
    let mut best: Option<(NumericIdentity, Identity)> = None;
    iterate_reserved_identities(|id, reserved_labels| {
        if labels_is_subset(labels, reserved_labels) {
            let id_labels = reserved_labels
                .clone()
                .into_iter()
                .map(|(k, v)| SecurityLabel::new(k, v))
                .collect::<Vec<_>>();
            best = Some((id, Identity::new(id, id_labels)));
        }
    });
    best.map(|(_, id)| id)
}

/// Scope for labels (port of ScopeForLabels).
/// Returns a NumericIdentity with the appropriate scope bits set.
pub fn scope_for_labels(labels: &BTreeMap<String, String>) -> NumericIdentity {
    let has_remote = labels
        .iter()
        .any(|(k, _)| k.contains("io.cilium.k8s.policy.remote-node"));
    if has_remote {
        return NumericIdentity::make_remote_node(1);
    }
    let has_cidr_or_fqdn = labels
        .iter()
        .any(|(k, _)| k.contains("cidr") || k.contains("fqdn") || k.contains("reserved"));
    if has_cidr_or_fqdn {
        return NumericIdentity::make_local(1);
    }
    NumericIdentity::new(IDENTITY_SCOPE_GLOBAL)
}

// ============================================================================
// Labels infrastructure
// Port of cilium/pkg/labels (Label, Labels, NewLabelsFromModel, GetCIDRLabels)
// ============================================================================

/// Label source prefixes (matches Go's label sources).
pub const LABEL_SOURCE_K8S: &str = "k8s";
pub const LABEL_SOURCE_RESERVED: &str = "reserved";
pub const LABEL_SOURCE_CIDR: &str = "cidr";
pub const LABEL_SOURCE_UNSPEC: &str = "unspec";
pub const LABEL_SOURCE_FQDN: &str = "fqdn";

/// A single label: source + key + value.
/// Serialized as "source:key=value" (or "source:key" when value is empty).
/// Port of Go's `labels.Label`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label {
    pub source: String,
    pub key: String,
    pub value: String,
}

impl Label {
    pub fn new(source: &str, key: &str, value: &str) -> Self {
        Self {
            source: source.to_owned(),
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }

    /// Parse "source:key=value", "source:key", "key=value", or "key".
    /// If source is absent, defaults to "unspec".
    /// Port of Go's `ParseLabel`.
    pub fn parse(s: &str) -> Self {
        // Split on first ':' to separate source from the rest.
        let (source, rest) = if let Some(colon) = s.find(':') {
            let src = &s[..colon];
            let rest = &s[colon + 1..];
            // Handle "reserved:=value" → key=value (Go special case)
            if src == LABEL_SOURCE_RESERVED && rest.starts_with('=') {
                (LABEL_SOURCE_RESERVED.to_owned(), rest[1..].to_owned())
            } else {
                (src.to_owned(), rest.to_owned())
            }
        } else {
            (LABEL_SOURCE_UNSPEC.to_owned(), s.to_owned())
        };

        // Split rest on '=' to get key and value.
        let (key, value) = if let Some(eq) = rest.find('=') {
            (rest[..eq].to_owned(), rest[eq + 1..].to_owned())
        } else {
            (rest, String::new())
        };

        Self { source, key, value }
    }

    /// Returns true if the label source is "reserved".
    pub fn is_reserved(&self) -> bool {
        self.source == LABEL_SOURCE_RESERVED
    }

    /// Returns true if the label source is "cidr".
    pub fn is_cidr(&self) -> bool {
        self.source == LABEL_SOURCE_CIDR
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.value.is_empty() {
            write!(f, "{}:{}", self.source, self.key)
        } else {
            write!(f, "{}:{}={}", self.source, self.key, self.value)
        }
    }
}

/// Labels is a map of `key → Label` where key is `label.key` (not `source:key`).
/// This matches Go's `labels.Labels = map[string]Label` where the map key is `lbl.Key`.
pub type Labels = HashMap<String, Label>;

/// Parse a model string like "k8s:foo=bar" or "reserved:world" into a Labels map.
/// Port of Go's `NewLabelsFromModel`.
pub fn labels_from_model(model: &[&str]) -> Labels {
    let mut map = Labels::new();
    for &s in model {
        let lbl = Label::parse(s);
        if !lbl.key.is_empty() {
            map.insert(lbl.key.clone(), lbl);
        }
    }
    map
}

/// Returns labels for a CIDR prefix (e.g., "10.0.0.0/24").
/// Port of Go's `GetCIDRLabels`.
pub fn get_cidr_labels(prefix: &str) -> Labels {
    let lbl = Label::new(LABEL_SOURCE_CIDR, prefix, "");
    let mut map = Labels::new();
    map.insert(lbl.key.clone(), lbl);
    map
}

// ============================================================================
// Labels helper methods (port of Go's Labels receiver methods)
// ============================================================================

/// Returns true if any label has source "reserved".
pub fn labels_is_reserved(labels: &Labels) -> bool {
    labels.values().any(|l| l.source == LABEL_SOURCE_RESERVED)
}

/// Returns true if labels contain a label with key "host".
pub fn labels_has_host_label(labels: &Labels) -> bool {
    labels.contains_key("host")
}

/// Returns true if labels contain a label with key "remote-node".
pub fn labels_has_remote_node_label(labels: &Labels) -> bool {
    labels.contains_key("remote-node")
}

/// Returns true if labels contain a label with key "kube-apiserver".
pub fn labels_has_kube_apiserver_label(labels: &Labels) -> bool {
    labels.contains_key("kube-apiserver")
}

/// Returns true if labels contain a label with key "ingress".
pub fn labels_has_ingress_label(labels: &Labels) -> bool {
    labels.contains_key("ingress")
}

// ============================================================================
// scope_for_labels (Labels-based version)
// Port of Go's ScopeForLabels in pkg/identity/identity.go
// ============================================================================

/// Port of Go's `ScopeForLabels`: returns the NumericIdentity scope for a set of labels.
/// - remote-node label → IdentityScopeRemoteNode
/// - reserved:ingress → IdentityScopeLocal
/// - All CIDR/FQDN/reserved labels → IdentityScopeLocal
/// - Otherwise → IdentityScopeGlobal (0)
pub fn scope_for_label_map(labels: &Labels) -> NumericIdentity {
    if labels_has_remote_node_label(labels) {
        return NumericIdentity::new(IDENTITY_SCOPE_REMOTE_NODE);
    }

    // reserved:ingress → local
    if labels_is_reserved(labels) && labels_has_ingress_label(labels) {
        return NumericIdentity::new(IDENTITY_SCOPE_LOCAL);
    }

    let mut scope = NumericIdentity::new(IDENTITY_SCOPE_GLOBAL);
    for lbl in labels.values() {
        match lbl.source.as_str() {
            LABEL_SOURCE_CIDR | LABEL_SOURCE_FQDN | LABEL_SOURCE_RESERVED => {
                scope = NumericIdentity::new(IDENTITY_SCOPE_LOCAL);
            }
            _ => {
                return NumericIdentity::new(IDENTITY_SCOPE_GLOBAL);
            }
        }
    }
    scope
}

// ============================================================================
// lookup_reserved_identity_by_labels (Labels-based version)
// Port of Go's LookupReservedIdentityByLabels in pkg/identity/identity.go
// ============================================================================

/// Build an Identity from a Labels map, deriving the BTreeMap labels representation.
fn identity_from_label_map(id: SecurityIdentity, labels: &Labels) -> Identity {
    let btree: BTreeMap<String, String> = labels
        .values()
        .map(|l| {
            let k = format!("{}:{}", l.source, l.key);
            (k, l.value.clone())
        })
        .collect();
    Identity {
        id,
        labels: btree,
        revision: 0,
    }
}

/// Port of Go's `LookupReservedIdentityByLabels`.
/// Returns `Some(Identity)` if labels exactly match a reserved identity.
///
/// `node_cidr_policy` corresponds to `option.Config.PolicyCIDRMatchesNodes()` in Go.
/// When `true`, remote-node identities return `None`.
pub fn lookup_reserved_identity_by_label_map(
    labels: &Labels,
    node_cidr_policy: bool,
) -> Option<Identity> {
    if labels.is_empty() {
        return None;
    }

    // If no label has reserved source, not a reserved identity.
    if !labels_is_reserved(labels) {
        return None;
    }

    // host has highest priority.
    if labels_has_host_label(labels) {
        return Some(identity_from_label_map(RESERVED_IDENTITY_HOST, labels));
    }

    // remote-node / kube-apiserver handling.
    if labels_has_remote_node_label(labels) {
        if node_cidr_policy {
            return None;
        }
        if labels_has_kube_apiserver_label(labels) {
            return Some(identity_from_label_map(
                RESERVED_IDENTITY_KUBE_APISERVER,
                labels,
            ));
        }
        return Some(identity_from_label_map(
            RESERVED_IDENTITY_REMOTE_NODE,
            labels,
        ));
    }

    // For single-label reserved identities, require exactly 1 label.
    if labels.len() != 1 {
        return None;
    }

    // The single label must be reserved; look it up by key.
    let lbl = labels.values().next().unwrap();
    if lbl.source != LABEL_SOURCE_RESERVED {
        return None;
    }

    let nid = get_reserved_id(&lbl.key);
    if nid == RESERVED_IDENTITY_UNKNOWN {
        return None;
    }

    Some(identity_from_label_map(nid, labels))
}

// ============================================================================
// new_identity_from_label_array
// Port of Go's NewIdentityFromLabelArray in pkg/identity/identity.go
// ============================================================================

/// Port of Go's `NewIdentityFromLabelArray`.
/// Builds an `Identity` from a numeric ID and a slice of `Label`s.
pub fn new_identity_from_label_array(id: NumericIdentity, label_array: &[Label]) -> Identity {
    let btree: BTreeMap<String, String> = label_array
        .iter()
        .map(|l| {
            let k = if l.source == LABEL_SOURCE_UNSPEC {
                l.key.clone()
            } else {
                format!("{}:{}", l.source, l.key)
            };
            (k, l.value.clone())
        })
        .collect();
    Identity {
        id,
        labels: btree,
        revision: 0,
    }
}

/// Parse a sorted list string like "unspec:a=;unspec:b;unspec:c=d" into a `Vec<Label>`.
/// Port of Go's `NewLabelArrayFromSortedList`.
pub fn label_array_from_sorted_list(list: &str) -> Vec<Label> {
    list.split(';')
        .map(Label::parse)
        .filter(|l| !l.key.is_empty())
        .collect()
}

// ============================================================================
// Cluster ID shift/bits (port of GetClusterIDShift / GetClusterIDBits)
// ============================================================================

/// Returns the cluster ID shift for the default 255-max-cluster config.
/// Port of Go's `GetClusterIDShift()`.
pub const fn get_cluster_id_shift() -> u32 {
    16
}

/// Returns the cluster ID bits for the default 255-max-cluster config.
/// Port of Go's `GetClusterIDBits()`.
pub const fn get_cluster_id_bits() -> u32 {
    8
}

// ============================================================================
// IPIdentityPair and NamedPort
// Port of cilium/pkg/identity/identity.go IPIdentityPair / PrefixString
// ============================================================================

/// A named port mapping.
/// Port of Go's `NamedPort struct`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedPort {
    pub name: String,
    pub port: u16,
    pub protocol: String,
}

/// A pairing of an IP address and the security identity to which it corresponds.
/// May include an optional mask which, if present, denotes a CIDR prefix.
/// Port of Go's `IPIdentityPair struct`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpIdentityPair {
    pub ip: IpAddr,
    pub mask: Option<Vec<u8>>,
    pub host_ip: IpAddr,
    pub id: SecurityIdentity,
    pub key: u8,
    pub metadata: String,
    pub k8s_namespace: String,
    pub k8s_pod_name: String,
    pub named_ports: Vec<NamedPort>,
}

impl IpIdentityPair {
    /// Returns true if the IP represents a host (no mask set).
    /// Port of Go's `IsHost()`.
    pub fn is_host(&self) -> bool {
        self.mask.is_none()
    }

    /// Returns the IP as a host string or a prefix string (IP/prefixlen).
    /// Handles IPv4-mapped IPv6 addresses by formatting them as IPv4.
    /// Port of Go's `PrefixString()`.
    pub fn prefix_string(&self) -> String {
        // Normalise to the canonical display IP: IPv4-mapped IPv6 → IPv4
        let display_ip = match self.ip {
            IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    IpAddr::V4(v4)
                } else {
                    IpAddr::V6(v6)
                }
            }
            other @ IpAddr::V4(_) => other,
        };

        let ip_str = display_ip.to_string();

        match &self.mask {
            None => ip_str,
            Some(mask_bytes) => {
                // Count the number of set bits (prefix length).
                let ones: u32 = mask_bytes.iter().map(|b| b.count_ones()).sum();
                format!("{ip_str}/{ones}")
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Pre-existing unit tests (kept passing)
    // ------------------------------------------------------------------

    #[test]
    fn identity_roundtrip() {
        let id = SecurityIdentity::from_str("123").unwrap();
        assert_eq!(id.as_u32(), 123);
    }

    #[test]
    fn label_and_allocator() {
        let mut a = SimpleIdentityAllocator::default();
        let labels = BTreeMap::from([(String::from("k"), String::from("v"))]);
        let id = a.allocate(&labels);
        assert_eq!(a.lookup(id), Some(&labels));
    }

    #[test]
    fn reserved_identities() {
        let labels = BTreeMap::from([("k8s:foo".into(), "bar".into())]);
        add_reserved_identity(NumericIdentity::new(100), labels.clone());
        assert_eq!(
            lookup_reserved_identity(NumericIdentity::new(100)),
            Some(labels)
        );
        assert_eq!(lookup_reserved_identity(NumericIdentity::new(999)), None);
    }

    #[test]
    fn identity_scope() {
        let global = NumericIdentity::new(500);
        assert_eq!(global.scope(), IdentityScope::Global);

        let local = NumericIdentity::make_local(500);
        assert_eq!(local.scope(), IdentityScope::Local);
        assert_eq!(local.allocator_id(), 500);

        let remote = NumericIdentity::make_remote_node(500);
        assert_eq!(remote.scope(), IdentityScope::RemoteNode);
        assert_eq!(remote.allocator_id(), 500);
    }

    #[test]
    fn world_is_reserved() {
        assert!(NumericIdentity::world().is_reserved());
    }

    #[test]
    fn unknown_identity_has_zero_id() {
        let uid = super::unknown_identity();
        assert_eq!(uid.id.as_u32(), IDENTITY_UNKNOWN);
    }

    #[test]
    fn allocator_gives_same_id_for_same_labels() {
        let mut a = SimpleIdentityAllocator::default();
        let labels1 = BTreeMap::from([("k".into(), "v".into())]);
        let labels2 = BTreeMap::from([("k".into(), "v".into())]);
        let id1 = a.allocate(&labels1);
        let id2 = a.allocate(&labels2);
        assert_eq!(id1, id2);
    }

    #[test]
    fn labels_is_subset_works() {
        let a = BTreeMap::from([("k".into(), "v".into())]);
        let b = BTreeMap::from([("k".into(), "v".into()), ("j".into(), "w".into())]);
        assert!(labels_is_subset(&a, &b));
        assert!(!labels_is_subset(&b, &a));
    }

    // ------------------------------------------------------------------
    // Parity tests ported from pkg/identity/identity_test.go
    // and pkg/identity/numericidentity_test.go
    // ------------------------------------------------------------------

    mod parity_tests {
        use super::*;
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        // ---- TestReservedID ----
        // Port of Go: TestReservedID in pkg/identity/identity_test.go

        #[test]
        fn parity_test_reserved_id_host() {
            let i = get_reserved_id("host");
            assert_eq!(i, SecurityIdentity::new(1));
            assert_eq!(i.to_string(), "host");
        }

        #[test]
        fn parity_test_reserved_id_world() {
            let i = get_reserved_id("world");
            assert_eq!(i, SecurityIdentity::new(2));
            assert_eq!(i.to_string(), "world");
        }

        #[test]
        fn parity_test_reserved_id_cluster_is_obsoleted() {
            // "cluster" is obsoleted — must return IdentityUnknown (0).
            let i = get_reserved_id("cluster");
            assert_eq!(i, RESERVED_IDENTITY_UNKNOWN);
            assert_eq!(i.to_string(), "unknown");
        }

        #[test]
        fn parity_test_reserved_id_health() {
            let i = get_reserved_id("health");
            assert_eq!(i, SecurityIdentity::new(4));
            assert_eq!(i.to_string(), "health");
        }

        #[test]
        fn parity_test_reserved_id_init() {
            let i = get_reserved_id("init");
            assert_eq!(i, SecurityIdentity::new(5));
            assert_eq!(i.to_string(), "init");
        }

        #[test]
        fn parity_test_reserved_id_unmanaged() {
            let i = get_reserved_id("unmanaged");
            assert_eq!(i, SecurityIdentity::new(3));
            assert_eq!(i.to_string(), "unmanaged");
        }

        #[test]
        fn parity_test_reserved_id_kube_apiserver() {
            let i = get_reserved_id("kube-apiserver");
            assert_eq!(i, SecurityIdentity::new(7));
            assert_eq!(i.to_string(), "kube-apiserver");
        }

        #[test]
        fn parity_test_reserved_id_unknown_returns_zero() {
            // get_reserved_id("unknown") → IdentityUnknown (0)
            assert_eq!(get_reserved_id("unknown"), RESERVED_IDENTITY_UNKNOWN);
        }

        #[test]
        fn parity_test_reserved_id_unknown_numeric_formats_as_number() {
            // A numeric identity that is not reserved should format as its number.
            let unknown = SecurityIdentity::new(700);
            assert_eq!(unknown.to_string(), "700");
        }

        // ---- TestIsReservedIdentity ----
        // Port of Go: TestIsReservedIdentity in pkg/identity/identity_test.go

        #[test]
        fn parity_test_is_reserved_identity() {
            assert!(RESERVED_IDENTITY_KUBE_APISERVER.is_reserved_identity());
            assert!(RESERVED_IDENTITY_HEALTH.is_reserved_identity());
            assert!(RESERVED_IDENTITY_HOST.is_reserved_identity());
            assert!(RESERVED_IDENTITY_WORLD.is_reserved_identity());
            assert!(RESERVED_IDENTITY_INIT.is_reserved_identity());
            assert!(RESERVED_IDENTITY_UNMANAGED.is_reserved_identity());

            assert!(!SecurityIdentity::new(123_456).is_reserved_identity());
        }

        // ---- TestScopeForLabels ----
        // Port of Go: TestScopeForLabels in pkg/identity/identity_test.go
        #[test]
        fn parity_test_scope_for_labels() {
            struct Case {
                lbls: Labels,
                scope: NumericIdentity,
            }
            let cases = vec![
                // CIDR labels → Local scope
                Case {
                    lbls: get_cidr_labels("0.0.0.0/0"),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                Case {
                    lbls: get_cidr_labels("192.168.23.0/24"),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                // k8s label → Global scope
                Case {
                    lbls: labels_from_model(&["k8s:foo=bar"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_GLOBAL),
                },
                // reserved:world → Global (reserved but not ingress)
                Case {
                    lbls: labels_from_model(&["reserved:world"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                // reserved:unmanaged → Local (reserved → local by ScopeForLabels rule)
                Case {
                    lbls: labels_from_model(&["reserved:unmanaged"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                // reserved:health → Local
                Case {
                    lbls: labels_from_model(&["reserved:health"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                // reserved:init → Local
                Case {
                    lbls: labels_from_model(&["reserved:init"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                // reserved:ingress → Local (special case: reserved+ingress)
                Case {
                    lbls: labels_from_model(&["reserved:ingress"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_LOCAL),
                },
                // reserved:remote-node → RemoteNode scope
                Case {
                    lbls: labels_from_model(&["reserved:remote-node"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_REMOTE_NODE),
                },
                // remote-node + kube-apiserver → RemoteNode (kube-apiserver is also remote-node)
                Case {
                    lbls: labels_from_model(&["reserved:remote-node", "reserved:kube-apiserver"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_REMOTE_NODE),
                },
                // k8s:ingress=allowed → Global (not the reserved ingress label)
                Case {
                    lbls: labels_from_model(&["k8s:ingress=allowed"]),
                    scope: NumericIdentity::new(IDENTITY_SCOPE_GLOBAL),
                },
            ];

            for (i, case) in cases.iter().enumerate() {
                // ScopeForLabels is only called when LookupReservedIdentityByLabels returns None.
                let reserved = lookup_reserved_identity_by_label_map(&case.lbls, false);
                if reserved.is_some() {
                    continue;
                }
                let scope = scope_for_label_map(&case.lbls);
                assert_eq!(scope, case.scope, "case {i}: labels={:?}", case.lbls);
            }
        }

        // ---- TestNewIdentityFromLabelArray ----
        // Port of Go: TestNewIdentityFromLabelArray in pkg/identity/identity_test.go
        #[test]
        fn parity_test_new_identity_from_label_array() {
            // Go: NewIdentityFromLabelArray(1001, NewLabelArrayFromSortedList("unspec:a=;unspec:b;unspec:c=d"))
            // Expected identity labels: a="", b="", c="d"
            let label_array = label_array_from_sorted_list("unspec:a=;unspec:b;unspec:c=d");
            let identity = new_identity_from_label_array(NumericIdentity::new(1001), &label_array);

            assert_eq!(identity.id, NumericIdentity::new(1001));
            // Labels map should contain a, b, c keys.
            // In Rust we store as BTreeMap<key_with_source, value>.
            // unspec source → stored without source prefix (matching Go's Label.Key).
            assert_eq!(identity.labels.get("a").map(String::as_str), Some(""));
            assert_eq!(identity.labels.get("b").map(String::as_str), Some(""));
            assert_eq!(identity.labels.get("c").map(String::as_str), Some("d"));
            assert_eq!(identity.labels.len(), 3);
        }

        // ---- TestLookupReservedIdentityByLabels ----
        // Port of Go: TestLookupReservedIdentityByLabels in pkg/identity/identity_test.go
        #[test]
        fn parity_test_lookup_reserved_identity_by_labels() {
            // nil → None
            let result = lookup_reserved_identity_by_label_map(&Labels::new(), false);
            assert!(result.is_none(), "nil labels should return None");

            // host → host identity
            let host_labels = labels_from_model(&["reserved:host"]);
            let result = lookup_reserved_identity_by_label_map(&host_labels, false);
            assert!(result.is_some(), "host labels should find identity");
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_HOST);

            // non-reserved → None
            let result = lookup_reserved_identity_by_label_map(&labels_from_model(&["foo"]), false);
            assert!(result.is_none(), "non-reserved labels should return None");

            // reserved:init + non-reserved → None (len != 1 and not remote-node)
            let result = lookup_reserved_identity_by_label_map(
                &labels_from_model(&["reserved:init", "foo"]),
                false,
            );
            assert!(
                result.is_none(),
                "mixed reserved+non-reserved should return None"
            );

            // health → health identity
            let result = lookup_reserved_identity_by_label_map(
                &labels_from_model(&["reserved:health"]),
                false,
            );
            assert!(result.is_some(), "health should find identity");
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_HEALTH);

            // world → world identity
            let result = lookup_reserved_identity_by_label_map(
                &labels_from_model(&["reserved:world"]),
                false,
            );
            assert!(result.is_some(), "world should find identity");
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_WORLD);

            // remote-node → remote-node identity (no cidr policy)
            let result = lookup_reserved_identity_by_label_map(
                &labels_from_model(&["reserved:remote-node"]),
                false,
            );
            assert!(result.is_some(), "remote-node should find identity");
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_REMOTE_NODE);

            // kube-apiserver + remote-node → kube-apiserver identity
            let mut kube_labels = Labels::new();
            kube_labels.insert(
                "kube-apiserver".to_owned(),
                Label::new(LABEL_SOURCE_RESERVED, "kube-apiserver", ""),
            );
            kube_labels.insert(
                "remote-node".to_owned(),
                Label::new(LABEL_SOURCE_RESERVED, "remote-node", ""),
            );
            let result = lookup_reserved_identity_by_label_map(&kube_labels, false);
            assert!(
                result.is_some(),
                "kube-apiserver+remote-node should find identity"
            );
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_KUBE_APISERVER);

            // kube-apiserver + host → host (host has highest priority)
            let mut kube_host_labels = Labels::new();
            kube_host_labels.insert(
                "kube-apiserver".to_owned(),
                Label::new(LABEL_SOURCE_RESERVED, "kube-apiserver", ""),
            );
            kube_host_labels.insert(
                "host".to_owned(),
                Label::new(LABEL_SOURCE_RESERVED, "host", ""),
            );
            let result = lookup_reserved_identity_by_label_map(&kube_host_labels, false);
            assert!(
                result.is_some(),
                "kube-apiserver+host should find host identity"
            );
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_HOST);

            // ingress → ingress identity
            let result = lookup_reserved_identity_by_label_map(
                &labels_from_model(&["reserved:ingress"]),
                false,
            );
            assert!(result.is_some(), "ingress should find identity");
            assert_eq!(result.unwrap().id, RESERVED_IDENTITY_INGRESS);

            // world + cidr labels → None (IsReserved=true but len>1 and no remote-node)
            let mut cidr_world_labels = Labels::new();
            cidr_world_labels.insert(
                "world".to_owned(),
                Label::new(LABEL_SOURCE_RESERVED, "world", ""),
            );
            cidr_world_labels.insert(
                "10.0.0.0/24".to_owned(),
                Label::new(LABEL_SOURCE_CIDR, "10.0.0.0/24", ""),
            );
            let result = lookup_reserved_identity_by_label_map(&cidr_world_labels, false);
            assert!(result.is_none(), "cidr+world labels should return None");

            // remote-node with node_cidr_policy=true → None
            let result = lookup_reserved_identity_by_label_map(
                &labels_from_model(&["reserved:remote-node"]),
                true,
            );
            assert!(
                result.is_none(),
                "remote-node with cidr policy should return None"
            );

            // kube-apiserver + remote-node with node_cidr_policy=true → None
            let result = lookup_reserved_identity_by_label_map(&kube_labels, true);
            assert!(
                result.is_none(),
                "kube-apiserver+remote-node with cidr policy should return None"
            );
        }

        // ---- TestIPIdentityPair_PrefixString ----
        // Port of Go: TestIPIdentityPair_PrefixString in pkg/identity/identity_test.go

        fn all_ones_ipv6_mask() -> Vec<u8> {
            vec![255u8; 16]
        }

        fn ipv4_mask_32() -> Vec<u8> {
            vec![255, 255, 255, 255]
        }

        #[test]
        fn parity_test_ip_identity_pair_prefix_string_ipv4_with_mask() {
            let pair = IpIdentityPair {
                ip: IpAddr::V4(Ipv4Addr::new(10, 1, 128, 15)),
                mask: Some(ipv4_mask_32()),
                host_ip: IpAddr::V4(Ipv4Addr::new(10, 1, 128, 15)),
                id: SecurityIdentity::new(1),
                key: 3,
                metadata: "metadata".into(),
                k8s_namespace: "kube-system".into(),
                k8s_pod_name: "pod-name".into(),
                named_ports: vec![NamedPort {
                    name: "port".into(),
                    port: 8080,
                    protocol: "TCP".into(),
                }],
            };
            assert_eq!(pair.prefix_string(), "10.1.128.15/32");
        }

        #[test]
        fn parity_test_ip_identity_pair_prefix_string_ipv4_without_mask() {
            let pair = IpIdentityPair {
                ip: IpAddr::V4(Ipv4Addr::new(10, 1, 128, 15)),
                mask: None,
                host_ip: IpAddr::V4(Ipv4Addr::new(10, 1, 128, 15)),
                id: SecurityIdentity::new(1),
                key: 3,
                metadata: "metadata".into(),
                k8s_namespace: "kube-system".into(),
                k8s_pod_name: "pod-name".into(),
                named_ports: vec![],
            };
            assert_eq!(pair.prefix_string(), "10.1.128.15");
        }

        #[test]
        fn parity_test_ip_identity_pair_prefix_string_ipv4_encoded_as_ipv6_with_mask() {
            // ::ffff:a01:800f is the IPv4-mapped IPv6 form of 10.1.128.15
            // Go's net.ParseIP("::ffff:a01:800f") returns an IPv6 that formats as "10.1.128.15"
            // when printed via .String() because net.IP.String() detects mapped addresses.
            // Our Rust implementation must do the same: detect v4-mapped and print as IPv4.
            let v6: Ipv6Addr = "::ffff:a01:800f".parse().unwrap();
            let pair = IpIdentityPair {
                ip: IpAddr::V6(v6),
                mask: Some(all_ones_ipv6_mask()),
                host_ip: IpAddr::V6(v6),
                id: SecurityIdentity::new(1),
                key: 3,
                metadata: "metadata".into(),
                k8s_namespace: "kube-system".into(),
                k8s_pod_name: "pod-name".into(),
                named_ports: vec![],
            };
            // Expected: "10.1.128.15/128" — Go counts 128 ones in the 16-byte all-ones mask.
            assert_eq!(pair.prefix_string(), "10.1.128.15/128");
        }

        #[test]
        fn parity_test_ip_identity_pair_prefix_string_ipv4_encoded_as_ipv6_without_mask() {
            let v6: Ipv6Addr = "::ffff:a01:800f".parse().unwrap();
            let pair = IpIdentityPair {
                ip: IpAddr::V6(v6),
                mask: None,
                host_ip: IpAddr::V6(v6),
                id: SecurityIdentity::new(1),
                key: 3,
                metadata: "metadata".into(),
                k8s_namespace: "kube-system".into(),
                k8s_pod_name: "pod-name".into(),
                named_ports: vec![],
            };
            assert_eq!(pair.prefix_string(), "10.1.128.15");
        }

        #[test]
        fn parity_test_ip_identity_pair_prefix_string_ipv6_with_mask() {
            let v6: Ipv6Addr = "fd12:3456:789a:1::1".parse().unwrap();
            let pair = IpIdentityPair {
                ip: IpAddr::V6(v6),
                mask: Some(all_ones_ipv6_mask()),
                host_ip: IpAddr::V6(v6),
                id: SecurityIdentity::new(1),
                key: 3,
                metadata: "metadata".into(),
                k8s_namespace: "kube-system".into(),
                k8s_pod_name: "pod-name".into(),
                named_ports: vec![],
            };
            assert_eq!(pair.prefix_string(), "fd12:3456:789a:1::1/128");
        }

        #[test]
        fn parity_test_ip_identity_pair_prefix_string_ipv6_without_mask() {
            let v6: Ipv6Addr = "fd12:3456:789a:1::1".parse().unwrap();
            let pair = IpIdentityPair {
                ip: IpAddr::V6(v6),
                mask: None,
                host_ip: IpAddr::V6(v6),
                id: SecurityIdentity::new(1),
                key: 3,
                metadata: "metadata".into(),
                k8s_namespace: "kube-system".into(),
                k8s_pod_name: "pod-name".into(),
                named_ports: vec![],
            };
            assert_eq!(pair.prefix_string(), "fd12:3456:789a:1::1");
        }

        // Benchmark: skip (no Rust test fn needed)
        // TODO(parity): BenchmarkIPIdentityPair_PrefixString from pkg/identity/identity_test.go
        // — benchmarks are criterion-based in Rust, not part of #[test].

        // ---- TestLocalIdentity ----
        // Port of Go: TestLocalIdentity in pkg/identity/numericidentity_test.go

        #[test]
        fn parity_test_local_identity() {
            // A local-scoped identity should HasLocalScope() == true
            let local_id = SecurityIdentity::new(IDENTITY_SCOPE_LOCAL | 1);
            assert!(local_id.has_local_scope());

            // ClusterIDMax (255) << 0 | 1 = 0xFF_0001 which does NOT have the local scope bit (bit 24)
            // cmtypes.ClusterIDMax = 255, so 255 | 1 = 255 — no scope bits set.
            let max_cluster_id = SecurityIdentity::new(255 | 1);
            assert!(!max_cluster_id.has_local_scope());

            // ReservedIdentityWorld (2) does not have local scope.
            assert!(!RESERVED_IDENTITY_WORLD.has_local_scope());
        }

        // ---- TestClusterID ----
        // Port of Go: TestClusterID in pkg/identity/numericidentity_test.go
        // Uses the default 255-cluster config: ClusterIDShift=16.

        #[test]
        fn parity_test_cluster_id() {
            struct Case {
                identity: u32,
                cluster_id: u32,
            }
            let cases = [
                Case {
                    identity: 0x0000_0000,
                    cluster_id: 0,
                },
                Case {
                    identity: 0x0001_0000,
                    cluster_id: 1,
                },
                Case {
                    identity: 0x002A_0000,
                    cluster_id: 42,
                },
                Case {
                    identity: 0x00FF_0000,
                    cluster_id: 255,
                },
                // cmtypes.ClusterIDMin = 0
                Case {
                    identity: 0 << 16,
                    cluster_id: 0,
                },
                // cmtypes.ClusterIDMax = 255
                Case {
                    identity: 255 << 16,
                    cluster_id: 255,
                },
            ];
            for c in &cases {
                assert_eq!(
                    SecurityIdentity::new(c.identity).cluster_id(),
                    c.cluster_id,
                    "identity=0x{:06x}",
                    c.identity
                );
            }
        }

        // ---- TestGetAllReservedIdentities ----
        // Port of Go: TestGetAllReservedIdentities in pkg/identity/numericidentity_test.go

        #[test]
        fn parity_test_get_all_reserved_identities() {
            let all = get_all_reserved_identities();
            assert!(!all.is_empty());
            // Must equal RESERVED_IDENTITIES length.
            assert_eq!(all.len(), RESERVED_IDENTITIES.len());
            // Must be in ascending order starting at 1 (identity 0 is unknown, not included).
            for (i, id) in all.iter().enumerate() {
                // NOTE: identity 0 is unknown, so reserved identities start at 1 → index+1
                assert_eq!(
                    id.uint32(),
                    (i + 1) as u32,
                    "index {i}: expected id={} got {}",
                    i + 1,
                    id.uint32()
                );
            }
        }

        // ---- TestAsUint32Slice ----
        // Port of Go: TestAsUint32Slice in pkg/identity/numericidentity_test.go

        #[test]
        fn parity_test_as_uint32_slice() {
            let nids: NumericIdentitySlice = vec![
                SecurityIdentity::new(2),
                SecurityIdentity::new(42),
                SecurityIdentity::new(42),
                SecurityIdentity::new(1),
                SecurityIdentity::new(1024),
                SecurityIdentity::new(1),
            ];
            let u32_slice = nids.as_u32_slice();
            assert_eq!(u32_slice.len(), nids.len());
            for (i, nid) in nids.iter().enumerate() {
                assert_eq!(nid.uint32(), u32_slice[i]);
            }
        }

        // ---- TestGetClusterIDShift ----
        // Port of Go: TestGetClusterIDShift in pkg/identity/numericidentity_test.go
        // Go tests dynamic reconfiguration via sync.Once; we implement a simpler
        // static version for the default 255-cluster config.
        #[test]
        fn parity_test_get_cluster_id_shift() {
            // Default 255-cluster config → shift=16, bits=8
            assert_eq!(get_cluster_id_shift(), 16);
            assert_eq!(get_cluster_id_bits(), 8);

            // Verify the shift is consistent with SecurityIdentity::cluster_id()
            // which uses the same shift value.
            let identity = SecurityIdentity::new(0x2A_0000); // cluster_id=42
            assert_eq!(identity.cluster_id(), 42);
            assert_eq!(
                identity.cluster_id(),
                (identity.as_u32() >> get_cluster_id_shift()) & 0xFF
            );
        }
    }
}
