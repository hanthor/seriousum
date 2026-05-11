//! Lightweight Hubble observability scaffold.
//!
//! This crate intentionally stays small for now: serde-friendly flow metadata,
//! summary counts, and a report wrapper that can be serialized for future event
//! streaming and API integration.

use serde::{Deserialize, Serialize};
use seriousum_api::MessageMetadata;
use seriousum_core::{Error, Port, Protocol, Result, SecurityIdentity, SecurityLabel};

/// Default component name used by the Hubble scaffold.
pub const HUBBLE_COMPONENT: &str = "seriousum-hubble";

/// Direction of a captured flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    /// Traffic entering a workload or node boundary.
    Ingress,
    /// Traffic leaving a workload or node boundary.
    Egress,
    /// Direction has not been determined yet.
    Unknown,
}

/// Verdict attached to an observed flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowVerdict {
    /// The flow was forwarded successfully.
    Forwarded,
    /// The flow was dropped.
    Dropped,
    /// The flow was denied by policy.
    Denied,
}

/// Endpoint metadata for a flow observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowEndpoint {
    /// Optional IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<std::net::IpAddr>,

    /// Optional transport port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<Port>,

    /// Optional security identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<SecurityIdentity>,

    /// Optional security labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<SecurityLabel>,
}

impl FlowEndpoint {
    /// Creates an empty endpoint scaffold.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ip: None,
            port: None,
            identity: None,
            labels: Vec::new(),
        }
    }

    /// Adds an IP address to the endpoint.
    #[must_use]
    pub fn with_ip(mut self, ip: impl Into<std::net::IpAddr>) -> Self {
        self.ip = Some(ip.into());
        self
    }

    /// Adds a port to the endpoint.
    #[must_use]
    pub fn with_port(mut self, port: impl Into<Port>) -> Self {
        self.port = Some(port.into());
        self
    }

    /// Adds a security identity to the endpoint.
    #[must_use]
    pub fn with_identity(mut self, identity: impl Into<SecurityIdentity>) -> Self {
        self.identity = Some(identity.into());
        self
    }

    /// Adds a label to the endpoint.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<SecurityLabel>) -> Self {
        self.labels.push(label.into());
        self
    }
}

impl Default for FlowEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata describing a single captured flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowMetadata {
    /// Stable identifier for the flow.
    pub id: String,

    /// Flow source endpoint.
    pub source: FlowEndpoint,

    /// Flow destination endpoint.
    pub destination: FlowEndpoint,

    /// Transport protocol.
    pub protocol: Protocol,

    /// Direction of traffic.
    pub direction: FlowDirection,

    /// Optional service name associated with the flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

impl FlowMetadata {
    /// Creates flow metadata.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        source: FlowEndpoint,
        destination: FlowEndpoint,
        protocol: Protocol,
        direction: FlowDirection,
    ) -> Self {
        Self {
            id: id.into(),
            source,
            destination,
            protocol,
            direction,
            service: None,
        }
    }

    /// Attaches a service name.
    #[must_use]
    pub fn with_service(mut self, service: impl Into<String>) -> Self {
        self.service = Some(service.into());
        self
    }
}

/// A captured flow observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowObservation {
    /// Flow metadata.
    pub metadata: FlowMetadata,

    /// Verdict associated with the observation.
    pub verdict: FlowVerdict,

    /// Number of events grouped into this observation.
    #[serde(default = "default_observation_count")]
    pub count: u64,
}

impl FlowObservation {
    /// Creates a single-event observation.
    #[must_use]
    pub fn new(metadata: FlowMetadata, verdict: FlowVerdict) -> Self {
        Self {
            metadata,
            verdict,
            count: 1,
        }
    }

    /// Sets the grouped count for the observation.
    #[must_use]
    pub const fn with_count(mut self, count: u64) -> Self {
        self.count = count;
        self
    }
}

fn default_observation_count() -> u64 {
    1
}

/// Summary counts for a batch of observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlowSummary {
    /// Total number of grouped events.
    pub total: u64,

    /// Number of forwarded events.
    pub forwarded: u64,

    /// Number of dropped events.
    pub dropped: u64,

    /// Number of denied events.
    pub denied: u64,
}

