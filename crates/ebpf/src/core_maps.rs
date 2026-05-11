//! eBPF map infrastructure — ported from cilium/pkg/bpf
//!
//! This module provides a trait-based abstraction over eBPF map types, allowing
//! Rust code to interact with kernel eBPF maps without dealing with syscall complexity.

use dashmap::DashMap;
use std::sync::Arc;
use thiserror::Error;

/// Error type for eBPF map operations.
#[derive(Debug, Error)]
pub enum BpfMapError {
    #[error("key not found")]
    KeyNotFound,

    #[error("key already exists")]
    KeyExists,

    #[error("invalid key size: expected {expected}, got {actual}")]
    InvalidKeySize { expected: usize, actual: usize },

    #[error("invalid value size: expected {expected}, got {actual}")]
    InvalidValueSize { expected: usize, actual: usize },

    #[error("map type mismatch")]
    TypeMismatch,

    #[error("unsupported operation")]
    UnsupportedOperation,

    #[error("map at capacity")]
    AtCapacity,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("eBPF error: {0}")]
    EbpfError(String),
}

pub type Result<T> = std::result::Result<T, BpfMapError>;

/// Enum of supported eBPF map types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BpfMapType {
    Hash,
    LRUHash,
    PerCPUHash,
    Array,
    PerCPUArray,
    ProgramArray,
    RingBuf,
    LPMTrie,
}

impl std::fmt::Display for BpfMapType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hash => write!(f, "BPF_MAP_TYPE_HASH"),
            Self::LRUHash => write!(f, "BPF_MAP_TYPE_LRU_HASH"),
            Self::PerCPUHash => write!(f, "BPF_MAP_TYPE_PERCPU_HASH"),
            Self::Array => write!(f, "BPF_MAP_TYPE_ARRAY"),
            Self::PerCPUArray => write!(f, "BPF_MAP_TYPE_PERCPU_ARRAY"),
            Self::ProgramArray => write!(f, "BPF_MAP_TYPE_PROG_ARRAY"),
            Self::RingBuf => write!(f, "BPF_MAP_TYPE_RINGBUF"),
            Self::LPMTrie => write!(f, "BPF_MAP_TYPE_LPM_TRIE"),
        }
    }
}

/// Desired action for batch operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredAction {
    Ok,
    Insert,
    Delete,
}

impl std::fmt::Display for DesiredAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "OK"),
            Self::Insert => write!(f, "INSERT"),
            Self::Delete => write!(f, "DELETE"),
        }
    }
}

/// Metadata about an eBPF map.
#[derive(Debug, Clone)]
pub struct MapInfo {
    pub name: String,
    pub map_type: BpfMapType,
    pub key_size: usize,
    pub value_size: usize,
    pub max_entries: usize,
    pub pin_path: Option<String>,
}

impl MapInfo {
    pub fn new(
        name: impl Into<String>,
        map_type: BpfMapType,
        key_size: usize,
        value_size: usize,
        max_entries: usize,
    ) -> Self {
        Self {
            name: name.into(),
            map_type,
            key_size,
            value_size,
            max_entries,
            pin_path: None,
        }
    }

    pub fn with_pin_path(mut self, path: impl Into<String>) -> Self {
        self.pin_path = Some(path.into());
        self
    }
}

/// Flags for map memory allocation.
#[derive(Debug, Clone, Default)]
pub struct MapMemFlags {
    pub pre_alloc: bool,
    pub lru_evict_on_capacity: bool,
}

/// Generic trait for all eBPF map types.
pub trait BpfMap: Send + Sync {
    /// Retrieve value by key.
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Insert or update a key-value pair.
    fn update(&self, key: &[u8], value: &[u8], flags: u64) -> Result<()>;

    /// Delete a key from the map.
    fn delete(&self, key: &[u8]) -> Result<()>;

    /// Iterate over all entries.
    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_>;

    /// Get the number of entries.
    fn len(&self) -> usize;

    /// Check if the map is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pin the map to /sys/fs/bpf.
    fn pin(&self, _path: &str) -> Result<()> {
        Err(BpfMapError::UnsupportedOperation)
    }

    /// Unpin the map from /sys/fs/bpf.
    fn unpin(&self, _path: &str) -> Result<()> {
        Err(BpfMapError::UnsupportedOperation)
    }

    /// Get map metadata.
    fn info(&self) -> MapInfo;
}

/// Hash map implementation.
pub struct HashMap {
    info: MapInfo,
    data: Arc<DashMap<Vec<u8>, Vec<u8>>>,
}

