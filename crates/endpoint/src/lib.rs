//! Core endpoint types and lifecycle state machine ported from `cilium/pkg/endpoint`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::SystemTime;
use thiserror::Error;
use tracing::debug;

/// A numeric endpoint identifier (16-bit, matching Cilium's endpoint ID space).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EndpointID(pub u16);

/// Endpoint lifecycle state (mirrors the core transitions in `pkg/endpoint`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointState {
    /// Endpoint is being created.
    Creating,
    /// Endpoint has a valid identity and is waiting for policy.
    WaitingForIdentity,
    /// Endpoint is not ready yet.
    NotReady,
    /// Endpoint is waiting for initial policy.
    WaitingToRegenerate,
    /// Endpoint is actively regenerating BPF programs.
    Regenerating,
    /// Endpoint is fully connected and operational.
    Ready,
    /// Endpoint is being disconnected.
    Disconnecting,
    /// Endpoint has been disconnected.
    Disconnected,
    /// Endpoint is in an invalid state.
    Invalid,
}

impl EndpointState {
    /// Returns true if the endpoint can move from the current state to `next`.
    pub const fn can_be_transitioned_to(&self, next: EndpointState) -> bool {
        match self {
            Self::Creating => matches!(next, Self::WaitingForIdentity),
            Self::WaitingForIdentity => {
                matches!(
                    next,
                    Self::NotReady | Self::WaitingToRegenerate | Self::Disconnecting
                )
            }
            Self::NotReady => matches!(next, Self::WaitingToRegenerate | Self::Disconnecting),
            Self::WaitingToRegenerate => {
                matches!(next, Self::Regenerating | Self::Disconnecting)
            }
            Self::Regenerating => {
                matches!(
                    next,
                    Self::Ready | Self::WaitingToRegenerate | Self::Disconnecting
                )
            }
            Self::Ready => matches!(next, Self::WaitingToRegenerate | Self::Disconnecting),
            Self::Disconnecting => matches!(next, Self::Disconnected),
            Self::Disconnected | Self::Invalid => false,
        }
    }

    /// Returns true if the endpoint is in a terminal state.
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Disconnected | Self::Invalid)
    }
}

/// Endpoint addressing information.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointAddressing {
    /// Pod IPv4 address.
    pub ipv4: Option<Ipv4Addr>,
    /// Pod IPv6 address.
    pub ipv6: Option<Ipv6Addr>,
    /// Node name hosting the endpoint.
    pub node_name: String,
    /// Node IP hosting the endpoint.
    pub node_ip: Option<IpAddr>,
}

/// Core endpoint model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    /// Endpoint identifier.
    pub id: EndpointID,
    /// Container runtime identifier.
    pub container_id: String,
    /// Container name.
    pub container_name: String,
    /// Pod name.
    pub pod_name: String,
    /// Pod namespace.
    pub pod_namespace: String,
    /// Lifecycle state.
    pub state: EndpointState,
    /// Endpoint addressing details.
    pub addressing: EndpointAddressing,
    /// Orchestration labels.
    pub labels: HashMap<String, String>,
    /// Numeric security identity.
    pub security_identity: Option<u32>,
    /// Last applied policy revision.
    pub policy_revision: u64,
    /// Last applied BPF program revision.
    pub bpf_prog_revision: u64,
    /// Endpoint creation timestamp.
    pub created_at: SystemTime,
}

impl Endpoint {
    /// Creates a new endpoint with empty metadata in the `Creating` state.
    pub fn new(id: EndpointID) -> Self {
        Self {
            id,
            container_id: String::new(),
            container_name: String::new(),
            pod_name: String::new(),
            pod_namespace: String::new(),
            state: EndpointState::Creating,
            addressing: EndpointAddressing::default(),
            labels: HashMap::new(),
            security_identity: None,
            policy_revision: 0,
            bpf_prog_revision: 0,
            created_at: SystemTime::now(),
        }
    }

