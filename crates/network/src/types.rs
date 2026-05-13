use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use thiserror::Error;
use tracing::debug;

/// Errors returned by pure network type helpers.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NetworkError {
    /// The provided MAC address was invalid.
    #[error("invalid MAC address: {0}")]
    InvalidMAC(String),
    /// The provided CIDR was invalid.
    #[error("invalid CIDR: {0}")]
    InvalidCIDR(String),
    /// The provided IP address was invalid.
    #[error("invalid address: {0}")]
    InvalidAddress(String),
}

/// A 6-byte MAC address.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MAC([u8; 6]);

impl MAC {
    /// Creates a MAC address from an exact 6-byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, NetworkError> {
        let bytes: [u8; 6] = bytes.try_into().map_err(|_| {
            NetworkError::InvalidMAC(format!("expected 6 bytes, got {}", bytes.len()))
        })?;
        Ok(Self(bytes))
    }

    /// Returns the raw six-byte representation.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }

    /// Returns true when all bytes are zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 6]
    }

    /// Returns the Ethernet broadcast MAC address.
    #[must_use]
    pub const fn broadcast() -> Self {
        Self([0xff; 6])
    }
}

impl fmt::Display for MAC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl FromStr for MAC {
    type Err = NetworkError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split([':', '-']).collect();
        if parts.len() != 6 || parts.iter().any(|part| part.len() != 2) {
            return Err(NetworkError::InvalidMAC(s.to_owned()));
        }

        let mut bytes = [0_u8; 6];
        for (index, part) in parts.iter().enumerate() {
            bytes[index] =
                u8::from_str_radix(part, 16).map_err(|_| NetworkError::InvalidMAC(s.to_owned()))?;
        }

        Ok(Self(bytes))
    }
}

/// Represents a node's IPv4 address with its optional allocation CIDR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIPv4 {
    /// The node's IPv4 address.
    pub ip: Ipv4Addr,
    /// The IPv4 CIDR associated with the node address.
    pub cidr: Option<Ipv4Net>,
}

/// Represents a node's IPv6 address with its optional allocation CIDR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIPv6 {
    /// The node's IPv6 address.
    pub ip: Ipv6Addr,
    /// The IPv6 CIDR associated with the node address.
    pub cidr: Option<Ipv6Net>,
}

/// Returns true if `inner` is fully contained within `outer`.
#[must_use]
pub fn cidr_contains(outer: &IpNet, inner: &IpNet) -> bool {
    match (outer, inner) {
        (IpNet::V4(outer), IpNet::V4(inner)) => {
            let (outer_start, outer_end) = ipv4_bounds(*outer);
            let (inner_start, inner_end) = ipv4_bounds(*inner);
            outer_start <= inner_start && inner_end <= outer_end
        }
        (IpNet::V6(outer), IpNet::V6(inner)) => {
            let (outer_start, outer_end) = ipv6_bounds(outer);
            let (inner_start, inner_end) = ipv6_bounds(inner);
            outer_start <= inner_start && inner_end <= outer_end
        }
        _ => false,
    }
}

/// Returns the set of CIDRs required to cover `target` while excluding `exclude`.
#[must_use]
pub fn subtract_cidr(target: &IpNet, exclude: &IpNet) -> Vec<IpNet> {
    if !nets_overlap(target, exclude) {
        return vec![*target];
    }

    if cidr_contains(exclude, target) {
        return Vec::new();
    }

    if !cidr_contains(target, exclude) {
        return vec![*target];
    }

    let Some((left, right)) = split_cidr(target) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for child in [left, right] {
        if nets_overlap(&child, exclude) {
            result.extend(subtract_cidr(&child, exclude));
        } else {
            result.push(child);
        }
    }
    result
}

/// Returns true when the IP is loopback, link-local, or unspecified.
#[must_use]
pub fn is_reserved(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_link_local() || ip.is_unspecified(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unicast_link_local() || ip.is_unspecified(),
    }
}

/// Converts an IPv4 prefix length into a netmask.
#[must_use]
pub fn prefix_to_mask_v4(prefix_len: u8) -> Ipv4Addr {
    let prefix = prefix_len.min(32);
    if prefix != prefix_len {
        debug!(
            prefix_len,
            clamped = prefix,
            "clamping invalid IPv4 prefix length"
        );
    }

    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    };
    Ipv4Addr::from(mask)
}

/// A pure route value independent from platform netlink APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The routed destination prefix.
    pub prefix: IpNet,
    /// The optional next-hop gateway.
    pub nexthop: Option<IpAddr>,
    /// The outgoing device name.
    pub device: String,
    /// The optional route MTU.
    pub mtu: Option<u32>,
    /// The routing table identifier.
    pub table: u32,
    /// The route installation protocol.
    pub proto: RouteProtocol,
    /// The route scope.
    pub scope: RouteScope,
}

/// Linux-compatible route protocol identifiers used by Cilium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RouteProtocol {
    /// Unspecified protocol.
    Unspec = 0,
    /// Kernel-managed route.
    Kernel = 2,
    /// Boot-time route.
    Boot = 3,
    /// Static administrator-configured route.
    Static = 4,
    /// Cilium-managed route.
    Cilium = 99,
}

/// Linux-compatible route scope values used by Cilium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RouteScope {
    /// Globally reachable route scope.
    Universe = 0,
    /// Site-local route scope.
    Site = 200,
    /// On-link route scope.
    Link = 253,
    /// Host-only route scope.
    Host = 254,
    /// Nowhere / blackhole route scope.
    Nowhere = 255,
}