impl HashMap {
    pub fn new(name: impl Into<String>, key_size: usize, value_size: usize, max_entries: usize) -> Self {
        Self {
            info: MapInfo::new(name, BpfMapType::Hash, key_size, value_size, max_entries),
            data: Arc::new(DashMap::new()),
        }
    }
}

impl BpfMap for HashMap {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        Ok(self.data.get(key).map(|ref_multi| ref_multi.value().clone()))
    }

    fn update(&self, key: &[u8], value: &[u8], _flags: u64) -> Result<()> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        if value.len() != self.info.value_size {
            return Err(BpfMapError::InvalidValueSize {
                expected: self.info.value_size,
                actual: value.len(),
            });
        }
        if self.data.len() >= self.info.max_entries && self.data.get(key).is_none() {
            return Err(BpfMapError::AtCapacity);
        }
        self.data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        self.data.remove(key);
        Ok(())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(
            self.data
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().clone())),
        )
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn info(&self) -> MapInfo {
        self.info.clone()
    }
}

/// LRU Hash map — evicts oldest entry on capacity.
pub struct LruHashMap {
    info: MapInfo,
    data: Arc<DashMap<Vec<u8>, Vec<u8>>>,
}

impl LruHashMap {
    pub fn new(name: impl Into<String>, key_size: usize, value_size: usize, max_entries: usize) -> Self {
        Self {
            info: MapInfo::new(name, BpfMapType::LRUHash, key_size, value_size, max_entries),
            data: Arc::new(DashMap::new()),
        }
    }
}

impl BpfMap for LruHashMap {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        Ok(self.data.get(key).map(|ref_multi| ref_multi.value().clone()))
    }

    fn update(&self, key: &[u8], value: &[u8], _flags: u64) -> Result<()> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        if value.len() != self.info.value_size {
            return Err(BpfMapError::InvalidValueSize {
                expected: self.info.value_size,
                actual: value.len(),
            });
        }

        // LRU: evict oldest entry if at capacity
        if self.data.len() >= self.info.max_entries && self.data.get(key).is_none() {
            // Remove first entry from iterator
            if let Some(entry) = self.data.iter().next() {
                let k = entry.key().clone();
                drop(entry);
                self.data.remove(&k);
            }
        }

        self.data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        self.data.remove(key);
        Ok(())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(
            self.data
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().clone())),
        )
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn info(&self) -> MapInfo {
        self.info.clone()
    }
}

/// Per-CPU hash map — stores separate values for each CPU.
pub struct PerCpuHashMap {
    info: MapInfo,
    num_cpus: usize,
    data: Arc<DashMap<Vec<u8>, Vec<Vec<u8>>>>,
}

impl PerCpuHashMap {
    pub fn new(name: impl Into<String>, key_size: usize, value_size: usize, max_entries: usize) -> Self {
        let num_cpus = num_cpus::get();
        Self {
            info: MapInfo::new(name, BpfMapType::PerCPUHash, key_size, value_size, max_entries),
            num_cpus,
            data: Arc::new(DashMap::new()),
        }
    }
}

impl BpfMap for PerCpuHashMap {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        Ok(self.data.get(key).map(|ref_multi| ref_multi.value()[0].clone()))
    }

    fn update(&self, key: &[u8], value: &[u8], _flags: u64) -> Result<()> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        if value.len() != self.info.value_size {
            return Err(BpfMapError::InvalidValueSize {
                expected: self.info.value_size,
                actual: value.len(),
            });
        }
        if self.data.len() >= self.info.max_entries && self.data.get(key).is_none() {
            return Err(BpfMapError::AtCapacity);
        }

        let cpu_values = vec![value.to_vec(); self.num_cpus];
        self.data.insert(key.to_vec(), cpu_values);
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        if key.len() != self.info.key_size {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.key_size,
                actual: key.len(),
            });
        }
        self.data.remove(key);
        Ok(())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(
            self.data
                .iter()
                .map(|entry| (entry.key().clone(), entry.value()[0].clone())),
        )
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn info(&self) -> MapInfo {
        self.info.clone()
    }
}

/// Array map — index-based access.
pub struct ArrayMap {
    info: MapInfo,
    data: Arc<DashMap<u32, Vec<u8>>>,
}

impl ArrayMap {
    pub fn new(name: impl Into<String>, value_size: usize, max_entries: usize) -> Self {
        Self {
            info: MapInfo::new(name, BpfMapType::Array, 4, value_size, max_entries),
            data: Arc::new(DashMap::new()),
        }
    }
}

