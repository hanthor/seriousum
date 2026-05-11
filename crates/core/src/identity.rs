//! Identity and security types for seriousum.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

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
    pub const fn is_reserved(self) -> bool {
        self.0 < 1024
    }
    pub const fn world() -> Self {
        Self(4)
    }
    pub const fn host() -> Self {
        Self(1)
    }
    pub const fn cluster() -> Self {
        Self(2)
    }
    pub const fn unmanaged() -> Self {
        Self(3)
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
            1 => "host",
            2 => "cluster",
            3 => "unmanaged",
            4 => "world",
            _ => return write!(f, "{}", self.0),
        };
        f.write_str(s)
    }
}

impl FromStr for SecurityIdentity {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "host" => Ok(Self::host()),
            "cluster" => Ok(Self::cluster()),
            "unmanaged" => Ok(Self::unmanaged()),
            "world" => Ok(Self::world()),
            _ => s
                .parse::<u32>()
                .map(Self::new)
                .map_err(|e| anyhow::anyhow!("invalid identity: {e}")),
        }
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

pub trait IdentityAllocator: Send + Sync {
    fn allocate(&mut self, labels: &BTreeMap<String, String>) -> u32;
    fn release(&mut self, id: u32);
    fn lookup(&self, id: u32) -> Option<&BTreeMap<String, String>>;
    fn lookup_by_labels(&self, labels: &BTreeMap<String, String>) -> Option<u32>;
}

pub struct SimpleIdentityAllocator {
    next_id: u32,
    id_to_labels: BTreeMap<u32, BTreeMap<String, String>>,
    labels_to_id: BTreeMap<BTreeMap<String, String>, u32>,
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
        Self::new(1024)
    }
}

impl IdentityAllocator for SimpleIdentityAllocator {
    fn allocate(&mut self, labels: &BTreeMap<String, String>) -> u32 {
        if let Some(id) = self.labels_to_id.get(labels).copied() {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.id_to_labels.insert(id, labels.clone());
        self.labels_to_id.insert(labels.clone(), id);
        id
    }
    fn release(&mut self, id: u32) {
        if let Some(labels) = self.id_to_labels.remove(&id) {
            self.labels_to_id.remove(&labels);
        }
    }
    fn lookup(&self, id: u32) -> Option<&BTreeMap<String, String>> {
        self.id_to_labels.get(&id)
    }
    fn lookup_by_labels(&self, labels: &BTreeMap<String, String>) -> Option<u32> {
        self.labels_to_id.get(labels).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
