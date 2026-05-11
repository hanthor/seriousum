//! Lightweight cluster-mesh synchronization scaffold.
//!
//! This crate stays intentionally small for now: serde-friendly sync status
//! types, a compact cluster/peer summary, and a report wrapper that carries
//! shared version metadata for future integration points.

use serde::{Deserialize, Serialize};
use seriousum_api::MessageMetadata;
use seriousum_core::{Error, Result};

/// Default component name used by the cluster-mesh scaffold.
pub const CLUSTERMESH_COMPONENT: &str = "seriousum-clustermesh";

/// High-level synchronization state for a peer or cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    /// Synchronization has not been evaluated yet.
    Unknown,

    /// Synchronization is in progress.
    Syncing,

    /// Synchronization is complete and healthy.
    Synced,

    /// Synchronization is still usable but not fully healthy.
    Degraded,

    /// Synchronization is stale and should be refreshed.
    Stale,

    /// Synchronization failed.
    Failed,
}

impl SyncStatus {
    /// Returns true when the status represents a fully synced peer.
    #[must_use]
    pub const fn is_synced(self) -> bool {
        matches!(self, Self::Synced)
    }

    /// Returns true when the status represents a usable but imperfect state.
    #[must_use]
    pub const fn is_degraded(self) -> bool {
        matches!(self, Self::Degraded | Self::Stale)
    }

    /// Returns true when the status is terminally unhealthy.
    #[must_use]
    pub const fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// Summary for a single cluster-mesh peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerSummary {
    /// Peer name or identifier.
    pub name: String,

    /// Cluster the peer belongs to.
    pub cluster: String,

    /// Current sync state for the peer.
    pub status: SyncStatus,

    /// Optional revision or version for the peer state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
}

impl PeerSummary {
    /// Creates a peer summary.
    #[must_use]
    pub fn new(name: impl Into<String>, cluster: impl Into<String>, status: SyncStatus) -> Self {
        Self {
            name: name.into(),
            cluster: cluster.into(),
            status,
            revision: None,
        }
    }

    /// Attaches a revision to the peer summary.
    #[must_use]
    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = Some(revision);
        self
    }
}

/// Summary for a cluster and its peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterSummary {
    /// Local cluster name.
    pub cluster: String,

    /// Known peers for the cluster-mesh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peers: Vec<PeerSummary>,
}

impl ClusterSummary {
    /// Creates an empty cluster summary.
    #[must_use]
    pub fn new(cluster: impl Into<String>) -> Self {
        Self {
            cluster: cluster.into(),
            peers: Vec::new(),
        }
    }

    /// Adds a peer to the summary.
    pub fn add_peer(&mut self, peer: PeerSummary) {
        self.peers.push(peer);
    }

    /// Returns a new summary with the peer appended.
    #[must_use]
    pub fn with_peer(mut self, peer: PeerSummary) -> Self {
        self.add_peer(peer);
        self
    }

    /// Returns the number of tracked peers.
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Returns the number of synced peers.
    #[must_use]
    pub fn synced_peer_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|peer| peer.status.is_synced())
            .count()
    }

    /// Returns the number of degraded peers.
    #[must_use]
    pub fn degraded_peer_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|peer| peer.status.is_degraded())
            .count()
    }

    /// Derives an aggregate status from the known peers.
    #[must_use]
    pub fn status(&self) -> SyncStatus {
        if self.peers.is_empty() {
            return SyncStatus::Unknown;
        }

        if self.peers.iter().any(|peer| peer.status.is_failed()) {
            return SyncStatus::Failed;
        }

        if self.peers.iter().any(|peer| peer.status.is_degraded()) {
            return SyncStatus::Degraded;
        }

        if self.peers.iter().all(|peer| peer.status.is_synced()) {
            return SyncStatus::Synced;
        }

        if self
            .peers
            .iter()
            .any(|peer| matches!(peer.status, SyncStatus::Syncing))
        {
            return SyncStatus::Syncing;
        }

        SyncStatus::Unknown
    }
}

