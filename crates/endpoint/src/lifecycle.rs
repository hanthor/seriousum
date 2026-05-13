//! Endpoint lifecycle management — ported from cilium/pkg/endpoint
//!
//! Implements the full endpoint lifecycle from creation through regeneration to disconnection.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

/// Endpoint lifecycle state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EndpointState {
    /// Endpoint is being created.
    Creating,

    /// Waiting for identity assignment from kvstore.
    WaitingForIdentity,

    /// Waiting to be regenerated (policy has changed).
    WaitingToRegenerate,

    /// Currently regenerating (compiling policy and eBPF).
    Regenerating,

    /// Endpoint is ready to forward traffic.
    Ready,

    /// Endpoint is being disconnected.
    Disconnecting,

    /// Endpoint has been disconnected.
    Disconnected,

    /// Endpoint creation failed.
    Invalid,
}

impl std::fmt::Display for EndpointState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::WaitingForIdentity => write!(f, "waiting-for-identity"),
            Self::WaitingToRegenerate => write!(f, "waiting-to-regenerate"),
            Self::Regenerating => write!(f, "regenerating"),
            Self::Ready => write!(f, "ready"),
            Self::Disconnecting => write!(f, "disconnecting"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Invalid => write!(f, "invalid"),
        }
    }
}

impl EndpointState {
    /// Is this state terminal (endpoint is gone)?
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Disconnected | Self::Invalid)
    }

    /// Is this state ready for traffic?
    pub fn is_ready(self) -> bool {
        self == Self::Ready
    }

    /// Is this state waiting for something?
    pub fn is_waiting(self) -> bool {
        matches!(self, Self::WaitingForIdentity | Self::WaitingToRegenerate)
    }
}

/// Endpoint metadata for creation.
#[derive(Debug, Clone)]
pub struct EndpointMetadata {
    /// Unique endpoint ID on this node.
    pub id: u16,

    /// Container ID.
    pub container_id: String,

    /// Pod name (Kubernetes).
    pub pod_name: String,

    /// Namespace.
    pub namespace: String,

    /// IPv4 address.
    pub ipv4: Option<IpAddr>,

    /// IPv6 address.
    pub ipv6: Option<IpAddr>,

    /// Security identity (from pod labels).
    pub security_identity: u32,

    /// Pod labels.
    pub labels: HashMap<String, String>,

    /// Interface name (veth host side).
    pub interface: String,

    /// Creation timestamp (Unix seconds).
    pub created_at: u64,
}

impl EndpointMetadata {
    /// Create a new endpoint metadata.
    pub fn new(
        id: u16,
        container_id: impl Into<String>,
        pod_name: impl Into<String>,
        namespace: impl Into<String>,
    ) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            container_id: container_id.into(),
            pod_name: pod_name.into(),
            namespace: namespace.into(),
            ipv4: None,
            ipv6: None,
            security_identity: 0,
            labels: HashMap::new(),
            interface: String::new(),
            created_at,
        }
    }

    pub fn with_ipv4(mut self, addr: IpAddr) -> Self {
        self.ipv4 = Some(addr);
        self
    }

    pub fn with_ipv6(mut self, addr: IpAddr) -> Self {
        self.ipv6 = Some(addr);
        self
    }

    pub fn with_identity(mut self, identity: u32) -> Self {
        self.security_identity = identity;
        self
    }

    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels = labels;
        self
    }

    pub fn with_interface(mut self, interface: impl Into<String>) -> Self {
        self.interface = interface.into();
        self
    }
}

/// Regeneration metadata.
#[derive(Debug, Clone)]
pub struct RegenerationMetadata {
    /// Reason for regeneration.
    pub reason: RegenerationReason,

    /// Optional message.
    pub message: Option<String>,

    /// Timestamp (Unix seconds).
    pub timestamp: u64,
}

/// Reasons for regeneration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegenerationReason {
    /// Policy rules changed.
    PolicyUpdate,

    /// Identity assignment changed.
    IdentityUpdate,

    /// Manual regeneration requested.
    Manual,

    /// Periodic background regeneration.
    Periodic,

    /// Endpoint address changed.
    AddressUpdate,
}

impl std::fmt::Display for RegenerationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PolicyUpdate => write!(f, "policy-update"),
            Self::IdentityUpdate => write!(f, "identity-update"),
            Self::Manual => write!(f, "manual"),
            Self::Periodic => write!(f, "periodic"),
            Self::AddressUpdate => write!(f, "address-update"),
        }
    }
}