impl FlowSummary {
    /// Creates an empty summary.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            total: 0,
            forwarded: 0,
            dropped: 0,
            denied: 0,
        }
    }

    /// Records a single observation into the summary.
    pub fn record(&mut self, observation: &FlowObservation) {
        self.total = self.total.saturating_add(observation.count);
        match observation.verdict {
            FlowVerdict::Forwarded => {
                self.forwarded = self.forwarded.saturating_add(observation.count)
            }
            FlowVerdict::Dropped => self.dropped = self.dropped.saturating_add(observation.count),
            FlowVerdict::Denied => self.denied = self.denied.saturating_add(observation.count),
        }
    }

    /// Builds a summary from a slice of observations.
    #[must_use]
    pub fn from_observations(observations: &[FlowObservation]) -> Self {
        let mut summary = Self::new();
        for observation in observations {
            summary.record(observation);
        }
        summary
    }
}

/// Report wrapper for serialized Hubble observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubbleReport {
    /// Report metadata using the shared API contract.
    pub metadata: MessageMetadata,

    /// Aggregate counts for the included observations.
    pub summary: FlowSummary,

    /// Flow observations included in the report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<FlowObservation>,
}

impl HubbleReport {
    /// Builds a report for a component from a set of observations.
    #[must_use]
    pub fn new(component: impl Into<String>, observations: Vec<FlowObservation>) -> Self {
        let summary = FlowSummary::from_observations(&observations);
        Self {
            metadata: MessageMetadata::new(component),
            summary,
            observations,
        }
    }

    /// Builds an empty scaffold report.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(HUBBLE_COMPONENT, Vec::new())
    }

    /// Adds a trace identifier to the report metadata.
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.metadata = self.metadata.with_trace_id(trace_id);
        self
    }
}

/// Serializes a report to pretty JSON.
pub fn render_report(report: &HubbleReport) -> Result<String> {
    serde_json::to_string_pretty(report).map_err(|error| Error::Hubble(error.to_string()))
}

/// Returns the scaffold report as pretty JSON.
pub fn run() -> Result<String> {
    render_report(&HubbleReport::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata() -> FlowMetadata {
        FlowMetadata::new(
            "flow-1",
            FlowEndpoint::new()
                .with_ip(std::net::Ipv4Addr::new(10, 0, 0, 1))
                .with_port(80)
                .with_identity(SecurityIdentity::world())
                .with_label(SecurityLabel::k8s_namespace("default")),
            FlowEndpoint::new()
                .with_ip(std::net::Ipv4Addr::new(10, 0, 0, 2))
                .with_port(443)
                .with_identity(SecurityIdentity::cluster()),
            Protocol::Tcp,
            FlowDirection::Ingress,
        )
        .with_service("frontend")
    }

    #[test]
    fn summary_counts_observations() {
        let observations = vec![
            FlowObservation::new(sample_metadata(), FlowVerdict::Forwarded).with_count(3),
            FlowObservation::new(
                FlowMetadata::new(
                    "flow-2",
                    FlowEndpoint::new(),
                    FlowEndpoint::new(),
                    Protocol::Udp,
                    FlowDirection::Egress,
                ),
                FlowVerdict::Dropped,
            )
            .with_count(2),
            FlowObservation::new(
                FlowMetadata::new(
                    "flow-3",
                    FlowEndpoint::new(),
                    FlowEndpoint::new(),
                    Protocol::Tcp,
                    FlowDirection::Unknown,
                ),
                FlowVerdict::Denied,
            ),
        ];

        let summary = FlowSummary::from_observations(&observations);

        assert_eq!(summary.total, 6);
        assert_eq!(summary.forwarded, 3);
        assert_eq!(summary.dropped, 2);
        assert_eq!(summary.denied, 1);
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = HubbleReport::new(
            HUBBLE_COMPONENT,
            vec![FlowObservation::new(sample_metadata(), FlowVerdict::Forwarded).with_count(4)],
        )
        .with_trace_id("trace-9");

        let json = render_report(&report).expect("report serializes");
        let decoded: HubbleReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.metadata.component, HUBBLE_COMPONENT);
        assert_eq!(decoded.metadata.trace_id.as_deref(), Some("trace-9"));
        assert_eq!(decoded.summary.total, 4);
        assert_eq!(decoded.summary.forwarded, 4);
        assert_eq!(decoded.observations.len(), 1);
        assert_eq!(decoded.observations[0].metadata.id, "flow-1");
    }

    #[test]
    fn scaffold_report_is_empty_and_versioned() {
        let report = HubbleReport::scaffold();

        assert_eq!(report.metadata.component, HUBBLE_COMPONENT);
        assert_eq!(report.metadata.version.contract, env!("CARGO_PKG_VERSION"));
        assert_eq!(report.summary, FlowSummary::default());
        assert!(report.observations.is_empty());
    }
}
