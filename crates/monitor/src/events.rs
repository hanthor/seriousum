use serde::{Deserialize, Serialize};
use thiserror::Error;

const TRACE_NOTIFY_V0_LEN: usize = 32;
const TRACE_NOTIFY_V1_LEN: usize = 48;
const TRACE_NOTIFY_V2_LEN: usize = 56;
const DROP_NOTIFY_V1_LEN: usize = 36;
const DROP_NOTIFY_V2_LEN: usize = 40;
const DROP_NOTIFY_V3_LEN: usize = 48;
const TRACE_NOTIFY_FLAG_IS_IPV6: u8 = 1 << 0;
const TRACE_NOTIFY_FLAG_IS_L3_DEVICE: u8 = 1 << 1;
const TRACE_NOTIFY_FLAG_IS_VXLAN: u8 = 1 << 2;
const TRACE_NOTIFY_FLAG_IS_GENEVE: u8 = 1 << 3;
const DROP_NOTIFY_FLAG_IS_IPV6: u8 = 1 << 0;
const DROP_NOTIFY_FLAG_IS_L3_DEVICE: u8 = 1 << 1;
const DROP_NOTIFY_FLAG_IS_VXLAN: u8 = 1 << 2;
const DROP_NOTIFY_FLAG_IS_GENEVE: u8 = 1 << 3;
const TRACE_NOTIFY_VERSION2: u8 = 2;
const DROP_NOTIFY_VERSION2: u8 = 2;
const DROP_NOTIFY_VERSION3: u8 = 3;
const TRACE_REASON_ENCRYPT_MASK: u8 = 0x80;

/// Errors returned while decoding datapath monitor events.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EventDecodeError {
    /// The event payload is too short for the expected layout.
    #[error("unexpected {event} data length, expected at least {expected} but got {actual}")]
    BufferTooShort {
        /// The event name.
        event: &'static str,
        /// The minimum expected size.
        expected: usize,
        /// The number of bytes that were provided.
        actual: usize,
    },
    /// The event version is not supported.
    #[error("unrecognized {event} event (version {version})")]
    UnsupportedVersion {
        /// The event name.
        event: &'static str,
        /// The unexpected version.
        version: u8,
    },
}

/// Datapath trace event mirroring Cilium's `trace_notify` layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceNotify {
    /// Event type discriminator.
    pub type_: u8,
    /// Observation point.
    pub obs_point: u8,
    /// Source endpoint ID.
    pub source: u16,
    /// Flow hash.
    pub hash: u32,
    /// Original packet length.
    pub orig_len: u32,
    /// Captured packet length.
    pub cap_len: u16,
    /// Layout version.
    pub version: u8,
    /// Extension layout version.
    pub ext_version: u8,
    /// Source identity.
    pub src_label: u32,
    /// Destination identity.
    pub dst_label: u32,
    /// Destination endpoint ID.
    pub dst_id: u16,
    /// Trace reason.
    pub reason: u8,
    /// Bitflags describing the event.
    pub flags: u8,
    /// Kernel interface index.
    pub ifindex: u32,
    /// Original IP address bytes.
    pub orig_ip: [u8; 16],
    /// Optional IP trace identifier.
    pub ip_trace_id: u64,
}