/// Report wrapper for serialized cluster-mesh sync state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Shared component/version metadata.
    pub metadata: MessageMetadata,

    /// Aggregate sync status.
    pub status: SyncStatus,

    /// Cluster and peer summary.
    pub summary: ClusterSummary,
}

impl SyncReport {
    /// Builds a report for a component from a summary.
    #[must_use]
    pub fn new(component: impl Into<String>, summary: ClusterSummary) -> Self {
        let status = summary.status();
        Self {
            metadata: MessageMetadata::new(component),
            status,
            summary,
        }
    }

    /// Returns an empty scaffold report.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(CLUSTERMESH_COMPONENT, ClusterSummary::new("local"))
    }

    /// Adds a trace identifier to the metadata.
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.metadata = self.metadata.with_trace_id(trace_id);
        self
    }

    /// Refreshes the aggregate status from the current summary.
    pub fn refresh_status(&mut self) {
        self.status = self.summary.status();
    }
}

/// Serializes a report to pretty JSON.
pub fn render_report(report: &SyncReport) -> Result<String> {
    serde_json::to_string_pretty(report).map_err(|error| Error::Clustermesh(error.to_string()))
}

/// Returns the scaffold report as pretty JSON.
pub fn run() -> Result<String> {
    render_report(&SyncReport::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_helpers_classify_states() {
        assert!(SyncStatus::Synced.is_synced());
        assert!(SyncStatus::Stale.is_degraded());
        assert!(SyncStatus::Failed.is_failed());
        assert!(!SyncStatus::Syncing.is_failed());
    }

    #[test]
    fn cluster_summary_derives_aggregate_status() {
        let summary = ClusterSummary::new("cluster-a")
            .with_peer(
                PeerSummary::new("peer-1", "cluster-a", SyncStatus::Synced).with_revision(10),
            )
            .with_peer(PeerSummary::new("peer-2", "cluster-b", SyncStatus::Syncing))
            .with_peer(PeerSummary::new(
                "peer-3",
                "cluster-c",
                SyncStatus::Degraded,
            ));

        assert_eq!(summary.cluster, "cluster-a");
        assert_eq!(summary.peer_count(), 3);
        assert_eq!(summary.synced_peer_count(), 1);
        assert_eq!(summary.degraded_peer_count(), 1);
        assert_eq!(summary.status(), SyncStatus::Degraded);
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = SyncReport::new(
            CLUSTERMESH_COMPONENT,
            ClusterSummary::new("cluster-a")
                .with_peer(PeerSummary::new("peer-1", "cluster-a", SyncStatus::Synced))
                .with_peer(
                    PeerSummary::new("peer-2", "cluster-b", SyncStatus::Synced).with_revision(42),
                ),
        )
        .with_trace_id("trace-11");

        let json = render_report(&report).expect("report serializes");
        let decoded: SyncReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.metadata.component, CLUSTERMESH_COMPONENT);
        assert_eq!(decoded.metadata.trace_id.as_deref(), Some("trace-11"));
        assert_eq!(decoded.status, SyncStatus::Synced);
        assert_eq!(decoded.summary.cluster, "cluster-a");
        assert_eq!(decoded.summary.peer_count(), 2);
        assert_eq!(decoded.summary.synced_peer_count(), 2);
        assert_eq!(decoded.summary.peers[1].revision, Some(42));
    }

    #[test]
    fn scaffold_report_is_empty_and_versioned() {
        let report = SyncReport::scaffold();

        assert_eq!(report.metadata.component, CLUSTERMESH_COMPONENT);
        assert_eq!(report.metadata.version.contract, env!("CARGO_PKG_VERSION"));
        assert_eq!(report.summary.cluster, "local");
        assert_eq!(report.status, SyncStatus::Unknown);
        assert!(report.summary.peers.is_empty());
    }
}
