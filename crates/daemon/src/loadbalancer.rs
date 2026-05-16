//! Minimal Track I implementation: sync discovered service backends to eBPF maps
//! 
//! This module provides basic service backend map synchronization for DNS and L7 support.
//! It's a pragmatic stub implementation that tracks service/backend changes.

use std::collections::HashMap;
use std::net::Ipv4Addr;

/// Backend discovery tracker
#[derive(Debug, Default)]
pub struct BackendSyncer {
    /// Cached backends: (service_key) -> [(backend_ip, backend_port, protocol)]
    synced_backends: HashMap<String, Vec<(u32, u16, u8)>>,
}

impl BackendSyncer {
    pub fn new() -> Self {
        Self {
            synced_backends: HashMap::new(),
        }
    }

    /// Sync backends for a service
    /// Returns true if backends changed
    pub fn sync_backends(
        &mut self,
        service_key: &str,
        backends: &[(Ipv4Addr, u16, &str)],
    ) -> bool {
        let new_backends: Vec<_> = backends
            .iter()
            .map(|(ip, port, protocol)| {
                (
                    u32::from_be_bytes(ip.octets()),
                    *port,
                    protocol_to_u8(protocol),
                )
            })
            .collect();

        let old = self.synced_backends.get(service_key).cloned();
        let changed = old.as_ref() != Some(&new_backends);

        if changed {
            self.synced_backends
                .insert(service_key.to_string(), new_backends);
            // TODO: Actually write to eBPF maps here
            tracing::debug!(
                service = service_key,
                backend_count = old.as_ref().map(|b| b.len()).unwrap_or(0),
                new_count = self.synced_backends[service_key].len(),
                "backends synced"
            );
        }

        changed
    }
}

fn protocol_to_u8(protocol: &str) -> u8 {
    match protocol {
        "TCP" => 6,
        "UDP" => 17,
        "SCTP" => 132,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_syncer_detects_changes() {
        let mut syncer = BackendSyncer::new();
        let backends = vec![(Ipv4Addr::new(10, 0, 0, 1), 8080u16, "TCP")];

        // First sync should be detected as change
        assert!(syncer.sync_backends("test/service", &backends));

        // Same backends should not be detected as change
        assert!(!syncer.sync_backends("test/service", &backends));

        // Different backends should be detected as change
        let new_backends = vec![
            (Ipv4Addr::new(10, 0, 0, 1), 8080u16, "TCP"),
            (Ipv4Addr::new(10, 0, 0, 2), 8080u16, "TCP"),
        ];
        assert!(syncer.sync_backends("test/service", &new_backends));
    }

    #[test]
    fn test_protocol_conversion() {
        assert_eq!(protocol_to_u8("TCP"), 6);
        assert_eq!(protocol_to_u8("UDP"), 17);
        assert_eq!(protocol_to_u8("SCTP"), 132);
        assert_eq!(protocol_to_u8("UNKNOWN"), 0);
    }
}
