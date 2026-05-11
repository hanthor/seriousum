// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Error types for egress gateway operations

use std::io;
use thiserror::Error;

/// Error type for egress gateway operations
#[derive(Debug, Error)]
pub enum Error {
    /// Policy parsing or validation error
    #[error("policy error: {0}")]
    PolicyError(String),

    /// Endpoint metadata extraction failed
    #[error("endpoint error: {0}")]
    EndpointError(String),

    /// Invalid gateway configuration
    #[error("gateway config error: {0}")]
    GatewayConfigError(String),

    /// Node selection failed
    #[error("node selection error: {0}")]
    NodeSelectionError(String),

    /// Label matching error
    #[error("label matching error: {0}")]
    LabelMatchError(String),

    /// Identity lookup failed
    #[error("identity lookup failed: {0}")]
    IdentityLookupError(String),

    /// BPF map operation failed
    #[error("bpf map error: {0}")]
    BpfMapError(String),

    /// IO error
    #[error("io error: {0}")]
    IoError(#[from] io::Error),

    /// Invalid CIDR
    #[error("invalid cidr: {0}")]
    InvalidCidr(String),

    /// Invalid IP address
    #[error("invalid ip address: {0}")]
    InvalidIpAddress(String),

    /// Reconciliation error
    #[error("reconciliation error: {0}")]
    ReconciliationError(String),

    /// Generic other error
    #[error("error: {0}")]
    Other(String),
}

/// Result type for egress gateway operations
pub type Result<T> = std::result::Result<T, Error>;