    /// Attempts a state transition and returns an error when the transition is invalid.
    pub fn set_state(&mut self, next: EndpointState, reason: &str) -> Result<(), EndpointError> {
        if !self.state.can_be_transitioned_to(next) {
            debug!(
                endpoint_id = self.id.0,
                from = ?self.state,
                to = ?next,
                reason = reason,
                "invalid endpoint state transition"
            );
            return Err(EndpointError::InvalidTransition {
                from: self.state,
                to: next,
                reason: reason.to_owned(),
            });
        }

        debug!(
            endpoint_id = self.id.0,
            from = ?self.state,
            to = ?next,
            reason = reason,
            "endpoint state transition"
        );
        self.state = next;
        Ok(())
    }

    /// Returns true if the endpoint is ready.
    pub fn is_ready(&self) -> bool {
        self.state == EndpointState::Ready
    }

    /// Returns true if the endpoint is disconnecting or already disconnected.
    pub fn is_disconnecting(&self) -> bool {
        matches!(
            self.state,
            EndpointState::Disconnecting | EndpointState::Disconnected
        )
    }

    /// Returns the string key used for Kubernetes lookups: `namespace/podname`.
    pub fn kubernetes_key(&self) -> String {
        format!("{}/{}", self.pod_namespace, self.pod_name)
    }

    /// Returns true when the endpoint policy revision is at least `current_revision`.
    pub const fn is_policy_up_to_date(&self, current_revision: u64) -> bool {
        self.policy_revision >= current_revision
    }

    /// Updates the endpoint security identity and returns true when it changed.
    pub fn set_identity(&mut self, id: u32) -> bool {
        if self.security_identity == Some(id) {
            return false;
        }

        self.security_identity = Some(id);
        true
    }
}

/// Describes why an endpoint needs regeneration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegenerationContext {
    /// Reason for the regeneration.
    pub reason: RegenerationReason,
    /// Policy revision that triggered the regeneration.
    pub policy_revision: u64,
}

/// High-level regeneration reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegenerationReason {
    /// Endpoint was newly created.
    NewlyCreated,
    /// Policy changed.
    PolicyChanged,
    /// Security identity changed.
    IdentityChanged,
    /// Regeneration was forced.
    Forced,
    /// State is being restored from disk.
    RestoreStateFromDisk,
}

/// Endpoint-specific errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EndpointError {
    /// A requested state transition is not valid.
    #[error("invalid state transition from {from:?} to {to:?}: {reason}")]
    InvalidTransition {
        /// State before the transition.
        from: EndpointState,
        /// Requested next state.
        to: EndpointState,
        /// Human-readable transition reason.
        reason: String,
    },
    /// An endpoint lookup failed.
    #[error("endpoint {0:?} not found")]
    NotFound(EndpointID),
    /// An endpoint with the same identifier already exists.
    #[error("endpoint {0:?} already exists")]
    AlreadyExists(EndpointID),
}

/// Tracks endpoints and their lookup indices.
pub struct EndpointManager {
    endpoints: Arc<RwLock<HashMap<EndpointID, Arc<Mutex<Endpoint>>>>>,
    by_container: Arc<RwLock<HashMap<String, EndpointID>>>,
    by_kube_key: Arc<RwLock<HashMap<String, EndpointID>>>,
    next_id: Arc<Mutex<u16>>,
}

