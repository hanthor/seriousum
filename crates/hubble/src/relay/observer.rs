//! Flow collection and observation from peers
//!
//! Handles aggregation of flows from multiple peers with filtering,
//! error aggregation, and result ordering.

use crate::relay::{RelayError, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::info;

/// Flow record from observation
#[derive(Debug, Clone)]
pub struct FlowRecord {
    /// Unique flow identifier
    pub id: String,
    /// Source address
    pub src_addr: String,
    /// Destination address
    pub dst_addr: String,
    /// Protocol type
    pub protocol: String,
    /// Timestamp of observation
    pub timestamp: SystemTime,
    /// Node name where flow was observed
    pub node_name: String,
    /// Optional verdict (forwarded, dropped, denied)
    pub verdict: Option<String>,
}

impl FlowRecord {
    /// Creates a new flow record
    pub fn new(
        id: impl Into<String>,
        src_addr: impl Into<String>,
        dst_addr: impl Into<String>,
        protocol: impl Into<String>,
        node_name: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            src_addr: src_addr.into(),
            dst_addr: dst_addr.into(),
            protocol: protocol.into(),
            timestamp: SystemTime::now(),
            node_name: node_name.into(),
            verdict: None,
        }
    }

    /// Sets the verdict
    pub fn with_verdict(mut self, verdict: impl Into<String>) -> Self {
        self.verdict = Some(verdict.into());
        self
    }
}

/// Request for collecting flows
#[derive(Debug, Clone)]
pub struct FlowRequest {
    /// Maximum number of flows to return
    pub max_flows: usize,
    /// Time window for collection
    pub time_window: Duration,
    /// Optional filter expression
    pub filter: Option<String>,
    /// Whether to follow (stream) results
    pub follow: bool,
}

impl Default for FlowRequest {
    fn default() -> Self {
        Self {
            max_flows: 100,
            time_window: Duration::from_secs(10),
            filter: None,
            follow: false,
        }
    }
}

/// Flow collector that manages collection from multiple peers
pub struct FlowCollector {
    /// Connected nodes
    connected: Arc<RwLock<HashMap<String, bool>>>,
    /// Collected flows
    flows: Arc<RwLock<Vec<FlowRecord>>>,
}