impl TraceNotify {
    /// Decodes a trace event from raw datapath bytes.
    pub fn decode(data: &[u8]) -> Result<Self, EventDecodeError> {
        if data.len() < TRACE_NOTIFY_V0_LEN {
            return Err(EventDecodeError::BufferTooShort {
                event: "trace",
                expected: TRACE_NOTIFY_V0_LEN,
                actual: data.len(),
            });
        }

        let version = data[14];
        if version > TRACE_NOTIFY_VERSION2 {
            return Err(EventDecodeError::UnsupportedVersion {
                event: "trace",
                version,
            });
        }

        let mut orig_ip = [0_u8; 16];
        let mut ip_trace_id = 0_u64;
        if version >= 1 {
            if data.len() < TRACE_NOTIFY_V1_LEN {
                return Err(EventDecodeError::BufferTooShort {
                    event: "trace",
                    expected: TRACE_NOTIFY_V1_LEN,
                    actual: data.len(),
                });
            }
            orig_ip.copy_from_slice(&data[32..48]);
        }
        if version >= TRACE_NOTIFY_VERSION2 {
            if data.len() < TRACE_NOTIFY_V2_LEN {
                return Err(EventDecodeError::BufferTooShort {
                    event: "trace",
                    expected: TRACE_NOTIFY_V2_LEN,
                    actual: data.len(),
                });
            }
            ip_trace_id = u64::from_ne_bytes([
                data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
            ]);
        }

        Ok(Self {
            type_: data[0],
            obs_point: data[1],
            source: u16::from_ne_bytes([data[2], data[3]]),
            hash: u32::from_ne_bytes([data[4], data[5], data[6], data[7]]),
            orig_len: u32::from_ne_bytes([data[8], data[9], data[10], data[11]]),
            cap_len: u16::from_ne_bytes([data[12], data[13]]),
            version,
            ext_version: data[15],
            src_label: u32::from_ne_bytes([data[16], data[17], data[18], data[19]]),
            dst_label: u32::from_ne_bytes([data[20], data[21], data[22], data[23]]),
            dst_id: u16::from_ne_bytes([data[24], data[25]]),
            reason: data[26],
            flags: data[27],
            ifindex: u32::from_ne_bytes([data[28], data[29], data[30], data[31]]),
            orig_ip,
            ip_trace_id,
        })
    }

    /// Returns the offset at which packet data starts for this event.
    #[must_use]
    pub fn data_offset(&self) -> usize {
        match self.version {
            0 => TRACE_NOTIFY_V0_LEN,
            1 => TRACE_NOTIFY_V1_LEN,
            2 => TRACE_NOTIFY_V2_LEN,
            _ => 0,
        }
    }

    /// Returns whether the event marks an encrypted packet.
    #[must_use]
    pub const fn is_encrypted(&self) -> bool {
        (self.reason & TRACE_REASON_ENCRYPT_MASK) != 0
    }

    /// Returns the trace reason with the encryption bit removed.
    #[must_use]
    pub const fn trace_reason(&self) -> u8 {
        self.reason & !TRACE_REASON_ENCRYPT_MASK
    }

    /// Returns whether the event refers to an IPv6 packet.
    #[must_use]
    pub const fn is_ipv6(&self) -> bool {
        (self.flags & TRACE_NOTIFY_FLAG_IS_IPV6) != 0
    }

    /// Returns whether the event refers to an L3 device.
    #[must_use]
    pub const fn is_l3_device(&self) -> bool {
        (self.flags & TRACE_NOTIFY_FLAG_IS_L3_DEVICE) != 0
    }

    /// Returns whether the event refers to a VXLAN packet.
    #[must_use]
    pub const fn is_vxlan(&self) -> bool {
        (self.flags & TRACE_NOTIFY_FLAG_IS_VXLAN) != 0
    }

    /// Returns whether the event refers to a Geneve packet.
    #[must_use]
    pub const fn is_geneve(&self) -> bool {
        (self.flags & TRACE_NOTIFY_FLAG_IS_GENEVE) != 0
    }
}

/// Datapath drop event mirroring Cilium's `drop_notify` layout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropNotify {
    /// Event type discriminator.
    pub type_: u8,
    /// Drop reason subtype.
    pub sub_type: u8,
    /// Source endpoint ID.
    pub source: u16,
    /// Flow hash.
    pub hash: u32,
    /// Original packet length.
    pub orig_len: u32,
    /// Captured packet length.
    pub cap_len: u16,
    /// Layout version.
    pub version: u8,
    /// Extension layout version.
    pub ext_version: u8,
    /// Source identity.
    pub src_label: u32,
    /// Destination identity.
    pub dst_label: u32,
    /// Destination endpoint ID.
    pub dst_id: u32,
    /// Source file line.
    pub line: u16,
    /// Source file identifier.
    pub file: u8,
    /// Extended drop error code.
    pub ext_error: i8,
    /// Kernel interface index.
    pub ifindex: u32,
    /// Bitflags describing the event.
    pub flags: u8,
    /// Optional IP trace identifier.
    pub ip_trace_id: u64,
}

