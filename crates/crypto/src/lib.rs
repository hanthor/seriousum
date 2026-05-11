//! Crypto helpers for seriousum.

use std::fmt;

use ring::digest::{SHA256, digest};

use serde::{Deserialize, Serialize};

/// A SHA-256 fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Creates a fingerprint from raw bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw fingerprint bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Computes a SHA-256 fingerprint.
    pub fn sha256(data: &[u8]) -> Self {
        let d = digest(&SHA256, data);
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(d.as_ref());
        Self(bytes)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A symmetric key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymmetricKey(Vec<u8>);

impl SymmetricKey {
    /// Creates a new symmetric key.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Returns the key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the key fingerprint.
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::sha256(&self.0)
    }
}

/// A lightweight keypair placeholder used by higher-level components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPair {
    /// Public key bytes.
    pub public: Vec<u8>,
    /// Private key bytes.
    pub private: Vec<u8>,
}

impl KeyPair {
    /// Creates a new keypair from raw bytes.
    pub fn new(public: impl Into<Vec<u8>>, private: impl Into<Vec<u8>>) -> Self {
        Self {
            public: public.into(),
            private: private.into(),
        }
    }

    /// Returns a fingerprint of the public key.
    pub fn public_fingerprint(&self) -> Fingerprint {
        Fingerprint::sha256(&self.public)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable() {
        let fp = Fingerprint::sha256(b"hello");
        assert_eq!(fp.as_bytes().len(), 32);
    }

    #[test]
    fn keypair_works() {
        let kp = KeyPair::new([1, 2, 3], [4, 5, 6]);
        assert_eq!(kp.public_fingerprint().as_bytes().len(), 32);
        assert_eq!(kp.private, vec![4, 5, 6]);
    }
}