impl FlowCollector {
    /// Creates a new flow collector
    pub fn new() -> Self {
        Self {
            connected: Arc::new(RwLock::new(HashMap::new())),
            flows: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Marks a peer as connected
    pub async fn mark_connected(&self, peer_name: impl Into<String>) -> Result<()> {
        let mut connected = self.connected.write().await;
        connected.insert(peer_name.into(), true);
        Ok(())
    }

    /// Marks a peer as disconnected
    pub async fn mark_disconnected(&self, peer_name: impl Into<String>) -> Result<()> {
        let mut connected = self.connected.write().await;
        connected.remove(&peer_name.into());
        Ok(())
    }

    /// Adds a flow to the collection
    pub async fn add_flow(&self, flow: FlowRecord) -> Result<()> {
        let mut flows = self.flows.write().await;
        flows.push(flow);
        Ok(())
    }

    /// Returns all collected flows
    pub async fn get_flows(&self) -> Result<Vec<FlowRecord>> {
        let flows = self.flows.read().await;
        Ok(flows.clone())
    }

    /// Returns flows sorted by timestamp
    pub async fn get_flows_sorted(&self) -> Result<Vec<FlowRecord>> {
        let flows = self.flows.read().await;
        let mut sorted = flows.clone();
        sorted.sort_by_key(|f| f.timestamp);
        Ok(sorted)
    }

    /// Clears all collected flows
    pub async fn clear(&self) -> Result<()> {
        let mut flows = self.flows.write().await;
        flows.clear();
        Ok(())
    }

    /// Returns the count of flows from a specific peer
    pub async fn flows_from_peer(&self, peer_name: &str) -> Result<usize> {
        let flows = self.flows.read().await;
        Ok(flows.iter().filter(|f| f.node_name == peer_name).count())
    }

    /// Returns connected peers
    pub async fn get_connected_peers(&self) -> Result<Vec<String>> {
        let connected = self.connected.read().await;
        Ok(connected
            .iter()
            .filter_map(|(name, &is_conn)| if is_conn { Some(name.clone()) } else { None })
            .collect())
    }
}

impl Default for FlowCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Observer instance managing flow collection
pub struct Observer {
    /// Flow collector
    collector: Arc<FlowCollector>,
}

impl Observer {
    /// Creates a new observer
    pub fn new(_sort_buffer_max_len: usize, _sort_buffer_drain_timeout: Duration) -> Self {
        Self {
            collector: Arc::new(FlowCollector::new()),
        }
    }

    /// Gets the flow collector
    pub fn collector(&self) -> Arc<FlowCollector> {
        self.collector.clone()
    }

    /// Collects flows from peers
    pub async fn collect_flows(&self, request: &FlowRequest) -> Result<Vec<FlowRecord>> {
        info!(max_flows = request.max_flows, "Collecting flows");

        // Get flows sorted by timestamp
        let flows = self.collector.get_flows_sorted().await?;

        // Apply max_flows limit
        let result = if flows.len() > request.max_flows {
            flows[..request.max_flows].to_vec()
        } else {
            flows
        };

        info!(count = result.len(), "Collected flows");

        Ok(result)
    }

    /// Aggregates errors from multiple sources
    pub fn aggregate_errors(&self, errors: Vec<RelayError>) -> Result<Vec<String>> {
        let mut aggregated: HashMap<String, usize> = HashMap::new();

        for error in errors {
            let msg = error.to_string();
            *aggregated.entry(msg).or_insert(0) += 1;
        }

        Ok(aggregated
            .into_iter()
            .map(|(msg, count)| {
                if count > 1 {
                    format!("{msg} ({count}x)")
                } else {
                    msg
                }
            })
            .collect())
    }
}

impl Default for Observer {
    fn default() -> Self {
        Self::new(
            crate::relay::defaults::SORT_BUFFER_MAX_LEN,
            crate::relay::defaults::SORT_BUFFER_DRAIN_TIMEOUT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn flow_record_creation() {
        let flow = FlowRecord::new("flow-1", "10.0.0.1", "10.0.0.2", "tcp", "node-1");

        assert_eq!(flow.id, "flow-1");
        assert_eq!(flow.src_addr, "10.0.0.1");
        assert_eq!(flow.dst_addr, "10.0.0.2");
        assert_eq!(flow.node_name, "node-1");
    }

    #[tokio::test]
    async fn flow_record_with_verdict() {
        let flow = FlowRecord::new("flow-1", "10.0.0.1", "10.0.0.2", "tcp", "node-1")
            .with_verdict("forwarded");

        assert_eq!(flow.verdict, Some("forwarded".to_string()));
    }

    #[tokio::test]
    async fn flow_collector_add_and_get() {
        let collector = FlowCollector::new();
        let flow = FlowRecord::new("flow-1", "10.0.0.1", "10.0.0.2", "tcp", "node-1");

        collector.add_flow(flow.clone()).await.unwrap();

        let flows = collector.get_flows().await.unwrap();
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].id, "flow-1");
    }

    #[tokio::test]
    async fn flow_collector_peer_tracking() {
        let collector = FlowCollector::new();

        collector.mark_connected("node-1").await.unwrap();
        collector.mark_connected("node-2").await.unwrap();

        let peers = collector.get_connected_peers().await.unwrap();
        assert_eq!(peers.len(), 2);

        collector.mark_disconnected("node-1").await.unwrap();
        let peers = collector.get_connected_peers().await.unwrap();
        assert_eq!(peers.len(), 1);
    }

    #[tokio::test]
    async fn flow_collector_flows_from_peer() {
        let collector = FlowCollector::new();

        collector
            .add_flow(FlowRecord::new(
                "flow-1", "10.0.0.1", "10.0.0.2", "tcp", "node-1",
            ))
            .await
            .unwrap();

        collector
            .add_flow(FlowRecord::new(
                "flow-2", "10.0.0.3", "10.0.0.4", "tcp", "node-1",
            ))
            .await
            .unwrap();

        collector
            .add_flow(FlowRecord::new(
                "flow-3", "10.0.0.5", "10.0.0.6", "tcp", "node-2",
            ))
            .await
            .unwrap();

        assert_eq!(collector.flows_from_peer("node-1").await.unwrap(), 2);
        assert_eq!(collector.flows_from_peer("node-2").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn observer_collect_flows() {
        let observer = Observer::default();
        let collector = observer.collector();

        for i in 0..5 {
            collector
                .add_flow(FlowRecord::new(
                    &format!("flow-{}", i),
                    "10.0.0.1",
                    "10.0.0.2",
                    "tcp",
                    "node-1",
                ))
                .await
                .unwrap();
        }

        let request = FlowRequest {
            max_flows: 3,
            ..Default::default()
        };

        let flows = observer.collect_flows(&request).await.unwrap();
        assert_eq!(flows.len(), 3);
    }

    #[test]
    fn observer_aggregate_errors() {
        let observer = Observer::default();

        let errors = vec![
            RelayError::PeerNotAvailable("node-1".to_string()),
            RelayError::PeerNotAvailable("node-1".to_string()),
            RelayError::ConnectionFailed("timeout".to_string()),
        ];

        let aggregated = observer.aggregate_errors(errors).unwrap();
        assert_eq!(aggregated.len(), 2);
    }

    #[test]
    fn flow_request_default() {
        let req = FlowRequest::default();
        assert_eq!(req.max_flows, 100);
        assert!(!req.follow);
    }
}