impl DropNotify {
    /// Decodes a drop event from raw datapath bytes.
    pub fn decode(data: &[u8]) -> Result<Self, EventDecodeError> {
        if data.len() < DROP_NOTIFY_V1_LEN {
            return Err(EventDecodeError::BufferTooShort {
                event: "drop",
                expected: DROP_NOTIFY_V1_LEN,
                actual: data.len(),
            });
        }

        let version = data[14];
        if version > DROP_NOTIFY_VERSION3 {
            return Err(EventDecodeError::UnsupportedVersion {
                event: "drop",
                version,
            });
        }

        let mut flags = 0_u8;
        if version >= DROP_NOTIFY_VERSION2 {
            if data.len() < DROP_NOTIFY_V2_LEN {
                return Err(EventDecodeError::BufferTooShort {
                    event: "drop",
                    expected: DROP_NOTIFY_V2_LEN,
                    actual: data.len(),
                });
            }
            flags = data[36];
        }

        let mut ip_trace_id = 0_u64;
        if version >= DROP_NOTIFY_VERSION3 {
            if data.len() < DROP_NOTIFY_V3_LEN {
                return Err(EventDecodeError::BufferTooShort {
                    event: "drop",
                    expected: DROP_NOTIFY_V3_LEN,
                    actual: data.len(),
                });
            }
            ip_trace_id = u64::from_ne_bytes([
                data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
            ]);
        }

        Ok(Self {
            type_: data[0],
            sub_type: data[1],
            source: u16::from_ne_bytes([data[2], data[3]]),
            hash: u32::from_ne_bytes([data[4], data[5], data[6], data[7]]),
            orig_len: u32::from_ne_bytes([data[8], data[9], data[10], data[11]]),
            cap_len: u16::from_ne_bytes([data[12], data[13]]),
            version,
            ext_version: data[15],
            src_label: u32::from_ne_bytes([data[16], data[17], data[18], data[19]]),
            dst_label: u32::from_ne_bytes([data[20], data[21], data[22], data[23]]),
            dst_id: u32::from_ne_bytes([data[24], data[25], data[26], data[27]]),
            line: u16::from_ne_bytes([data[28], data[29]]),
            file: data[30],
            ext_error: i8::from_ne_bytes([data[31]]),
            ifindex: u32::from_ne_bytes([data[32], data[33], data[34], data[35]]),
            flags,
            ip_trace_id,
        })
    }

    /// Returns the offset at which packet data starts for this event.
    #[must_use]
    pub fn data_offset(&self) -> usize {
        match self.version {
            0 | 1 => DROP_NOTIFY_V1_LEN,
            2 => DROP_NOTIFY_V2_LEN,
            3 => DROP_NOTIFY_V3_LEN,
            _ => 0,
        }
    }

    /// Returns whether the event refers to an IPv6 packet.
    #[must_use]
    pub const fn is_ipv6(&self) -> bool {
        (self.flags & DROP_NOTIFY_FLAG_IS_IPV6) != 0
    }

    /// Returns whether the event refers to an L3 device.
    #[must_use]
    pub const fn is_l3_device(&self) -> bool {
        (self.flags & DROP_NOTIFY_FLAG_IS_L3_DEVICE) != 0
    }

    /// Returns whether the event refers to a VXLAN packet.
    #[must_use]
    pub const fn is_vxlan(&self) -> bool {
        (self.flags & DROP_NOTIFY_FLAG_IS_VXLAN) != 0
    }

    /// Returns whether the event refers to a Geneve packet.
    #[must_use]
    pub const fn is_geneve(&self) -> bool {
        (self.flags & DROP_NOTIFY_FLAG_IS_GENEVE) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extend_u16(buf: &mut Vec<u8>, value: u16) {
        buf.extend_from_slice(&value.to_ne_bytes());
    }

    fn extend_u32(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_ne_bytes());
    }

    fn extend_u64(buf: &mut Vec<u8>, value: u64) {
        buf.extend_from_slice(&value.to_ne_bytes());
    }

