use serde::{Deserialize, Serialize};
use thiserror::Error;

const PAYLOAD_LENGTH_PREFIX_LEN: usize = std::mem::size_of::<u32>();
const PAYLOAD_FIXED_BODY_LEN: usize =
    std::mem::size_of::<u8>() + std::mem::size_of::<i32>() + std::mem::size_of::<u64>();
const PAYLOAD_HEADER_LEN: usize = PAYLOAD_LENGTH_PREFIX_LEN + PAYLOAD_FIXED_BODY_LEN;

/// The top-level monitor payload kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PayloadType {
    /// A sampled datapath or agent event.
    EventSample = 0,
    /// A lost-record notification.
    RecordLost = 1,
}

impl PayloadType {
    fn from_u8(value: u8) -> Result<Self, PayloadError> {
        match value {
            0 => Ok(Self::EventSample),
            1 => Ok(Self::RecordLost),
            other => Err(PayloadError::UnknownPayloadType(other)),
        }
    }
}

/// Errors returned while decoding a monitor payload.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// The input buffer is too short to contain a full payload header.
    #[error("payload buffer too short: expected at least {expected} bytes, got {actual}")]
    BufferTooShort {
        /// The minimum payload header size.
        expected: usize,
        /// The number of bytes that were available.
        actual: usize,
    },
    /// The payload type byte is not recognized.
    #[error("unknown payload type {0}")]
    UnknownPayloadType(u8),
    /// The encoded payload length does not match the actual buffer size.
    #[error("payload length mismatch: expected {expected} bytes, got {actual}")]
    LengthMismatch {
        /// The total encoded length implied by the payload header.
        expected: usize,
        /// The actual buffer length.
        actual: usize,
    },
}

/// The monitor payload envelope used to ship sampled events and lost-record notifications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    /// Raw event bytes for sampled events.
    pub data: Vec<u8>,
    /// CPU index that produced the event.
    pub cpu: i32,
    /// Number of lost records represented by this payload.
    pub lost: u64,
    /// The payload discriminator.
    pub payload_type: PayloadType,
}

impl Payload {
    /// Creates an event payload.
    #[must_use]
    pub fn new_event(data: Vec<u8>, cpu: i32) -> Self {
        Self {
            data,
            cpu,
            lost: 0,
            payload_type: PayloadType::EventSample,
        }
    }

    /// Creates a lost-record payload.
    #[must_use]
    pub fn new_lost(lost: u64, cpu: i32) -> Self {
        Self {
            data: Vec::new(),
            cpu,
            lost,
            payload_type: PayloadType::RecordLost,
        }
    }

    /// Encodes the payload into a native-endian length-prefixed binary envelope.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let Ok(body_len) = u32::try_from(PAYLOAD_FIXED_BODY_LEN + self.data.len()) else {
            tracing::debug!(
                data_len = self.data.len(),
                "monitor payload too large to encode"
            );
            return Vec::new();
        };

        let mut encoded = Vec::with_capacity(PAYLOAD_LENGTH_PREFIX_LEN + body_len as usize);
        encoded.extend_from_slice(&body_len.to_ne_bytes());
        encoded.push(self.payload_type as u8);
        encoded.extend_from_slice(&self.cpu.to_ne_bytes());
        encoded.extend_from_slice(&self.lost.to_ne_bytes());
        encoded.extend_from_slice(&self.data);
        encoded
    }

    /// Decodes a payload from the native-endian binary envelope produced by [`Self::encode`].
    pub fn decode(buf: &[u8]) -> Result<Self, PayloadError> {
        if buf.len() < PAYLOAD_HEADER_LEN {
            return Err(PayloadError::BufferTooShort {
                expected: PAYLOAD_HEADER_LEN,
                actual: buf.len(),
            });
        }

        let body_len = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let expected = std::mem::size_of::<u32>() + body_len;
        if buf.len() != expected {
            return Err(PayloadError::LengthMismatch {
                expected,
                actual: buf.len(),
            });
        }

        Ok(Self {
            payload_type: PayloadType::from_u8(buf[4])?,
            cpu: i32::from_ne_bytes([buf[5], buf[6], buf[7], buf[8]]),
            lost: u64::from_ne_bytes([
                buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15], buf[16],
            ]),
            data: buf[PAYLOAD_HEADER_LEN..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_event_roundtrip() {
        let payload = Payload::new_event(vec![1, 2, 3, 4], 7);
        let decoded = Payload::decode(&payload.encode());

        assert_eq!(decoded, Ok(payload));
    }

    #[test]
    fn payload_lost_roundtrip() {
        let payload = Payload::new_lost(99, 3);
        let decoded = Payload::decode(&payload.encode());

        assert_eq!(decoded, Ok(payload));
    }
}
