// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Egress Gateway module
//!
//! This module defines an internal representation of the Cilium Egress Gateway Policy.
//! The structures are managed by the [Manager].
//!
//! The egress gateway feature allows originating traffic from specific IPv4 addresses,
//! enabling fine-grained control over egress traffic from endpoints. It supports:
//!
//! - **Policy Management**: Parse and manage CiliumEgressGatewayPolicy resources
//! - **Node Selection**: Select which nodes act as egress gateways based on label selectors
//! - **Endpoint Matching**: Match endpoints based on pod/namespace selectors
//! - **Traffic Redirection**: Redirect traffic to specific gateways using BPF policies
//! - **Multi-gateway Load Distribution**: Distribute endpoints across multiple gateways
//!
//! ## Architecture
//!
//! The module consists of several key components:
//!
//! 1. **Manager**: Orchestrates policies, endpoints, and nodes; triggers reconciliation
//! 2. **PolicyConfig**: Represents parsed egress policies with selectors and CIDR ranges
//! 3. **GatewayConfig**: Runtime gateway configuration (interface, IP addresses, gateway IP)
//! 4. **EndpointMetadata**: Stores endpoint labels, IPs, and node information
//! 5. **Event Handlers**: Process K8s resource events (policies, endpoints, nodes)
//! 6. **BPF Map Updates**: Synchronize policy rules to datapath BPF maps

#![deny(unsafe_code, unused_imports)]
#![warn(missing_docs)]

pub mod error;
pub mod endpoint;
pub mod event;
pub mod gateway;
pub mod manager;
pub mod policy;
pub mod reconcile;
pub mod types;

// Re-export key types
pub use error::{Error, Result};
pub use endpoint::EndpointMetadata;
pub use gateway::GatewayConfig;
pub use manager::Manager;
pub use policy::PolicyConfig;
pub use types::*;

/// Configuration for the egress gateway module
#[derive(Debug, Clone)]
pub struct Config {
    /// Interval between reconciliation triggers
    pub reconciliation_trigger_interval: std::time::Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            reconciliation_trigger_interval: std::time::Duration::from_secs(1),
        }
    }
}
