//! Endpoint manager — CRUD operations and lifecycle coordination
//!
//! Manages the collection of endpoints on this node and coordinates
//! regeneration, policy updates, and cleanup.

use crate::lifecycle::{
    EndpointLifecycle, EndpointMetadata, EndpointState, RegenerationMetadata,
    RegenerationReason,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Endpoint manager error type.
#[derive(Debug, Clone)]
pub enum ManagerError {
    EndpointNotFound(u16),
    EndpointAlreadyExists(u16),
    InvalidStateTransition(String),
    RegenerationFailed(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EndpointNotFound(id) => write!(f, "endpoint not found: {}", id),
            Self::EndpointAlreadyExists(id) => write!(f, "endpoint already exists: {}", id),
            Self::InvalidStateTransition(msg) => write!(f, "invalid state transition: {}", msg),
            Self::RegenerationFailed(msg) => write!(f, "regeneration failed: {}", msg),
        }
    }
}

pub type ManagerResult<T> = Result<T, ManagerError>;

/// Single managed endpoint.
#[derive(Debug, Clone)]
pub struct ManagedEndpoint {
    /// Metadata.
    pub metadata: EndpointMetadata,

    /// Lifecycle state machine.
    pub lifecycle: EndpointLifecycle,

    /// Pending policy (not yet applied to eBPF).
    pub pending_policy: Option<String>,
}

impl ManagedEndpoint {
    pub fn new(metadata: EndpointMetadata) -> Self {
        Self {
            metadata,
            lifecycle: EndpointLifecycle::new(),
            pending_policy: None,
        }
    }

    /// Get current state.
    pub fn state(&self) -> EndpointState {
        self.lifecycle.state()
    }
}

/// Endpoint manager coordinates lifecycle and regeneration.
pub struct EndpointManager {
    // Map of endpoint ID → managed endpoint
    endpoints: Arc<RwLock<HashMap<u16, ManagedEndpoint>>>,

    // Statistics
    stats: Arc<RwLock<ManagerStats>>,
}

/// Manager statistics.
#[derive(Debug, Clone, Default)]
pub struct ManagerStats {
    pub total_created: u64,
    pub total_deleted: u64,
    pub total_regenerations: u64,
    pub currently_ready: u32,
    pub currently_regenerating: u32,
    pub failed_regenerations: u64,
}

