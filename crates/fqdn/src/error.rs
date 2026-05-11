//! Error handling for FQDN subsystem

use std::io;
use std::net::AddrParseError;
use thiserror::Error;

/// Result type for FQDN operations
pub type Result<T> = std::result::Result<T, Error>;

/// FQDN subsystem errors
#[derive(Debug, Error)]
pub enum Error {
    /// DNS parsing error
    #[error("dns parse error: {0}")]
    DnsParse(String),

    /// Invalid DNS query
    #[error("invalid dns query: {0}")]
    InvalidQuery(String),

    /// Cache operation error
    #[error("cache error: {0}")]
    CacheError(String),

    /// Policy enforcement error
    #[error("policy error: {0}")]
    PolicyError(String),

    /// Invalid FQDN format
    #[error("invalid fqdn: {0}")]
    InvalidFqdn(String),

    /// CIDR parsing error
    #[error("invalid cidr: {0}")]
    InvalidCidr(String),

    /// Network I/O error
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// Address parsing error
    #[error("address parse error: {0}")]
    AddrParse(#[from] AddrParseError),

    /// Other error
    #[error("fqdn error: {0}")]
    Other(String),
}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error::Other(msg)
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Error::Other(msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_from_string() {
        let err: Error = "test error".into();
        assert_eq!(err.to_string(), "fqdn error: test error");
    }

    #[test]
    fn error_dns_parse() {
        let err = Error::DnsParse("malformed packet".to_string());
        assert!(err.to_string().contains("dns parse error"));
    }
}