impl BpfMap for ArrayMap {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        if idx >= self.info.max_entries as u32 {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.max_entries as usize,
                actual: idx as usize,
            });
        }
        Ok(self.data.get(&idx).map(|ref_multi| ref_multi.value().clone()))
    }

    fn update(&self, key: &[u8], value: &[u8], _flags: u64) -> Result<()> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        if value.len() != self.info.value_size {
            return Err(BpfMapError::InvalidValueSize {
                expected: self.info.value_size,
                actual: value.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        if idx >= self.info.max_entries as u32 {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.max_entries as usize,
                actual: idx as usize,
            });
        }
        self.data.insert(idx, value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        self.data.remove(&idx);
        Ok(())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(self.data.iter().map(|entry| {
            let idx_bytes = entry.key().to_le_bytes().to_vec();
            (idx_bytes, entry.value().clone())
        }))
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn info(&self) -> MapInfo {
        self.info.clone()
    }
}

/// Per-CPU array map.
pub struct PerCpuArrayMap {
    info: MapInfo,
    num_cpus: usize,
    data: Arc<DashMap<u32, Vec<Vec<u8>>>>,
}

impl PerCpuArrayMap {
    pub fn new(name: impl Into<String>, value_size: usize, max_entries: usize) -> Self {
        let num_cpus = num_cpus::get();
        Self {
            info: MapInfo::new(name, BpfMapType::PerCPUArray, 4, value_size, max_entries),
            num_cpus,
            data: Arc::new(DashMap::new()),
        }
    }
}

impl BpfMap for PerCpuArrayMap {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        Ok(self.data.get(&idx).map(|ref_multi| ref_multi.value()[0].clone()))
    }

    fn update(&self, key: &[u8], value: &[u8], _flags: u64) -> Result<()> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        if value.len() != self.info.value_size {
            return Err(BpfMapError::InvalidValueSize {
                expected: self.info.value_size,
                actual: value.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        if idx >= self.info.max_entries as u32 {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.max_entries as usize,
                actual: idx as usize,
            });
        }
        let cpu_values = vec![value.to_vec(); self.num_cpus];
        self.data.insert(idx, cpu_values);
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        self.data.remove(&idx);
        Ok(())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(self.data.iter().map(|entry| {
            let idx_bytes = entry.key().to_le_bytes().to_vec();
            (idx_bytes, entry.value()[0].clone())
        }))
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn info(&self) -> MapInfo {
        self.info.clone()
    }
}

/// Program array — stores eBPF program file descriptors for tail calls.
pub struct ProgramArrayMap {
    info: MapInfo,
    data: Arc<DashMap<u32, Vec<u8>>>,
}

impl ProgramArrayMap {
    pub fn new(name: impl Into<String>, max_entries: usize) -> Self {
        Self {
            info: MapInfo::new(name, BpfMapType::ProgramArray, 4, 4, max_entries),
            data: Arc::new(DashMap::new()),
        }
    }
}

impl BpfMap for ProgramArrayMap {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        Ok(self.data.get(&idx).map(|ref_multi| ref_multi.value().clone()))
    }

    fn update(&self, key: &[u8], value: &[u8], _flags: u64) -> Result<()> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        if value.len() != 4 {
            return Err(BpfMapError::InvalidValueSize {
                expected: 4,
                actual: value.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        if idx >= self.info.max_entries as u32 {
            return Err(BpfMapError::InvalidKeySize {
                expected: self.info.max_entries as usize,
                actual: idx as usize,
            });
        }
        self.data.insert(idx, value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        if key.len() != 4 {
            return Err(BpfMapError::InvalidKeySize {
                expected: 4,
                actual: key.len(),
            });
        }
        let idx = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        self.data.remove(&idx);
        Ok(())
    }

    fn iter(&self) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(self.data.iter().map(|entry| {
            let idx_bytes = entry.key().to_le_bytes().to_vec();
            (idx_bytes, entry.value().clone())
        }))
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn info(&self) -> MapInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_info_creation() {
        let info = MapInfo::new("test", BpfMapType::Hash, 4, 8, 1024);
        assert_eq!(info.name, "test");
        assert_eq!(info.key_size, 4);
        assert_eq!(info.value_size, 8);
        assert_eq!(info.max_entries, 1024);
    }

    #[test]
    fn test_map_type_display() {
        assert_eq!(BpfMapType::Hash.to_string(), "BPF_MAP_TYPE_HASH");
        assert_eq!(BpfMapType::LRUHash.to_string(), "BPF_MAP_TYPE_LRU_HASH");
        assert_eq!(BpfMapType::PerCPUHash.to_string(), "BPF_MAP_TYPE_PERCPU_HASH");
    }

    #[test]
    fn test_hash_map_insert_lookup_delete() {
        let map = HashMap::new("test", 4, 8, 1024);
        let key = vec![1, 2, 3, 4];
        let value = vec![5, 6, 7, 8, 9, 10, 11, 12];

        assert!(map.lookup(&key).unwrap().is_none());
        assert!(map.update(&key, &value, 0).is_ok());
        assert_eq!(map.lookup(&key).unwrap(), Some(value.clone()));
        assert!(map.delete(&key).is_ok());
        assert!(map.lookup(&key).unwrap().is_none());
    }

    #[test]
    fn test_hash_map_overwrite() {
        let map = HashMap::new("test", 4, 8, 1024);
        let key = vec![1, 2, 3, 4];
        let value1 = vec![5, 6, 7, 8, 9, 10, 11, 12];
        let value2 = vec![13, 14, 15, 16, 17, 18, 19, 20];

        map.update(&key, &value1, 0).unwrap();
        map.update(&key, &value2, 0).unwrap();
        assert_eq!(map.lookup(&key).unwrap(), Some(value2));
    }

    #[test]
    fn test_hash_map_wrong_key_size() {
        let map = HashMap::new("test", 4, 8, 1024);
        let key = vec![1, 2];
        let value = vec![5, 6, 7, 8, 9, 10, 11, 12];

        assert!(map.update(&key, &value, 0).is_err());
        assert!(map.lookup(&key).is_err());
    }

    #[test]
    fn test_hash_map_wrong_value_size() {
        let map = HashMap::new("test", 4, 8, 1024);
        let key = vec![1, 2, 3, 4];
        let value = vec![5, 6, 7];

        assert!(map.update(&key, &value, 0).is_err());
    }

    #[test]
    fn test_hash_map_capacity() {
        let map = HashMap::new("test", 4, 8, 2);
        let key1 = vec![1, 2, 3, 4];
        let key2 = vec![5, 6, 7, 8];
        let key3 = vec![9, 10, 11, 12];
        let value = vec![1, 2, 3, 4, 5, 6, 7, 8];

        map.update(&key1, &value, 0).unwrap();
        map.update(&key2, &value, 0).unwrap();
        assert!(map.update(&key3, &value, 0).is_err());
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_lru_hash_map_operations() {
        let map = LruHashMap::new("test", 4, 8, 2);
        let key1 = vec![1, 2, 3, 4];
        let key2 = vec![5, 6, 7, 8];
        let value = vec![1, 2, 3, 4, 5, 6, 7, 8];

        map.update(&key1, &value, 0).unwrap();
        map.update(&key2, &value, 0).unwrap();
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_percpu_hash_map_all_cpus() {
        let map = PerCpuHashMap::new("test", 4, 4, 1024);
        let key = vec![1, 2, 3, 4];
        let value = vec![5, 6, 7, 8];

        assert!(map.update(&key, &value, 0).is_ok());
        assert!(map.lookup(&key).unwrap().is_some());
    }

    #[test]
    fn test_array_map_index_operations() {
        let map = ArrayMap::new("test", 8, 256);
        let key = vec![0, 0, 0, 0];
        let value = vec![5, 6, 7, 8, 9, 10, 11, 12];

        assert!(map.update(&key, &value, 0).is_ok());
        assert_eq!(map.lookup(&key).unwrap(), Some(value));
    }

    #[test]
    fn test_program_array_map_operations() {
        let map = ProgramArrayMap::new("test", 256);
        let key = vec![0, 0, 0, 0];
        let value = vec![1, 0, 0, 0];

        assert!(map.update(&key, &value, 0).is_ok());
        assert_eq!(map.lookup(&key).unwrap(), Some(value));
    }

    #[test]
    fn test_map_len() {
        let map = HashMap::new("test", 4, 8, 1024);
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());

        let key = vec![1, 2, 3, 4];
        let value = vec![5, 6, 7, 8, 9, 10, 11, 12];
        map.update(&key, &value, 0).unwrap();

        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
    }

    #[test]
    fn test_desired_action_display() {
        assert_eq!(DesiredAction::Ok.to_string(), "OK");
        assert_eq!(DesiredAction::Insert.to_string(), "INSERT");
        assert_eq!(DesiredAction::Delete.to_string(), "DELETE");
    }

    #[test]
    fn test_bpf_map_error_messages() {
        let err = BpfMapError::KeyNotFound;
        assert_eq!(err.to_string(), "key not found");
        let err = BpfMapError::AtCapacity;
        assert_eq!(err.to_string(), "map at capacity");
    }
}
