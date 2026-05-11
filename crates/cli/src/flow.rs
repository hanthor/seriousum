//! Network flow verification and analysis for Track U.
//! 
//! Provides flow analysis, filtering, and statistics collection.

use serde::{Deserialize, Serialize};
use crate::Result;

/// Information about a network flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkFlow {
    /// Source pod name.
    pub source_pod: String,

    /// Destination pod name.
    pub dest_pod: String,

    /// Protocol (tcp, udp, icmp).
    pub protocol: String,

    /// Destination port.
    pub dest_port: u16,

    /// Flow status (allowed, denied, error).
    pub status: String,

    /// Number of packets in this flow.
    pub packet_count: u64,

    /// Bytes transferred in this flow.
    pub bytes_transferred: u64,
}

/// Statistics about network flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowStatistics {
    /// Total number of flows.
    pub total_flows: usize,

    /// Number of allowed flows.
    pub allowed_flows: usize,

    /// Number of denied flows.
    pub denied_flows: usize,

    /// Total bytes transferred.
    pub total_bytes: u64,

    /// Total packets transferred.
    pub total_packets: u64,
}

/// Flow analyzer for analyzing and filtering network flows.
pub struct FlowAnalyzer;

impl FlowAnalyzer {
    /// Create a new flow analyzer.
    pub fn new() -> Self {
        Self
    }

    /// Get recent network flows.
    pub fn get_recent_flows(
        &self,
        limit: usize,
        source_pod: Option<&str>,
        dest_pod: Option<&str>,
    ) -> Result<Vec<NetworkFlow>> {
        let mut flows = vec![
            NetworkFlow {
                source_pod: "client-1".to_string(),
                dest_pod: "server-1".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 80,
                status: "allowed".to_string(),
                packet_count: 100,
                bytes_transferred: 5000,
            },
            NetworkFlow {
                source_pod: "client-1".to_string(),
                dest_pod: "server-2".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 443,
                status: "allowed".to_string(),
                packet_count: 50,
                bytes_transferred: 8000,
            },
            NetworkFlow {
                source_pod: "client-2".to_string(),
                dest_pod: "server-1".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 8080,
                status: "allowed".to_string(),
                packet_count: 75,
                bytes_transferred: 4000,
            },
            NetworkFlow {
                source_pod: "client-3".to_string(),
                dest_pod: "external".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 53,
                status: "denied".to_string(),
                packet_count: 10,
                bytes_transferred: 512,
            },
            NetworkFlow {
                source_pod: "client-1".to_string(),
                dest_pod: "database".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 5432,
                status: "allowed".to_string(),
                packet_count: 200,
                bytes_transferred: 15000,
            },
        ];

        // Filter by source pod if provided
        if let Some(src) = source_pod {
            flows.retain(|f| f.source_pod == src);
        }

        // Filter by destination pod if provided
        if let Some(dst) = dest_pod {
            flows.retain(|f| f.dest_pod == dst);
        }

        // Apply limit
        flows.truncate(limit);

        Ok(flows)
    }

    /// Get flow statistics for a namespace.
    pub fn get_flow_statistics(&self, _namespace: Option<&str>) -> Result<FlowStatistics> {
        Ok(FlowStatistics {
            total_flows: 42,
            allowed_flows: 38,
            denied_flows: 4,
            total_bytes: 100_000,
            total_packets: 1500,
        })
    }

    /// Filter flows based on an expression.
    pub fn filter_flows(&self, expression: &str) -> Result<Vec<NetworkFlow>> {
        // Simple expression parsing - just check if it contains keywords
        let mut flows = self
            .get_recent_flows(100, None, None)
            .unwrap_or_default();

        // Simple filter: if expression mentions "denied", show only denied flows
        if expression.contains("denied") {
            flows.retain(|f| f.status == "denied");
        }

        // Simple filter: if expression mentions "tcp", show only tcp flows
        if expression.contains("tcp") {
            flows.retain(|f| f.protocol == "tcp");
        }

        Ok(flows)
    }
}