impl Route {
    /// Returns true if the route is directly reachable on-link.
    #[must_use]
    pub fn is_local(&self) -> bool {
        self.scope == RouteScope::Link
    }
}

/// Identifies an ARP/NDP responder entry by IP and interface index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct L2ResponderEntry {
    /// The proxied IP address.
    pub ip: IpAddr,
    /// The interface index for the responder.
    pub ifindex: u32,
}

fn nets_overlap(left: &IpNet, right: &IpNet) -> bool {
    match (left, right) {
        (IpNet::V4(left), IpNet::V4(right)) => {
            let (left_start, left_end) = ipv4_bounds(*left);
            let (right_start, right_end) = ipv4_bounds(*right);
            left_start <= right_end && right_start <= left_end
        }
        (IpNet::V6(left), IpNet::V6(right)) => {
            let (left_start, left_end) = ipv6_bounds(left);
            let (right_start, right_end) = ipv6_bounds(right);
            left_start <= right_end && right_start <= left_end
        }
        _ => false,
    }
}

fn split_cidr(net: &IpNet) -> Option<(IpNet, IpNet)> {
    match net {
        IpNet::V4(net) => split_ipv4_cidr(*net),
        IpNet::V6(net) => split_ipv6_cidr(net),
    }
}

fn split_ipv4_cidr(net: Ipv4Net) -> Option<(IpNet, IpNet)> {
    let prefix = net.prefix_len();
    if prefix >= 32 {
        return None;
    }

    let left = Ipv4Net::new(net.network(), prefix + 1).ok()?;
    let right_start = u32::from(net.network()) + (1_u32 << (32 - u32::from(prefix + 1)));
    let right = Ipv4Net::new(Ipv4Addr::from(right_start), prefix + 1).ok()?;
    Some((IpNet::V4(left), IpNet::V4(right)))
}

fn split_ipv6_cidr(net: &Ipv6Net) -> Option<(IpNet, IpNet)> {
    let prefix = net.prefix_len();
    if prefix >= 128 {
        return None;
    }

    let left = Ipv6Net::new(net.network(), prefix + 1).ok()?;
    let right_start = u128::from(net.network()) + (1_u128 << (128 - u32::from(prefix + 1)));
    let right = Ipv6Net::new(Ipv6Addr::from(right_start), prefix + 1).ok()?;
    Some((IpNet::V6(left), IpNet::V6(right)))
}

fn ipv4_bounds(net: Ipv4Net) -> (u32, u32) {
    let start = u32::from(net.network());
    let end = if net.prefix_len() == 32 {
        start
    } else {
        start | (u32::MAX >> u32::from(net.prefix_len()))
    };
    (start, end)
}

fn ipv6_bounds(net: &Ipv6Net) -> (u128, u128) {
    let start = u128::from(net.network());
    let end = if net.prefix_len() == 128 {
        start
    } else {
        start | (u128::MAX >> u32::from(net.prefix_len()))
    };
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_parse_and_display() {
        let mac: MAC = "aa:bb:cc:dd:ee:ff".parse().unwrap();
        assert_eq!(mac.to_string(), "aa:bb:cc:dd:ee:ff");
        assert!(!mac.is_zero());
        assert!(MAC([0; 6]).is_zero());
    }

    #[test]
    fn test_mac_broadcast() {
        assert_eq!(MAC::broadcast().to_string(), "ff:ff:ff:ff:ff:ff");
    }

    #[test]
    fn test_mac_invalid_parse() {
        assert!("not:a:mac".parse::<MAC>().is_err());
        assert!("aa:bb:cc".parse::<MAC>().is_err());
    }

    #[test]
    fn test_mac_from_bytes() {
        let mac = MAC::from_bytes(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]).unwrap();
        assert_eq!(mac.as_bytes(), &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert!(MAC::from_bytes(&[0xaa, 0xbb]).is_err());
    }

    #[test]
    fn test_cidr_contains() {
        let outer: IpNet = "10.0.0.0/8".parse().unwrap();
        let inner: IpNet = "10.1.2.0/24".parse().unwrap();
        assert!(cidr_contains(&outer, &inner));
        assert!(!cidr_contains(&inner, &outer));
    }

    #[test]
    fn test_subtract_cidr() {
        let target: IpNet = "10.0.0.0/24".parse().unwrap();
        let exclude: IpNet = "10.0.0.0/25".parse().unwrap();
        let result = subtract_cidr(&target, &exclude);
        assert_eq!(result, vec!["10.0.0.128/25".parse().unwrap()]);
        for cidr in &result {
            assert!(cidr_contains(&target, cidr));
            assert!(!nets_overlap(cidr, &exclude));
        }
    }

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved("127.0.0.1".parse().unwrap()));
        assert!(is_reserved("::1".parse().unwrap()));
        assert!(!is_reserved("10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_prefix_to_mask_v4() {
        assert_eq!(prefix_to_mask_v4(0), Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(prefix_to_mask_v4(24), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(prefix_to_mask_v4(32), Ipv4Addr::new(255, 255, 255, 255));
    }

    #[test]
    fn test_route_is_local() {
        let route = Route {
            prefix: "0.0.0.0/0".parse().unwrap(),
            nexthop: None,
            device: "eth0".into(),
            mtu: None,
            table: 254,
            proto: RouteProtocol::Cilium,
            scope: RouteScope::Link,
        };
        assert!(route.is_local());
    }
}
