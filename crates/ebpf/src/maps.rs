//! Service-specific eBPF maps for load balancing
//!
//! This module implements the eBPF maps required for service load balancing:
//! - SVC_MAP: Service metadata and configuration
//! - BACKEND_MAP: Available backends for services
//! - AFFINITY_MAP: Client-to-backend session affinity
//! - COUNTERS_MAP: Monitoring and statistics

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum number of services
pub const MAX_SERVICES: usize = 65536;

/// Maximum number of backends
pub const MAX_BACKENDS: usize = 262_144;

/// Maximum affinity entries
pub const MAX_AFFINITY: usize = 1_048_576;

/// Map name for services
pub const SVC_MAP_NAME: &str = "cilium_svc_map";

/// Map name for backends
pub const BACKEND_MAP_NAME: &str = "cilium_backend_map";

/// Map name for affinity tracking
pub const AFFINITY_MAP_NAME: &str = "cilium_affinity_map";

/// Map name for counters
pub const COUNTERS_MAP_NAME: &str = "cilium_counters_map";

// ============================================================================
// Data Structures
// ============================================================================

/// Service map entry (stored in SVC_MAP)
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ServiceMapEntry {
    /// Virtual IP (cluster IP) as u32
    pub vip: u32,
    /// Service port
    pub port: u16,
    /// Protocol: IPPROTO_TCP (6) or IPPROTO_UDP (17)
    pub protocol: u8,
    /// Flags: service configuration flags
    pub flags: u8,
    /// Number of backends
    pub backend_count: u32,
    /// Base index in BACKEND_MAP
    pub backend_base: u32,
    /// Session affinity type: 0=None, 1=ClientIP
    pub session_affinity: u32,
}

impl ServiceMapEntry {
    pub fn new(vip: u32, port: u16, protocol: u8) -> Self {
        Self {
            vip,
            port,
            protocol,
            ..Default::default()
        }
    }
}

/// Backend entry (stored in BACKEND_MAP)
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackendEntry {
    /// Backend IP (pod IP) as u32
    pub ip: u32,
    /// Backend port (target port)
    pub port: u16,
    /// Protocol: IPPROTO_TCP (6) or IPPROTO_UDP (17)
    pub protocol: u8,
    /// State: 0=healthy, 1=unhealthy, 2=draining
    pub state: u8,
    /// Weight for weighted load balancing
    pub weight: u32,
    /// Connection count (for monitoring)
    pub connection_count: u32,
}

impl Default for BackendEntry {
    fn default() -> Self {
        Self {
            ip: 0,
            port: 0,
            protocol: 0,
            state: 0,
            weight: 1,
            connection_count: 0,
        }
    }
}

impl BackendEntry {
    pub fn new(ip: u32, port: u16, protocol: u8) -> Self {
        Self {
            ip,
            port,
            protocol,
            state: 0, // healthy by default
            weight: 1,
            connection_count: 0,
        }
    }

    pub fn with_state(mut self, state: u8) -> Self {
        self.state = state;
        self
    }

    pub fn is_healthy(&self) -> bool {
        self.state == 0
    }
}

/// Client affinity key (used to look up session affinity)
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[allow(clippy::pub_underscore_fields)]
pub struct ClientKey {
    /// Client source IP as u32
    pub client_ip: u32,
    /// Client source port
    pub client_port: u16,
    /// Service identifier (hash of VIP + port)
    pub service_id: u32,
    /// Protocol
    pub protocol: u8,
    /// Padding
    pub _pad: u8,
}

impl ClientKey {
    pub fn new(client_ip: u32, client_port: u16, service_id: u32, protocol: u8) -> Self {
        Self {
            client_ip,
            client_port,
            service_id,
            protocol,
            _pad: 0,
        }
    }
}

/// Counter key for statistics tracking
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[allow(clippy::pub_underscore_fields)]
pub struct CounterKey {
    /// Service identifier
    pub service_id: u32,
    /// Backend index in BACKEND_MAP
    pub backend_index: u32,
    /// Counter type: 0=packets, 1=bytes, 2=errors
    pub counter_type: u8,
    /// Padding
    pub _pad: [u8; 3],
}

impl CounterKey {
    pub fn new(service_id: u32, backend_index: u32, counter_type: u8) -> Self {
        Self {
            service_id,
            backend_index,
            counter_type,
            _pad: [0; 3],
        }
    }
}

// ============================================================================
// Map Wrappers
// ============================================================================

/// In-memory simulation of SVC_MAP for testing/development
/// In production, this would interact with actual BPF maps via /sys/fs/bpf
pub struct ServiceMap {
    entries: HashMap<u32, ServiceMapEntry>,
    name: String,
}