impl RegenerationMetadata {
    pub fn new(reason: RegenerationReason) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            reason,
            message: None,
            timestamp,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Endpoint lifecycle state machine.
#[derive(Debug, Clone)]
pub struct EndpointLifecycle {
    /// Current state.
    state: EndpointState,

    /// Last state change timestamp.
    last_state_change: u64,

    /// Number of regenerations.
    regeneration_count: u32,

    /// Last regeneration metadata.
    last_regeneration: Option<RegenerationMetadata>,

    /// Error message if state is Invalid.
    error_message: Option<String>,
}

impl Default for EndpointLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl EndpointLifecycle {
    /// Create a new lifecycle in Creating state.
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            state: EndpointState::Creating,
            last_state_change: timestamp,
            regeneration_count: 0,
            last_regeneration: None,
            error_message: None,
        }
    }

    /// Get current state.
    pub fn state(&self) -> EndpointState {
        self.state
    }

    /// Transition to a new state (validates state machine).
    pub fn transition(&mut self, new_state: EndpointState) -> Result<(), String> {
        // Define valid transitions
        let valid = match self.state {
            EndpointState::Creating => matches!(
                new_state,
                EndpointState::WaitingForIdentity | EndpointState::Invalid
            ),
            EndpointState::WaitingForIdentity => matches!(
                new_state,
                EndpointState::Ready | EndpointState::WaitingToRegenerate | EndpointState::Invalid
            ),
            EndpointState::WaitingToRegenerate => {
                matches!(
                    new_state,
                    EndpointState::Regenerating | EndpointState::Disconnecting
                )
            }
            EndpointState::Regenerating => {
                matches!(
                    new_state,
                    EndpointState::Ready
                        | EndpointState::WaitingToRegenerate
                        | EndpointState::Disconnecting
                )
            }
            EndpointState::Ready => {
                matches!(
                    new_state,
                    EndpointState::WaitingToRegenerate
                        | EndpointState::Regenerating
                        | EndpointState::Disconnecting
                )
            }
            EndpointState::Disconnecting => matches!(new_state, EndpointState::Disconnected),
            EndpointState::Disconnected | EndpointState::Invalid => {
                // Terminal states: no transitions
                false
            }
        };

        if !valid {
            return Err(format!(
                "invalid state transition: {} → {}",
                self.state, new_state
            ));
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        debug!("Endpoint state transition: {} → {}", self.state, new_state);
        self.state = new_state;
        self.last_state_change = timestamp;

        Ok(())
    }

    /// Mark endpoint as invalid with error message.
    pub fn mark_invalid(&mut self, error: impl Into<String>) {
        self.state = EndpointState::Invalid;
        self.error_message = Some(error.into());
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_state_change = timestamp;
    }

    /// Record a successful regeneration.
    pub fn regeneration_started(&mut self) -> Result<(), String> {
        self.transition(EndpointState::Regenerating)
    }

    /// Mark regeneration as complete.
    pub fn regeneration_complete(&mut self, metadata: RegenerationMetadata) -> Result<(), String> {
        self.regeneration_count = self.regeneration_count.saturating_add(1);
        self.last_regeneration = Some(metadata);
        self.transition(EndpointState::Ready)
    }

    /// Trigger regeneration request.
    pub fn request_regeneration(&mut self) -> Result<(), String> {
        if self.state == EndpointState::Ready {
            self.transition(EndpointState::WaitingToRegenerate)
        } else {
            Ok(())
        }
    }

    /// Get statistics about endpoint lifetime.
    pub fn stats(&self) -> EndpointStats {
        EndpointStats {
            state: self.state,
            regeneration_count: self.regeneration_count,
            last_state_change: self.last_state_change,
            last_regeneration: self.last_regeneration.clone(),
            is_terminal: self.state.is_terminal(),
            error: self.error_message.clone(),
        }
    }
}

/// Endpoint statistics.
#[derive(Debug, Clone)]
pub struct EndpointStats {
    pub state: EndpointState,
    pub regeneration_count: u32,
    pub last_state_change: u64,
    pub last_regeneration: Option<RegenerationMetadata>,
    pub is_terminal: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_state_display() {
        assert_eq!(EndpointState::Creating.to_string(), "creating");
        assert_eq!(EndpointState::Ready.to_string(), "ready");
        assert_eq!(EndpointState::Disconnected.to_string(), "disconnected");
    }

