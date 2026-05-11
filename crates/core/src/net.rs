//! Networking types for seriousum.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub use ipnet::IpNet as IpNetwork;
pub use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Port(u16);

impl Port {
    pub const fn new(port: u16) -> Self {
        Self(port)
    }
    pub const fn as_u16(self) -> u16 {
        self.0
    }
    pub const fn is_privileged(self) -> bool {
        self.0 < 1024
    }
    pub const fn is_ephemeral(self) -> bool {
        self.0 >= 49152
    }
    pub const fn cilium_health() -> Self {
        Self(4244)
    }
    pub const fn cilium_agent() -> Self {
        Self(9876)
    }
    pub const fn cilium_operator() -> Self {
        Self(9234)
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u16> for Port {
    fn from(port: u16) -> Self {
        Self(port)
    }
}
impl From<Port> for u16 {
    fn from(port: Port) -> Self {
        port.0
    }
}

impl serde::Serialize for Port {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Port {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <u16 as Deserialize>::deserialize(deserializer).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
    pub const fn zero() -> Self {
        Self([0; 6])
    }
    pub const fn broadcast() -> Self {
        Self([0xff; 6])
    }
    pub const fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
    pub const fn is_unicast(self) -> bool {
        self.0[0] & 1 == 0
    }
    pub const fn is_multicast(self) -> bool {
        self.0[0] & 1 == 1
    }
    pub fn from_endpoint_id(id: u32) -> Self {
        let b = id.to_be_bytes();
        Self([0x00, 0x20, 0x00, b[0], b[1], b[2]])
    }
}

impl Default for MacAddr {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl FromStr for MacAddr {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split([':', '-']).collect();
        if parts.len() != 6 {
            return Err(anyhow::anyhow!("invalid MAC address: {s}"));
        }
        let mut bytes = [0u8; 6];
        for (idx, part) in parts.iter().enumerate() {
            bytes[idx] = u8::from_str_radix(part, 16)?;
        }
        Ok(Self(bytes))
    }
}

impl serde::Serialize for MacAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for MacAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <String as Deserialize>::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Protocol {
    Icmp,
    Tcp,
    Udp,
    Sctp,
    Gre,
    Other(u8),
}

impl Protocol {
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Icmp,
            6 => Self::Tcp,
            17 => Self::Udp,
            132 => Self::Sctp,
            47 => Self::Gre,
            x => Self::Other(x),
        }
    }
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Icmp => 1,
            Self::Tcp => 6,
            Self::Udp => 17,
            Self::Sctp => 132,
            Self::Gre => 47,
            Self::Other(x) => x,
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Icmp => f.write_str("icmp"),
            Self::Tcp => f.write_str("tcp"),
            Self::Udp => f.write_str("udp"),
            Self::Sctp => f.write_str("sctp"),
            Self::Gre => f.write_str("gre"),
            Self::Other(v) => write!(f, "{v}"),
        }
    }
}

impl From<u8> for Protocol {
    fn from(value: u8) -> Self {
        Self::from_u8(value)
    }
}
impl From<Protocol> for u8 {
    fn from(value: Protocol) -> Self {
        value.as_u8()
    }
}

#[derive(Debug, Clone)]
pub struct Interface {
    pub name: String,
    pub ifindex: u32,
    pub mac: MacAddr,
    pub ips: Vec<IpNetwork>,
    pub mtu: u32,
    pub is_up: bool,
    pub is_loopback: bool,
}

#[derive(Debug, Clone)]
pub struct NetNs {
    pub path: String,
    pub id: u64,
    pub interfaces: Vec<Interface>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketAddr {
    pub ip: IpAddr,
    pub port: Port,
}

impl SocketAddr {
    pub const fn new(ip: IpAddr, port: Port) -> Self {
        Self { ip, port }
    }
}

impl fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}]:{}", self.ip, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum L7Protocol {
    Http,
    Http2,
    Grpc,
    Kafka,
    Redis,
    Custom(String),
}

impl fmt::Display for L7Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http => f.write_str("http"),
            Self::Http2 => f.write_str("http2"),
            Self::Grpc => f.write_str("grpc"),
            Self::Kafka => f.write_str("kafka"),
            Self::Redis => f.write_str("redis"),
            Self::Custom(s) => f.write_str(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PortRule {
    pub port_range: (u16, u16),
    pub protocol: Protocol,
    pub name: Option<String>,
}

impl PortRule {
    pub fn single_port(port: u16, protocol: Protocol) -> Self {
        Self {
            port_range: (port, port),
            protocol,
            name: None,
        }
    }
    pub fn matches(&self, port: u16, protocol: Protocol) -> bool {
        self.protocol == protocol && port >= self.port_range.0 && port <= self.port_range.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mac_parse() {
        let mac: MacAddr = "00:11:22:33:44:55".parse().unwrap();
        assert_eq!(mac.as_bytes(), &[0, 17, 34, 51, 68, 85]);
    }
    #[test]
    fn port_rule() {
        assert!(PortRule::single_port(80, Protocol::Tcp).matches(80, Protocol::Tcp));
    }
}