impl ServiceMap {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            name: SVC_MAP_NAME.to_string(),
        }
    }

    /// Add a service entry
    pub fn add_service(&mut self, key: u32, entry: ServiceMapEntry) -> anyhow::Result<()> {
        if self.entries.contains_key(&key) {
            return Err(anyhow::anyhow!("Service already exists: {key}"));
        }
        self.entries.insert(key, entry);
        Ok(())
    }

    /// Update a service entry
    pub fn update_service(&mut self, key: u32, entry: ServiceMapEntry) -> anyhow::Result<()> {
        if !self.entries.contains_key(&key) {
            return Err(anyhow::anyhow!("Service not found: {key}"));
        }
        self.entries.insert(key, entry);
        Ok(())
    }

    /// Get a service entry
    pub fn get_service(&self, key: u32) -> anyhow::Result<Option<ServiceMapEntry>> {
        Ok(self.entries.get(&key).copied())
    }

    /// Delete a service entry
    pub fn delete_service(&mut self, key: u32) -> anyhow::Result<()> {
        self.entries
            .remove(&key)
            .ok_or_else(|| anyhow::anyhow!("Service not found: {key}"))?;
        Ok(())
    }

    /// Iterate over all services
    pub fn iterate_services(&self) -> Vec<(u32, ServiceMapEntry)> {
        self.entries.iter().map(|(&k, &v)| (k, v)).collect()
    }

    /// Get service count
    pub fn service_count(&self) -> usize {
        self.entries.len()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Default for ServiceMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Backend map wrapper
pub struct BackendMap {
    entries: HashMap<u32, BackendEntry>,
    name: String,
}

impl BackendMap {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            name: BACKEND_MAP_NAME.to_string(),
        }
    }

    /// Add a backend entry
    pub fn add_backend(&mut self, index: u32, entry: BackendEntry) -> anyhow::Result<()> {
        if index >= MAX_BACKENDS as u32 {
            return Err(anyhow::anyhow!("Backend index out of range: {index}"));
        }
        self.entries.insert(index, entry);
        Ok(())
    }

    /// Update a backend entry
    pub fn update_backend(&mut self, index: u32, entry: BackendEntry) -> anyhow::Result<()> {
        if !self.entries.contains_key(&index) {
            return Err(anyhow::anyhow!("Backend not found: {index}"));
        }
        self.entries.insert(index, entry);
        Ok(())
    }

    /// Get a backend entry
    pub fn get_backend(&self, index: u32) -> anyhow::Result<Option<BackendEntry>> {
        Ok(self.entries.get(&index).copied())
    }

    /// Delete a backend entry
    pub fn delete_backend(&mut self, index: u32) -> anyhow::Result<()> {
        self.entries
            .remove(&index)
            .ok_or_else(|| anyhow::anyhow!("Backend not found: {index}"))?;
        Ok(())
    }

    /// Count healthy backends
    pub fn count_healthy(&self) -> usize {
        self.entries.values().filter(|b| b.is_healthy()).count()
    }

    /// Get all backends for a service
    pub fn get_backends_for_service(
        &self,
        _service_id: u32,
        base_idx: u32,
        count: u32,
    ) -> anyhow::Result<Vec<BackendEntry>> {
        let end = base_idx + count;
        let backends: Vec<_> = (base_idx..end)
            .filter_map(|idx| self.entries.get(&idx).copied())
            .collect();
        Ok(backends)
    }

    /// Get backend count
    pub fn backend_count(&self) -> usize {
        self.entries.len()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Default for BackendMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Affinity map wrapper for session affinity
pub struct AffinityMap {
    entries: HashMap<ClientKey, u32>,
    name: String,
}

impl AffinityMap {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            name: AFFINITY_MAP_NAME.to_string(),
        }
    }

    /// Set affinity for a client
    pub fn set_affinity(&mut self, key: ClientKey, backend_idx: u32) -> anyhow::Result<()> {
        self.entries.insert(key, backend_idx);
        Ok(())
    }

    /// Get affinity for a client
    pub fn get_affinity(&self, key: ClientKey) -> anyhow::Result<Option<u32>> {
        Ok(self.entries.get(&key).copied())
    }

    /// Delete affinity for a client
    pub fn delete_affinity(&mut self, key: ClientKey) -> anyhow::Result<()> {
        self.entries
            .remove(&key)
            .ok_or_else(|| anyhow::anyhow!("Affinity entry not found"))?;
        Ok(())
    }

    /// Clear all affinity entries
    pub fn clear_all(&mut self) -> anyhow::Result<()> {
        self.entries.clear();
        Ok(())
    }

    /// Get affinity entry count
    pub fn affinity_count(&self) -> usize {
        self.entries.len()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Default for AffinityMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Counters map wrapper for statistics
pub struct CountersMap {
    entries: HashMap<CounterKey, u64>,
    name: String,
}

impl CountersMap {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            name: COUNTERS_MAP_NAME.to_string(),
        }
    }

    /// Increment a counter
    pub fn increment(&mut self, key: CounterKey, delta: u64) -> anyhow::Result<()> {
        let entry = self.entries.entry(key).or_insert(0);
        *entry = entry.saturating_add(delta);
        Ok(())
    }

    /// Get a counter value
    pub fn get_counter(&self, key: CounterKey) -> anyhow::Result<u64> {
        Ok(self.entries.get(&key).copied().unwrap_or(0))
    }

    /// Clear a counter
    pub fn clear_counter(&mut self, key: CounterKey) -> anyhow::Result<()> {
        self.entries.remove(&key);
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn counter_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for CountersMap {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_map_entry_creation() {
        let entry = ServiceMapEntry::new(0x0a00_0001, 80, 6); // 10.0.0.1:80/TCP
        assert_eq!(entry.vip, 0x0a00_0001);
        assert_eq!(entry.port, 80);
        assert_eq!(entry.protocol, 6);
        assert_eq!(entry.backend_count, 0);
    }

    #[test]
    fn test_backend_entry_creation() {
        let entry = BackendEntry::new(0x0a00_0002, 8080, 6);
        assert_eq!(entry.ip, 0x0a00_0002);
        assert_eq!(entry.port, 8080);
        assert_eq!(entry.protocol, 6);
        assert!(entry.is_healthy());
    }

    #[test]
    fn test_backend_entry_unhealthy() {
        let entry = BackendEntry::new(0x0a00_0002, 8080, 6).with_state(1);
        assert!(!entry.is_healthy());
    }

    #[test]
    fn test_service_map_add() {
        let mut map = ServiceMap::new();
        let entry = ServiceMapEntry::new(0x0a00_0001, 80, 6);

        map.add_service(1, entry).unwrap();
        assert_eq!(map.service_count(), 1);

        let retrieved = map.get_service(1).unwrap().unwrap();
        assert_eq!(retrieved.vip, 0x0a00_0001);
    }

    #[test]
    fn test_service_map_duplicate_add_fails() {
        let mut map = ServiceMap::new();
        let entry = ServiceMapEntry::new(0x0a00_0001, 80, 6);

        map.add_service(1, entry).unwrap();
        let result = map.add_service(1, entry);
        assert!(result.is_err());
    }

    #[test]
    fn test_service_map_update() {
        let mut map = ServiceMap::new();
        let mut entry = ServiceMapEntry::new(0x0a00_0001, 80, 6);
        map.add_service(1, entry).unwrap();

        entry.backend_count = 5;
        map.update_service(1, entry).unwrap();

        let retrieved = map.get_service(1).unwrap().unwrap();
        assert_eq!(retrieved.backend_count, 5);
    }

    #[test]
    fn test_service_map_delete() {
        let mut map = ServiceMap::new();
        let entry = ServiceMapEntry::new(0x0a00_0001, 80, 6);
        map.add_service(1, entry).unwrap();

        map.delete_service(1).unwrap();
        assert_eq!(map.service_count(), 0);

        let result = map.get_service(1).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_backend_map_add() {
        let mut map = BackendMap::new();
        let entry = BackendEntry::new(0x0a00_0002, 8080, 6);

        map.add_backend(0, entry).unwrap();
        assert_eq!(map.backend_count(), 1);
    }

    #[test]
    fn test_backend_map_healthy_count() {
        let mut map = BackendMap::new();
        map.add_backend(0, BackendEntry::new(0x0a00_0002, 8080, 6))
            .unwrap();
        map.add_backend(1, BackendEntry::new(0x0a00_0003, 8080, 6).with_state(1))
            .unwrap();

        assert_eq!(map.count_healthy(), 1);
    }

    #[test]
    fn test_affinity_map_set_get() {
        let mut map = AffinityMap::new();
        let key = ClientKey::new(0xc0a8_0001, 12345, 1, 6);

        map.set_affinity(key, 0).unwrap();
        let result = map.get_affinity(key).unwrap();
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_affinity_map_delete() {
        let mut map = AffinityMap::new();
        let key = ClientKey::new(0xc0a8_0001, 12345, 1, 6);

        map.set_affinity(key, 0).unwrap();
        map.delete_affinity(key).unwrap();

        let result = map.get_affinity(key).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_counters_map_increment() {
        let mut map = CountersMap::new();
        let key = CounterKey::new(1, 0, 0);

        map.increment(key, 100).unwrap();
        let value = map.get_counter(key).unwrap();
        assert_eq!(value, 100);

        map.increment(key, 50).unwrap();
        let value = map.get_counter(key).unwrap();
        assert_eq!(value, 150);
    }

    #[test]
    fn test_service_map_iterate() {
        let mut map = ServiceMap::new();
        for i in 0_u16..3 {
            let service_id = u32::from(i);
            let entry = ServiceMapEntry::new(0x0a00_0001 + service_id, 80 + i, 6);
            map.add_service(service_id, entry).unwrap();
        }

        let entries = map.iterate_services();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_backend_map_get_backends_for_service() {
        let mut map = BackendMap::new();
        for i in 0_u32..5 {
            let entry = BackendEntry::new(0x0a00_0002 + i, 8080, 6);
            map.add_backend(i, entry).unwrap();
        }

        let backends = map.get_backends_for_service(1, 0, 5).unwrap();
        assert_eq!(backends.len(), 5);
    }

    #[test]
    fn test_map_names() {
        assert_eq!(ServiceMap::new().name(), SVC_MAP_NAME);
        assert_eq!(BackendMap::new().name(), BACKEND_MAP_NAME);
        assert_eq!(AffinityMap::new().name(), AFFINITY_MAP_NAME);
        assert_eq!(CountersMap::new().name(), COUNTERS_MAP_NAME);
    }
}