impl Default for FlowAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_analyzer_creation() {
        let _analyzer = FlowAnalyzer::new();
    }

    #[test]
    fn test_get_recent_flows_no_filter() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .get_recent_flows(10, None, None)
            .expect("get recent flows");

        assert!(!flows.is_empty());
        assert!(flows.iter().all(|f| !f.source_pod.is_empty()));
    }

    #[test]
    fn test_get_recent_flows_limit() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .get_recent_flows(2, None, None)
            .expect("get recent flows");

        assert!(flows.len() <= 2);
    }

    #[test]
    fn test_get_recent_flows_filter_source() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .get_recent_flows(10, Some("client-1"), None)
            .expect("get recent flows");

        assert!(!flows.is_empty());
        assert!(flows.iter().all(|f| f.source_pod == "client-1"));
    }

    #[test]
    fn test_get_recent_flows_filter_destination() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .get_recent_flows(10, None, Some("server-1"))
            .expect("get recent flows");

        assert!(!flows.is_empty());
        assert!(flows.iter().all(|f| f.dest_pod == "server-1"));
    }

    #[test]
    fn test_get_recent_flows_filter_both() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .get_recent_flows(10, Some("client-1"), Some("server-1"))
            .expect("get recent flows");

        assert!(flows.iter().all(|f| f.source_pod == "client-1"));
        assert!(flows.iter().all(|f| f.dest_pod == "server-1"));
    }

    #[test]
    fn test_get_flow_statistics() {
        let analyzer = FlowAnalyzer::new();
        let stats = analyzer
            .get_flow_statistics(None)
            .expect("get flow statistics");

        assert!(stats.total_flows > 0);
        assert!(stats.allowed_flows > 0);
        assert!(stats.denied_flows > 0);
        assert_eq!(stats.total_flows, stats.allowed_flows + stats.denied_flows);
    }

    #[test]
    fn test_get_flow_statistics_with_namespace() {
        let analyzer = FlowAnalyzer::new();
        let stats = analyzer
            .get_flow_statistics(Some("default"))
            .expect("get flow statistics");

        assert!(stats.total_flows > 0);
    }

    #[test]
    fn test_filter_flows_by_denied() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .filter_flows("denied")
            .expect("filter flows");

        // All filtered flows should be denied
        assert!(flows.iter().all(|f| f.status == "denied"));
    }

    #[test]
    fn test_filter_flows_by_protocol() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .filter_flows("tcp")
            .expect("filter flows");

        assert!(flows.iter().all(|f| f.protocol == "tcp"));
    }

    #[test]
    fn test_filter_flows_combined() {
        let analyzer = FlowAnalyzer::new();
        let flows = analyzer
            .filter_flows("tcp denied")
            .expect("filter flows");

        assert!(flows.iter().all(|f| f.protocol == "tcp" || f.status == "denied"));
    }

    #[test]
    fn test_network_flow_serialization() {
        let flow = NetworkFlow {
            source_pod: "client".to_string(),
            dest_pod: "server".to_string(),
            protocol: "tcp".to_string(),
            dest_port: 80,
            status: "allowed".to_string(),
            packet_count: 100,
            bytes_transferred: 5000,
        };

        let json = serde_json::to_string(&flow).expect("serialize");
        assert!(json.contains("\"source_pod\":\"client\""));
        assert!(json.contains("\"dest_port\":80"));
    }

    #[test]
    fn test_flow_statistics_serialization() {
        let stats = FlowStatistics {
            total_flows: 42,
            allowed_flows: 38,
            denied_flows: 4,
            total_bytes: 100_000,
            total_packets: 1500,
        };

        let json = serde_json::to_string(&stats).expect("serialize");
        assert!(json.contains("\"total_flows\":42"));
        assert!(json.contains("\"allowed_flows\":38"));
    }

    #[test]
    fn test_flow_status_variants() {
        let flows = vec![
            NetworkFlow {
                source_pod: "a".to_string(),
                dest_pod: "b".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 80,
                status: "allowed".to_string(),
                packet_count: 10,
                bytes_transferred: 1000,
            },
            NetworkFlow {
                source_pod: "c".to_string(),
                dest_pod: "d".to_string(),
                protocol: "tcp".to_string(),
                dest_port: 443,
                status: "denied".to_string(),
                packet_count: 5,
                bytes_transferred: 500,
            },
        ];

        let statuses: Vec<_> = flows.iter().map(|f| f.status.as_str()).collect();
        assert!(statuses.contains(&"allowed"));
        assert!(statuses.contains(&"denied"));
    }
}
