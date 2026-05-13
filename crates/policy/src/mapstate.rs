//! Distilled map-state output for policy resolution.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::TrafficDirection;

/// The final distilled policy map keyed by datapath lookup key.
pub type MapStateMap = HashMap<Key, MapStateEntry>;

/// Datapath lookup key used by the policy map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    /// Remote security identity.
    pub identity: u32,
    /// Destination port.
    pub dest_port: u16,
    /// Next-header or protocol number.
    pub nexthdr: u8,
    /// Traffic direction.
    pub traffic_direction: TrafficDirection,
}

impl Key {
    /// Creates a new map-state key.
    #[must_use]
    pub fn new(
        identity: u32,
        dest_port: u16,
        nexthdr: u8,
        traffic_direction: TrafficDirection,
    ) -> Self {
        Self {
            identity,
            dest_port,
            nexthdr,
            traffic_direction,
        }
    }
}

/// Value stored for a map-state key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapStateEntry {
    /// Proxy port used for redirection.
    pub proxy_port: u16,
    /// Whether this entry denies traffic.
    pub deny: bool,
    /// Whether this entry requires authentication.
    pub authenticate: bool,
}

impl MapStateEntry {
    /// Creates a new map-state entry.
    #[must_use]
    pub fn new(proxy_port: u16, deny: bool, authenticate: bool) -> Self {
        Self {
            proxy_port,
            deny,
            authenticate,
        }
    }

    /// Creates an allow entry.
    #[must_use]
    pub fn allow(proxy_port: u16, authenticate: bool) -> Self {
        Self::new(proxy_port, false, authenticate)
    }

    /// Creates a deny entry.
    #[must_use]
    pub fn deny() -> Self {
        Self::new(0, true, false)
    }
}

/// Inserts a map-state entry while keeping deny semantics stable.
pub fn insert_map_state(map: &mut MapStateMap, key: Key, entry: MapStateEntry) {
    match map.get_mut(&key) {
        Some(existing) if existing.deny && !entry.deny => {}
        Some(existing) => {
            existing.deny = existing.deny || entry.deny;
            existing.authenticate = existing.authenticate || entry.authenticate;
            if existing.proxy_port == 0 {
                existing.proxy_port = entry.proxy_port;
            }
        }
        None => {
            map.insert(key, entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_entries_override_existing_allow_entries() {
        let key = Key::new(42, 80, 6, TrafficDirection::Ingress);
        let mut state = MapStateMap::new();
        insert_map_state(&mut state, key, MapStateEntry::allow(0, false));
        insert_map_state(&mut state, key, MapStateEntry::deny());

        assert_eq!(state.get(&key), Some(&MapStateEntry::deny()));
    }
}