impl EndpointManager {
    /// Creates an empty endpoint manager.
    pub fn new() -> Self {
        Self {
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            by_container: Arc::new(RwLock::new(HashMap::new())),
            by_kube_key: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Registers an endpoint under its current ID.
    pub fn add_endpoint(&self, ep: Endpoint) -> Result<(), EndpointError> {
        let id = ep.id;
        let container_id = ep.container_id.clone();
        let kube_key = kube_index_key(&ep);
        let endpoint = Arc::new(Mutex::new(ep));

        let mut endpoints = rwlock_write(&self.endpoints);
        if endpoints.contains_key(&id) {
            return Err(EndpointError::AlreadyExists(id));
        }
        endpoints.insert(id, Arc::clone(&endpoint));
        drop(endpoints);

        if !container_id.is_empty() {
            rwlock_write(&self.by_container).insert(container_id, id);
        }
        if let Some(kube_key) = kube_key {
            rwlock_write(&self.by_kube_key).insert(kube_key, id);
        }

        let mut next_id = mutex_lock(&self.next_id);
        if *next_id <= id.0 {
            *next_id = id.0.saturating_add(1);
        }

        debug!(endpoint_id = id.0, "endpoint registered");
        Ok(())
    }

    /// Removes an endpoint from the manager.
    pub fn remove_endpoint(&self, id: EndpointID) -> Option<Arc<Mutex<Endpoint>>> {
        let endpoint = rwlock_write(&self.endpoints).remove(&id)?;

        let (container_id, kube_key) = {
            let endpoint = mutex_lock(&endpoint);
            (endpoint.container_id.clone(), kube_index_key(&endpoint))
        };

        if !container_id.is_empty() {
            let mut by_container = rwlock_write(&self.by_container);
            if by_container.get(&container_id) == Some(&id) {
                by_container.remove(&container_id);
            }
        }
        if let Some(kube_key) = kube_key {
            let mut by_kube_key = rwlock_write(&self.by_kube_key);
            if by_kube_key.get(&kube_key) == Some(&id) {
                by_kube_key.remove(&kube_key);
            }
        }

        debug!(endpoint_id = id.0, "endpoint removed");
        Some(endpoint)
    }

    /// Returns the endpoint with the provided ID.
    pub fn get_endpoint(&self, id: EndpointID) -> Option<Arc<Mutex<Endpoint>>> {
        rwlock_read(&self.endpoints).get(&id).cloned()
    }

    /// Returns the endpoint associated with a container ID.
    pub fn get_by_container_id(&self, id: &str) -> Option<Arc<Mutex<Endpoint>>> {
        let endpoint_id = rwlock_read(&self.by_container).get(id).copied()?;
        self.get_endpoint(endpoint_id)
    }

    /// Returns the endpoint associated with a Kubernetes namespace/pod key.
    pub fn get_by_kube_key(&self, ns: &str, pod: &str) -> Option<Arc<Mutex<Endpoint>>> {
        let endpoint_id = rwlock_read(&self.by_kube_key)
            .get(&format!("{ns}/{pod}"))
            .copied()?;
        self.get_endpoint(endpoint_id)
    }

    /// Returns the number of managed endpoints.
    pub fn count(&self) -> usize {
        rwlock_read(&self.endpoints).len()
    }

    /// Returns the IDs of endpoints currently in the `Ready` state.
    pub fn ready_endpoints(&self) -> Vec<EndpointID> {
        let endpoints: Vec<_> = rwlock_read(&self.endpoints).values().cloned().collect();
        endpoints
            .into_iter()
            .filter_map(|endpoint| {
                let endpoint = mutex_lock(&endpoint);
                endpoint.is_ready().then_some(endpoint.id)
            })
            .collect()
    }

    /// Applies `f` to every managed endpoint using a stable snapshot of the registry.
    pub fn for_each<F: FnMut(&Endpoint)>(&self, mut f: F) {
        let endpoints: Vec<_> = rwlock_read(&self.endpoints).values().cloned().collect();
        for endpoint in endpoints {
            let endpoint = mutex_lock(&endpoint);
            f(&endpoint);
        }
    }
}

impl Default for EndpointManager {
    fn default() -> Self {
        Self::new()
    }
}

fn mutex_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn rwlock_read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn rwlock_write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn kube_index_key(endpoint: &Endpoint) -> Option<String> {
    if endpoint.pod_namespace.is_empty() || endpoint.pod_name.is_empty() {
        None
    } else {
        Some(endpoint.kubernetes_key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_state_transitions() {
        let mut ep = Endpoint::new(EndpointID(1));
        assert_eq!(ep.state, EndpointState::Creating);
        ep.set_state(EndpointState::WaitingForIdentity, "init")
            .unwrap();
        ep.set_state(EndpointState::WaitingToRegenerate, "got identity")
            .unwrap();
        ep.set_state(EndpointState::Regenerating, "start regen")
            .unwrap();
        ep.set_state(EndpointState::Ready, "done").unwrap();
        assert!(ep.is_ready());
    }

    #[test]
    fn test_invalid_state_transition_returns_error() {
        let mut ep = Endpoint::new(EndpointID(1));
        let err = ep
            .set_state(EndpointState::Ready, "skip ahead")
            .unwrap_err();
        assert!(matches!(err, EndpointError::InvalidTransition { .. }));
    }

    #[test]
    fn test_endpoint_manager_add_get_remove() {
        let mgr = EndpointManager::new();
        let ep = Endpoint::new(EndpointID(42));
        mgr.add_endpoint(ep).unwrap();
        assert_eq!(mgr.count(), 1);
        assert!(mgr.get_endpoint(EndpointID(42)).is_some());
        mgr.remove_endpoint(EndpointID(42));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_endpoint_kubernetes_key() {
        let mut ep = Endpoint::new(EndpointID(1));
        ep.pod_namespace = "kube-system".into();
        ep.pod_name = "cilium-abc".into();
        assert_eq!(ep.kubernetes_key(), "kube-system/cilium-abc");
    }

    #[test]
    fn test_set_identity_change_detection() {
        let mut ep = Endpoint::new(EndpointID(1));
        assert!(ep.set_identity(100));
        assert!(!ep.set_identity(100));
        assert!(ep.set_identity(200));
    }

    #[test]
    fn test_ready_endpoints_filter() {
        let mgr = EndpointManager::new();

        let mut ready = Endpoint::new(EndpointID(1));
        ready
            .set_state(EndpointState::WaitingForIdentity, "init")
            .unwrap();
        ready
            .set_state(EndpointState::WaitingToRegenerate, "policy ready")
            .unwrap();
        ready
            .set_state(EndpointState::Regenerating, "start regen")
            .unwrap();
        ready.set_state(EndpointState::Ready, "done").unwrap();

        let mut pending = Endpoint::new(EndpointID(2));
        pending
            .set_state(EndpointState::WaitingForIdentity, "init")
            .unwrap();
        pending
            .set_state(EndpointState::NotReady, "missing policy")
            .unwrap();

        mgr.add_endpoint(ready).unwrap();
        mgr.add_endpoint(pending).unwrap();

        assert_eq!(mgr.ready_endpoints(), vec![EndpointID(1)]);
    }

    #[test]
    fn test_manager_indices_track_container_and_kube_key() {
        let mgr = EndpointManager::new();
        let mut ep = Endpoint::new(EndpointID(7));
        ep.container_id = "container-7".into();
        ep.pod_namespace = "default".into();
        ep.pod_name = "pod-7".into();

        mgr.add_endpoint(ep).unwrap();
        assert!(mgr.get_by_container_id("container-7").is_some());
        assert!(mgr.get_by_kube_key("default", "pod-7").is_some());

        mgr.remove_endpoint(EndpointID(7));
        assert!(mgr.get_by_container_id("container-7").is_none());
        assert!(mgr.get_by_kube_key("default", "pod-7").is_none());
    }

    #[test]
    fn test_for_each_iterates_snapshot() {
        let mgr = EndpointManager::new();
        mgr.add_endpoint(Endpoint::new(EndpointID(1))).unwrap();
        mgr.add_endpoint(Endpoint::new(EndpointID(2))).unwrap();

        let mut seen = Vec::new();
        mgr.for_each(|endpoint| seen.push(endpoint.id));
        seen.sort();

        assert_eq!(seen, vec![EndpointID(1), EndpointID(2)]);
    }
}