    #[test]
    fn trace_notify_decodes_v2_layout() {
        let mut raw = Vec::new();
        raw.push(1);
        raw.push(2);
        extend_u16(&mut raw, 0x0304);
        extend_u32(&mut raw, 0x0506_0708);
        extend_u32(&mut raw, 0x090a_0b0c);
        extend_u16(&mut raw, 0x0d0e);
        raw.push(2);
        raw.push(1);
        extend_u32(&mut raw, 0x1112_1314);
        extend_u32(&mut raw, 0x1516_1718);
        extend_u16(&mut raw, 0x191a);
        raw.push(0x1b);
        raw.push(0x0f);
        extend_u32(&mut raw, 0x1d1e_1f20);
        raw.extend_from_slice(&[
            0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e,
            0x2f, 0x30,
        ]);
        extend_u64(&mut raw, 0x2b2c_2d2e_2f30_3132);
        assert_eq!(raw.len(), TRACE_NOTIFY_V2_LEN);

        let decoded = TraceNotify::decode(&raw).expect("trace notify should decode");

        assert_eq!(decoded.type_, 1);
        assert_eq!(decoded.obs_point, 2);
        assert_eq!(decoded.source, 0x0304);
        assert_eq!(decoded.hash, 0x0506_0708);
        assert_eq!(decoded.orig_len, 0x090a_0b0c);
        assert_eq!(decoded.cap_len, 0x0d0e);
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.ext_version, 1);
        assert_eq!(decoded.src_label, 0x1112_1314);
        assert_eq!(decoded.dst_label, 0x1516_1718);
        assert_eq!(decoded.dst_id, 0x191a);
        assert_eq!(decoded.reason, 0x1b);
        assert_eq!(decoded.ifindex, 0x1d1e_1f20);
        assert_eq!(decoded.orig_ip[0], 0x21);
        assert_eq!(decoded.ip_trace_id, 0x2b2c_2d2e_2f30_3132);
        assert!(decoded.is_ipv6());
        assert!(decoded.is_l3_device());
        assert!(decoded.is_vxlan());
        assert!(decoded.is_geneve());
        assert_eq!(decoded.data_offset(), TRACE_NOTIFY_V2_LEN);
    }

    #[test]
    fn drop_notify_decodes_v3_layout() {
        let mut raw = Vec::new();
        raw.push(1);
        raw.push(4);
        extend_u16(&mut raw, 0x0203);
        extend_u32(&mut raw, 0x0405_0607);
        extend_u32(&mut raw, 0x0809_0a0b);
        extend_u16(&mut raw, 0x0c0d);
        raw.push(3);
        raw.push(2);
        extend_u32(&mut raw, 0x1112_1314);
        extend_u32(&mut raw, 0x1516_1718);
        extend_u32(&mut raw, 0x191a_1b1c);
        extend_u16(&mut raw, 0x1d1e);
        raw.push(0x1f);
        raw.push(0x20);
        extend_u32(&mut raw, 0x2122_2324);
        raw.push(0x0f);
        raw.extend_from_slice(&[0, 0, 0]);
        extend_u64(&mut raw, 0x2b2c_2d2e_2f30_3132);
        assert_eq!(raw.len(), DROP_NOTIFY_V3_LEN);

        let decoded = DropNotify::decode(&raw).expect("drop notify should decode");

        assert_eq!(decoded.type_, 1);
        assert_eq!(decoded.sub_type, 4);
        assert_eq!(decoded.source, 0x0203);
        assert_eq!(decoded.hash, 0x0405_0607);
        assert_eq!(decoded.orig_len, 0x0809_0a0b);
        assert_eq!(decoded.cap_len, 0x0c0d);
        assert_eq!(decoded.version, 3);
        assert_eq!(decoded.ext_version, 2);
        assert_eq!(decoded.src_label, 0x1112_1314);
        assert_eq!(decoded.dst_label, 0x1516_1718);
        assert_eq!(decoded.dst_id, 0x191a_1b1c);
        assert_eq!(decoded.line, 0x1d1e);
        assert_eq!(decoded.file, 0x1f);
        assert_eq!(decoded.ext_error, 0x20_i8);
        assert_eq!(decoded.ifindex, 0x2122_2324);
        assert_eq!(decoded.flags, 0x0f);
        assert_eq!(decoded.ip_trace_id, 0x2b2c_2d2e_2f30_3132);
        assert!(decoded.is_ipv6());
        assert!(decoded.is_l3_device());
        assert!(decoded.is_vxlan());
        assert!(decoded.is_geneve());
        assert_eq!(decoded.data_offset(), DROP_NOTIFY_V3_LEN);
    }
}
