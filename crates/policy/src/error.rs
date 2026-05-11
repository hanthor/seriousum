//! Policy error types

use thiserror::Error;

/// Error type for policy operations.
#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("policy not found: {0}")]
    NotFound(String),

    #[error("invalid rule: {0}")]
    InvalidRule(String),

    #[error("invalid selector: {0}")]
    InvalidSelector(String),

    #[error("invalid L4 policy: {0}")]
    InvalidL4Policy(String),

    #[error("identity not found: {0}")]
    IdentityNotFound(u32),

    #[error("selector cache error: {0}")]
    SelectorCacheError(String),

    #[error("policy compilation error: {0}")]
    CompilationError(String),

    #[error("eBPF map error: {0}")]
    EbpfMapError(String),

    #[error("concurrent modification")]
    ConcurrentModification,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PolicyError>;
