// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Gateway configuration handling

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::error::{Error, Result};
use crate::types::{LabelSelector, SpecialIPs};

/// Policy-level gateway configuration specification
#[derive(Debug, Clone)]
pub struct PolicyGatewayConfig {
    /// Node selector for choosing gateway nodes
    pub node_selector: LabelSelector,
    /// Interface name (optional)
    pub interface: Option<String>,
    /// Egress IP address (optional)
    pub egress_ip: Option<IpAddr>,
}

impl PolicyGatewayConfig {
    /// Create a new policy gateway config
    pub fn new(node_selector: LabelSelector) -> Self {
        Self {
            node_selector,
            interface: None,
            egress_ip: None,
        }
    }

    /// Set interface name
    pub fn with_interface(mut self, interface: impl Into<String>) -> Self {
        self.interface = Some(interface.into());
        self
    }

    /// Set egress IP
    pub fn with_egress_ip(mut self, ip: IpAddr) -> Self {
        self.egress_ip = Some(ip);
        self
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Can't specify both interface and egress IP
        if self.interface.is_some() && self.egress_ip.is_some() {
            return Err(Error::GatewayConfigError(
                "cannot specify both interface and egress IP".to_string(),
            ));
        }

        Ok(())
    }
}

/// Runtime gateway configuration derived from policy and system state
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Interface name for SNAT traffic
    pub interface_name: String,
    /// Interface index (ifindex)
    pub interface_index: u32,
    /// IPv4 address used for SNAT
    pub egress_ipv4: Ipv4Addr,
    /// IPv6 address used for SNAT
    pub egress_ipv6: Ipv6Addr,
    /// Gateway node IP
    pub gateway_ip: IpAddr,
    /// Whether local node is configured as gateway
    pub local_node_configured_as_gateway: bool,
}

impl GatewayConfig {
    /// Create new gateway config with unspecified addresses
    pub fn new() -> Self {
        Self {
            interface_name: String::new(),
            interface_index: 0,
            egress_ipv4: SpecialIPs::EGRESS_IP_NOT_FOUND_IPV4,
            egress_ipv6: SpecialIPs::EGRESS_IP_NOT_FOUND_IPV6,
            gateway_ip: IpAddr::V4(SpecialIPs::GATEWAY_NOT_FOUND_IPV4),
            local_node_configured_as_gateway: false,
        }
    }

    /// Set interface information
    pub fn set_interface(&mut self, name: impl Into<String>, index: u32) -> &mut Self {
        self.interface_name = name.into();
        self.interface_index = index;
        self
    }

    /// Set egress IPv4 address
    pub fn set_egress_ipv4(&mut self, ip: Ipv4Addr) -> &mut Self {
        self.egress_ipv4 = ip;
        self
    }

    /// Set egress IPv6 address
    pub fn set_egress_ipv6(&mut self, ip: Ipv6Addr) -> &mut Self {
        self.egress_ipv6 = ip;
        self
    }

    /// Set gateway IP
    pub fn set_gateway_ip(&mut self, ip: IpAddr) -> &mut Self {
        self.gateway_ip = ip;
        self
    }

    /// Set local node configured as gateway flag
    pub fn set_local_node_configured(&mut self, configured: bool) -> &mut Self {
        self.local_node_configured_as_gateway = configured;
        self
    }

    /// Check if gateway config is valid (has gateway IP)
    pub fn is_valid(&self) -> bool {
        match self.gateway_ip {
            IpAddr::V4(v4) => v4 != SpecialIPs::GATEWAY_NOT_FOUND_IPV4,
            IpAddr::V6(v6) => v6 != SpecialIPs::GATEWAY_NOT_FOUND_IPV6,
        }
    }

    /// Check if IPv4 is configured
    pub fn has_ipv4(&self) -> bool {
        self.egress_ipv4 != SpecialIPs::EGRESS_IP_NOT_FOUND_IPV4
    }

    /// Check if IPv6 is configured
    pub fn has_ipv6(&self) -> bool {
        self.egress_ipv6 != SpecialIPs::EGRESS_IP_NOT_FOUND_IPV6
    }
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LabelSelector;

    #[test]
    fn test_policy_gateway_config_creation() {
        let selector = LabelSelector::new();
        let config = PolicyGatewayConfig::new(selector);
        assert!(config.interface.is_none());
        assert!(config.egress_ip.is_none());
    }

    #[test]
    fn test_policy_gateway_config_with_interface() {
        let selector = LabelSelector::new();
        let config = PolicyGatewayConfig::new(selector).with_interface("eth0");
        assert_eq!(config.interface, Some("eth0".to_string()));
    }

    #[test]
    fn test_policy_gateway_config_validation_both_specified() {
        let selector = LabelSelector::new();
        let config = PolicyGatewayConfig::new(selector)
            .with_interface("eth0")
            .with_egress_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)));

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gateway_config_creation() {
        let config = GatewayConfig::new();
        assert_eq!(config.interface_name, "");
        assert_eq!(config.interface_index, 0);
        assert!(!config.has_ipv4());
        assert!(!config.has_ipv6());
    }

    #[test]
    fn test_gateway_config_set_interface() {
        let mut config = GatewayConfig::new();
        config
            .set_interface("eth0", 2)
            .set_egress_ipv4(Ipv4Addr::new(10, 0, 0, 1))
            .set_gateway_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        assert_eq!(config.interface_name, "eth0");
        assert_eq!(config.interface_index, 2);
        assert!(config.has_ipv4());
        assert!(config.is_valid());
    }

    #[test]
    fn test_gateway_config_validity() {
        let mut valid_config = GatewayConfig::new();
        valid_config.set_gateway_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(valid_config.is_valid());

        let invalid_config = GatewayConfig::new();
        assert!(!invalid_config.is_valid());
    }
}
