use serde::{Deserialize, Serialize};

/// Core monitor message discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum MessageType {
    /// Drop notifications.
    Drop = 1,
    /// Debug notifications.
    Debug = 2,
    /// Packet capture notifications.
    Capture = 3,
    /// Generic notifications.
    Generic = 4,
    /// Trace notifications.
    Trace = 5,
    /// Policy verdict notifications.
    PolicyVerdict = 6,
    /// Agent-generated notifications.
    AgentNotify = 7,
    /// Lost perf records.
    RecordLost = 0x8000,
}

/// A subset of core drop reasons shared by monitor events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DropReason {
    /// Invalid source MAC address.
    InvalidSourceMac = 0,
    /// Invalid destination MAC address.
    InvalidDestMac = 1,
    /// Invalid source IP address.
    InvalidSourceIP = 2,
    /// Policy denied the packet.
    PolicyDenied = 3,
    /// Invalid packet contents.
    InvalidPacket = 4,
    /// Tunnel key lookup failed.
    NoTunnelKey = 5,
    /// Unspecified datapath error.
    Error = 6,
    /// Unknown or unmapped drop reason.
    Unknown = 0xFF,
}

impl DropReason {
    /// Maps a raw drop reason value into a known reason.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::InvalidSourceMac,
            1 => Self::InvalidDestMac,
            2 => Self::InvalidSourceIP,
            3 => Self::PolicyDenied,
            4 => Self::InvalidPacket,
            5 => Self::NoTunnelKey,
            6 => Self::Error,
            _ => Self::Unknown,
        }
    }

    /// Returns the human-readable description for this drop reason.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::InvalidSourceMac => "invalid source mac",
            Self::InvalidDestMac => "invalid destination mac",
            Self::InvalidSourceIP => "invalid source ip",
            Self::PolicyDenied => "policy denied",
            Self::InvalidPacket => "invalid packet",
            Self::NoTunnelKey => "no tunnel key",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_reason_from_u8_maps_expected_values() {
        assert_eq!(DropReason::from_u8(0), DropReason::InvalidSourceMac);
        assert_eq!(DropReason::from_u8(1), DropReason::InvalidDestMac);
        assert_eq!(DropReason::from_u8(2), DropReason::InvalidSourceIP);
        assert_eq!(DropReason::from_u8(3), DropReason::PolicyDenied);
        assert_eq!(DropReason::from_u8(4), DropReason::InvalidPacket);
        assert_eq!(DropReason::from_u8(5), DropReason::NoTunnelKey);
        assert_eq!(DropReason::from_u8(6), DropReason::Error);
        assert_eq!(DropReason::from_u8(u8::MAX), DropReason::Unknown);
    }

    #[test]
    fn message_type_discriminants_match_expected_values() {
        assert_eq!(MessageType::Drop as u16, 1);
        assert_eq!(MessageType::Debug as u16, 2);
        assert_eq!(MessageType::Capture as u16, 3);
        assert_eq!(MessageType::Generic as u16, 4);
        assert_eq!(MessageType::Trace as u16, 5);
        assert_eq!(MessageType::PolicyVerdict as u16, 6);
        assert_eq!(MessageType::AgentNotify as u16, 7);
        assert_eq!(MessageType::RecordLost as u16, 0x8000);
    }
}