impl EndpointManager {
    /// Create a new endpoint manager.
    pub fn new() -> Self {
        Self {
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(ManagerStats::default())),
        }
    }

    /// Create a new endpoint.
    pub async fn create(&self, metadata: EndpointMetadata) -> ManagerResult<()> {
        let ep_id = metadata.id;
        let mut eps = self.endpoints.write().await;

        if eps.contains_key(&ep_id) {
            return Err(ManagerError::EndpointAlreadyExists(ep_id));
        }

        let ep = ManagedEndpoint::new(metadata);
        eps.insert(ep_id, ep);

        let mut stats = self.stats.write().await;
        stats.total_created += 1;

        info!("Created endpoint {}", ep_id);
        Ok(())
    }

    /// Get endpoint by ID.
    pub async fn get(&self, id: u16) -> ManagerResult<ManagedEndpoint> {
        let eps = self.endpoints.read().await;
        eps.get(&id)
            .cloned()
            .ok_or(ManagerError::EndpointNotFound(id))
    }

    /// List all endpoints.
    pub async fn list(&self) -> Vec<ManagedEndpoint> {
        let eps = self.endpoints.read().await;
        eps.values().cloned().collect()
    }

    /// Delete an endpoint.
    pub async fn delete(&self, id: u16) -> ManagerResult<ManagedEndpoint> {
        let mut eps = self.endpoints.write().await;
        let ep = eps
            .remove(&id)
            .ok_or(ManagerError::EndpointNotFound(id))?;

        let mut stats = self.stats.write().await;
        stats.total_deleted += 1;

        info!("Deleted endpoint {}", id);
        Ok(ep)
    }

    /// Mark endpoint as waiting for identity.
    pub async fn mark_waiting_for_identity(&self, id: u16) -> ManagerResult<()> {
        let mut eps = self.endpoints.write().await;
        let ep = eps
            .get_mut(&id)
            .ok_or(ManagerError::EndpointNotFound(id))?;

        ep.lifecycle
            .transition(EndpointState::WaitingForIdentity)
            .map_err(|e| ManagerError::InvalidStateTransition(e))?;

        debug!("Endpoint {} now waiting for identity", id);
        Ok(())
    }

    /// Mark endpoint as ready.
    pub async fn mark_ready(&self, id: u16) -> ManagerResult<()> {
        let mut eps = self.endpoints.write().await;
        let ep = eps
            .get_mut(&id)
            .ok_or(ManagerError::EndpointNotFound(id))?;

        // Skip to ready if in waiting-for-identity state
        if ep.lifecycle.state() == EndpointState::WaitingForIdentity {
            ep.lifecycle
                .transition(EndpointState::Ready)
                .map_err(|e| ManagerError::InvalidStateTransition(e))?;

            let mut stats = self.stats.write().await;
            stats.currently_ready = stats.currently_ready.saturating_add(1);

            info!("Endpoint {} is ready", id);
        }

        Ok(())
    }

    /// Trigger regeneration for an endpoint.
    pub async fn regenerate(
        &self,
        id: u16,
        reason: RegenerationReason,
        message: Option<String>,
    ) -> ManagerResult<()> {
        let mut eps = self.endpoints.write().await;
        let ep = eps
            .get_mut(&id)
            .ok_or(ManagerError::EndpointNotFound(id))?;

        // Transition to regenerating
        ep.lifecycle
            .regeneration_started()
            .map_err(|e| ManagerError::InvalidStateTransition(e))?;

        let mut stats = self.stats.write().await;
        stats.currently_regenerating = stats.currently_regenerating.saturating_add(1);

        debug!("Endpoint {} regeneration started: {}", id, reason);

        // Simulate regeneration (real impl would compile eBPF, update maps, etc.)
        let mut metadata = RegenerationMetadata::new(reason);
        if let Some(msg) = message {
            metadata = metadata.with_message(msg);
        }

        ep.lifecycle
            .regeneration_complete(metadata)
            .map_err(|e| ManagerError::InvalidStateTransition(e))?;

        stats.total_regenerations += 1;
        stats.currently_regenerating = stats.currently_regenerating.saturating_sub(1);
        // Don't increment currently_ready here, endpoint was already ready before regen

        info!("Endpoint {} regeneration complete", id);
        Ok(())
    }

    /// Get current manager statistics.
    pub async fn get_stats(&self) -> ManagerStats {
        self.stats.read().await.clone()
    }

    /// Count endpoints in a given state.
    pub async fn count_by_state(&self, state: EndpointState) -> usize {
        let eps = self.endpoints.read().await;
        eps.values().filter(|ep| ep.state() == state).count()
    }

    /// Get all ready endpoints.
    pub async fn ready_endpoints(&self) -> Vec<ManagedEndpoint> {
        let eps = self.endpoints.read().await;
        eps.values()
            .filter(|ep| ep.state() == EndpointState::Ready)
            .cloned()
            .collect()
    }
}

