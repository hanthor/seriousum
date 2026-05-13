// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Endpoint metadata handling for egress gateway

use std::net::IpAddr;

use crate::error::{Error, Result};
use crate::types::{EndpointID, Labels};

/// Metadata associated with an endpoint
#[derive(Debug, Clone)]
pub struct EndpointMetadata {
    /// Endpoint ID (based on UID)
    pub id: EndpointID,
    /// Endpoint labels (from identity)
    pub labels: Labels,
    /// Endpoint IP addresses
    pub ips: Vec<IpAddr>,
    /// Node IP where endpoint is running
    pub node_ip: String,
}

impl EndpointMetadata {
    /// Create new endpoint metadata
    pub fn new(id: EndpointID, labels: Labels, ips: Vec<IpAddr>, node_ip: String) -> Self {
        Self {
            id,
            labels,
            ips,
            node_ip,
        }
    }

    /// Validate that metadata has required fields
    pub fn validate(&self) -> Result<()> {
        if self.ips.is_empty() {
            return Err(Error::EndpointError("endpoint has no IPs".to_string()));
        }

        if self.node_ip.is_empty() {
            return Err(Error::EndpointError("endpoint has no node IP".to_string()));
        }

        Ok(())
    }

    /// Get IPv4 addresses
    pub fn ipv4_addresses(&self) -> Vec<std::net::Ipv4Addr> {
        self.ips
            .iter()
            .filter_map(|ip| {
                if let IpAddr::V4(v4) = ip {
                    Some(*v4)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get IPv6 addresses
    pub fn ipv6_addresses(&self) -> Vec<std::net::Ipv6Addr> {
        self.ips
            .iter()
            .filter_map(|ip| {
                if let IpAddr::V6(v6) = ip {
                    Some(*v6)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if endpoint has IPv4 address
    pub fn has_ipv4(&self) -> bool {
        self.ips.iter().any(IpAddr::is_ipv4)
    }

    /// Check if endpoint has IPv6 address
    pub fn has_ipv6(&self) -> bool {
        self.ips.iter().any(IpAddr::is_ipv6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::str::FromStr;

    #[test]
    fn test_endpoint_metadata_creation() {
        let mut labels = HashMap::new();
        labels.insert("app".to_string(), "web".to_string());

        let ips = vec![
            IpAddr::from_str("10.0.0.1").unwrap(),
            IpAddr::from_str("fd00::1").unwrap(),
        ];

        let meta = EndpointMetadata::new(EndpointID(123), labels, ips, "192.168.1.1".to_string());

        assert_eq!(meta.ips.len(), 2);
        assert!(meta.has_ipv4());
        assert!(meta.has_ipv6());
    }

    #[test]
    fn test_endpoint_ipv4_addresses() {
        let ips = vec![
            IpAddr::from_str("10.0.0.1").unwrap(),
            IpAddr::from_str("fd00::1").unwrap(),
            IpAddr::from_str("10.0.0.2").unwrap(),
        ];

        let meta = EndpointMetadata::new(
            EndpointID(123),
            HashMap::new(),
            ips,
            "192.168.1.1".to_string(),
        );

        let v4_addrs = meta.ipv4_addresses();
        assert_eq!(v4_addrs.len(), 2);
    }

    #[test]
    fn test_endpoint_validation() {
        // Empty IPs should fail
        let meta_no_ips = EndpointMetadata::new(
            EndpointID(123),
            HashMap::new(),
            vec![],
            "192.168.1.1".to_string(),
        );
        assert!(meta_no_ips.validate().is_err());

        // Empty node IP should fail
        let meta_no_node = EndpointMetadata::new(
            EndpointID(123),
            HashMap::new(),
            vec![IpAddr::from_str("10.0.0.1").unwrap()],
            String::new(),
        );
        assert!(meta_no_node.validate().is_err());

        // Valid metadata should pass
        let meta_valid = EndpointMetadata::new(
            EndpointID(123),
            HashMap::new(),
            vec![IpAddr::from_str("10.0.0.1").unwrap()],
            "192.168.1.1".to_string(),
        );
        assert!(meta_valid.validate().is_ok());
    }
}