    #[test]
    fn test_endpoint_state_properties() {
        assert!(!EndpointState::Creating.is_terminal());
        assert!(EndpointState::Disconnected.is_terminal());
        assert!(EndpointState::Ready.is_ready());
        assert!(!EndpointState::Creating.is_ready());
        assert!(EndpointState::WaitingForIdentity.is_waiting());
    }

    #[test]
    fn test_endpoint_metadata_creation() {
        let meta = EndpointMetadata::new(0, "cont123", "pod-web-1", "default");
        assert_eq!(meta.id, 0);
        assert_eq!(meta.pod_name, "pod-web-1");
        assert_eq!(meta.namespace, "default");
    }

    #[test]
    fn test_endpoint_metadata_builder() {
        let meta = EndpointMetadata::new(1, "cont123", "pod-web-1", "default")
            .with_identity(42)
            .with_interface("eth0");

        assert_eq!(meta.security_identity, 42);
        assert_eq!(meta.interface, "eth0");
    }

    #[test]
    fn test_lifecycle_initial_state() {
        let lifecycle = EndpointLifecycle::new();
        assert_eq!(lifecycle.state(), EndpointState::Creating);
        assert!(!lifecycle.state().is_terminal());
    }

    #[test]
    fn test_lifecycle_valid_transition_creating_to_waiting() {
        let mut lifecycle = EndpointLifecycle::new();
        assert!(
            lifecycle
                .transition(EndpointState::WaitingForIdentity)
                .is_ok()
        );
        assert_eq!(lifecycle.state(), EndpointState::WaitingForIdentity);
    }

    #[test]
    fn test_lifecycle_valid_transition_waiting_to_ready() {
        let mut lifecycle = EndpointLifecycle::new();
        lifecycle
            .transition(EndpointState::WaitingForIdentity)
            .unwrap();
        assert!(lifecycle.transition(EndpointState::Ready).is_ok());
        assert_eq!(lifecycle.state(), EndpointState::Ready);
    }

    #[test]
    fn test_lifecycle_invalid_transition() {
        let mut lifecycle = EndpointLifecycle::new();
        lifecycle.transition(EndpointState::Ready).unwrap_err();
        // Creating can only go to WaitingForIdentity or Invalid
        assert_eq!(lifecycle.state(), EndpointState::Creating);
    }

    #[test]
    fn test_lifecycle_invalid_state_no_transitions() {
        let mut lifecycle = EndpointLifecycle::new();
        lifecycle.mark_invalid("test error");
        assert_eq!(lifecycle.state(), EndpointState::Invalid);
        assert!(lifecycle.transition(EndpointState::Ready).is_err());
    }

    #[test]
    fn test_lifecycle_regeneration_complete() {
        let mut lifecycle = EndpointLifecycle::new();
        lifecycle
            .transition(EndpointState::WaitingForIdentity)
            .unwrap();
        lifecycle.transition(EndpointState::Ready).unwrap();
        lifecycle.regeneration_started().unwrap();

        let metadata = RegenerationMetadata::new(RegenerationReason::PolicyUpdate)
            .with_message("policy changed");
        lifecycle.regeneration_complete(metadata).unwrap();

        assert_eq!(lifecycle.state(), EndpointState::Ready);
        assert_eq!(lifecycle.regeneration_count, 1);
    }

    #[test]
    fn test_lifecycle_stats() {
        let mut lifecycle = EndpointLifecycle::new();
        lifecycle
            .transition(EndpointState::WaitingForIdentity)
            .unwrap();
        lifecycle.transition(EndpointState::Ready).unwrap();

        let stats = lifecycle.stats();
        assert_eq!(stats.state, EndpointState::Ready);
        assert_eq!(stats.regeneration_count, 0);
        assert!(!stats.is_terminal);
        assert!(stats.error.is_none());
    }

    #[test]
    fn test_regeneration_reason_display() {
        assert_eq!(
            RegenerationReason::PolicyUpdate.to_string(),
            "policy-update"
        );
        assert_eq!(RegenerationReason::Manual.to_string(), "manual");
    }

    #[test]
    fn test_regeneration_metadata_builder() {
        let meta = RegenerationMetadata::new(RegenerationReason::PolicyUpdate)
            .with_message("policy rules changed");
        assert_eq!(meta.reason, RegenerationReason::PolicyUpdate);
        assert_eq!(meta.message.as_deref(), Some("policy rules changed"));
    }
}