impl Default for EndpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_endpoint() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        assert!(mgr.create(meta.clone()).await.is_ok());
        assert_eq!(mgr.list().await.len(), 1);
    }

    #[tokio::test]
    async fn test_create_duplicate_endpoint() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta.clone()).await.unwrap();
        assert!(mgr.create(meta).await.is_err());
    }

    #[tokio::test]
    async fn test_get_endpoint() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta.clone()).await.unwrap();
        let ep = mgr.get(0).await.unwrap();
        assert_eq!(ep.metadata.pod_name, "pod-1");
    }

    #[tokio::test]
    async fn test_get_nonexistent_endpoint() {
        let mgr = EndpointManager::new();
        assert!(mgr.get(999).await.is_err());
    }

    #[tokio::test]
    async fn test_delete_endpoint() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta).await.unwrap();
        assert_eq!(mgr.list().await.len(), 1);

        let deleted = mgr.delete(0).await.unwrap();
        assert_eq!(deleted.metadata.pod_name, "pod-1");
        assert_eq!(mgr.list().await.len(), 0);
    }

    #[tokio::test]
    async fn test_mark_waiting_for_identity() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta).await.unwrap();
        mgr.mark_waiting_for_identity(0).await.unwrap();

        let ep = mgr.get(0).await.unwrap();
        assert_eq!(ep.state(), EndpointState::WaitingForIdentity);
    }

    #[tokio::test]
    async fn test_mark_ready() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta).await.unwrap();
        mgr.mark_waiting_for_identity(0).await.unwrap();
        mgr.mark_ready(0).await.unwrap();

        let ep = mgr.get(0).await.unwrap();
        assert_eq!(ep.state(), EndpointState::Ready);
    }

    #[tokio::test]
    async fn test_regenerate_endpoint() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta).await.unwrap();
        mgr.mark_waiting_for_identity(0).await.unwrap();
        mgr.mark_ready(0).await.unwrap();

        mgr.regenerate(0, RegenerationReason::PolicyUpdate, None)
            .await
            .unwrap();

        let ep = mgr.get(0).await.unwrap();
        assert_eq!(ep.state(), EndpointState::Ready);
        assert_eq!(ep.lifecycle.stats().regeneration_count, 1);
    }

    #[tokio::test]
    async fn test_count_by_state() {
        let mgr = EndpointManager::new();

        let meta1 = EndpointMetadata::new(0, "cont1", "pod-1", "default");
        let meta2 = EndpointMetadata::new(1, "cont2", "pod-2", "default");
        

        mgr.create(meta1).await.unwrap();
        mgr.create(meta2).await.unwrap();

        mgr.mark_waiting_for_identity(0).await.unwrap();
        mgr.mark_ready(0).await.unwrap();

        assert_eq!(
            mgr.count_by_state(EndpointState::Ready).await,
            1
        );
        assert_eq!(
            mgr.count_by_state(EndpointState::Creating).await,
            1
        );
    }

    #[tokio::test]
    async fn test_ready_endpoints() {
        let mgr = EndpointManager::new();

        let meta1 = EndpointMetadata::new(0, "cont1", "pod-1", "default");
        let meta2 = EndpointMetadata::new(1, "cont2", "pod-2", "default");
        

        mgr.create(meta1).await.unwrap();
        mgr.create(meta2).await.unwrap();

        mgr.mark_waiting_for_identity(0).await.unwrap();
        mgr.mark_ready(0).await.unwrap();

        let ready = mgr.ready_endpoints().await;
        assert_eq!(ready.len(), 1);
    }

    #[tokio::test]
    async fn test_manager_statistics() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta).await.unwrap();
        mgr.mark_waiting_for_identity(0).await.unwrap();
        mgr.mark_ready(0).await.unwrap();
        mgr.regenerate(0, RegenerationReason::PolicyUpdate, None)
            .await
            .unwrap();

        let stats = mgr.get_stats().await;
        assert_eq!(stats.total_created, 1);
        assert_eq!(stats.total_regenerations, 1);
        assert_eq!(stats.currently_ready, 1);
    }

    #[tokio::test]
    async fn test_manager_delete_updates_stats() {
        let mgr = EndpointManager::new();
        let meta = EndpointMetadata::new(0, "cont1", "pod-1", "default");

        mgr.create(meta).await.unwrap();
        let before = mgr.get_stats().await;
        assert_eq!(before.total_created, 1);

        mgr.delete(0).await.unwrap();
        let after = mgr.get_stats().await;
        assert_eq!(after.total_deleted, 1);
    }
}
