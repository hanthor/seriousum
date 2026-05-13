//! Policy error types.

use thiserror::Error;

/// Result type used throughout the policy crate.
pub type Result<T> = std::result::Result<T, PolicyError>;

/// Error type for policy operations.
#[derive(Debug, Error)]
pub enum PolicyError {
    /// The requested policy object could not be found.
    #[error("policy not found: {0}")]
    NotFound(String),

    /// The provided rule is invalid.
    #[error("invalid rule: {0}")]
    InvalidRule(String),

    /// The provided selector is invalid.
    #[error("invalid selector: {0}")]
    InvalidSelector(String),

    /// The provided L4 policy is invalid.
    #[error("invalid L4 policy: {0}")]
    InvalidL4Policy(String),

    /// The provided CIDR policy is invalid.
    #[error("invalid CIDR policy: {0}")]
    InvalidCidr(String),

    /// Shared policy state could not be accessed.
    #[error("concurrent modification")]
    ConcurrentModification,
}
