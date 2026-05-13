//! Hubble observability and relay infrastructure.
//!
//! This crate provides:
//! - Lightweight flow metadata types (serde-friendly)
//! - Summary counts for flow aggregation
//! - Hubble Relay server for distributed multi-cluster flow observation
//! - Flow collection and filtering with peer coordination

pub mod relay;

use std::collections::VecDeque;
use std::net::IpAddr;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Port, Protocol, Result, SecurityIdentity, SecurityLabel};
use tracing::debug;

/// Default component name used by the Hubble scaffold.
pub const HUBBLE_COMPONENT: &str = "seriousum-hubble";

/// Metadata describing one end of a flow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Optional IP address associated with the endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<IpAddr>,

    /// Optional transport-layer port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Optional numeric security identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<u32>,

    /// Kubernetes namespace for the endpoint, when known.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub namespace: String,

    /// Kubernetes pod name for the endpoint, when known.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pod_name: String,

    /// Security labels attached to the endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
}

/// Verdict attached to a flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The flow was forwarded.
    Forwarded,
    /// The flow was dropped.
    Dropped,
    /// The datapath reported an error.
    Error,
    /// The flow was observed in audit mode.
    Audit,
    /// The flow was redirected.
    Redirected,
    /// The verdict is unknown.
    Unknown,
}

/// TCP flag summary for a flow event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct TcpFlags {
    /// SYN flag.
    pub syn: bool,
    /// ACK flag.
    pub ack: bool,
    /// FIN flag.
    pub fin: bool,
    /// RST flag.
    pub rst: bool,
    /// PSH flag.
    pub psh: bool,
}

/// Layer 4 protocol details for a flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum L4Proto {
    /// TCP source and destination ports plus relevant flags.
    TCP {
        /// TCP source port.
        source_port: u16,
        /// TCP destination port.
        dest_port: u16,
        /// TCP flag summary.
        flags: TcpFlags,
    },
    /// UDP source and destination ports.
    UDP {
        /// UDP source port.
        source_port: u16,
        /// UDP destination port.
        dest_port: u16,
    },
    /// ICMPv4 type and code.
    ICMPv4 {
        /// ICMPv4 type.
        type_: u8,
        /// ICMPv4 code.
        code: u8,
    },
    /// ICMPv6 type and code.
    ICMPv6 {
        /// ICMPv6 type.
        type_: u8,
        /// ICMPv6 code.
        code: u8,
    },
}

/// Direction of observed traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficDirection {
    /// Direction is unknown.
    Unknown,
    /// Traffic is ingress to the endpoint or node.
    Ingress,
    /// Traffic is egress from the endpoint or node.
    Egress,
}

/// Monitor event type attached to a flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventType {
    /// Primary event type.
    pub type_: i32,
    /// Event subtype.
    pub sub_type: i32,
}

/// Core flow event observed by Hubble.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    /// Timestamp when the flow was observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<SystemTime>,

    /// Flow verdict.
    pub verdict: Verdict,

    /// Numeric drop reason; zero means not dropped.
    pub drop_reason: u32,

    /// Source endpoint metadata.
    pub source: Endpoint,

    /// Destination endpoint metadata.
    pub destination: Endpoint,

    /// Optional layer 4 protocol details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l4: Option<L4Proto>,

    /// Traffic direction for the event.
    pub traffic_direction: TrafficDirection,

    /// Whether the event is marked as a reply.
    pub reply: bool,

    /// Optional reply marker retained from the source event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_reply: Option<bool>,

    /// Name of the node that observed the flow.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub node_name: String,

    /// Optional monitor event type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<EventType>,
}

/// Predicate applied to flows.
pub trait FlowFilter: Send + Sync {
    /// Returns true when the flow matches the filter.
    fn matches(&self, flow: &Flow) -> bool;
}

/// Filter matching flows by verdict.
#[derive(Debug, Clone, Default)]
pub struct VerdictFilter {
    /// Verdicts accepted by the filter.
    pub verdicts: Vec<Verdict>,
}

impl FlowFilter for VerdictFilter {
    fn matches(&self, flow: &Flow) -> bool {
        self.verdicts.contains(&flow.verdict)
    }
}

/// Filter matching flows by traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionFilter {
    /// Direction accepted by the filter.
    pub direction: TrafficDirection,
}

impl FlowFilter for DirectionFilter {
    fn matches(&self, flow: &Flow) -> bool {
        flow.traffic_direction == self.direction
    }
}

/// Filter that requires every child filter to match.
#[derive(Default)]
pub struct AndFilter {
    /// Child filters evaluated with AND semantics.
    pub filters: Vec<Box<dyn FlowFilter>>,
}

impl FlowFilter for AndFilter {
    fn matches(&self, flow: &Flow) -> bool {
        self.filters.iter().all(|filter| filter.matches(flow))
    }
}

/// Fixed-capacity flow ring buffer.
#[derive(Debug, Clone)]
pub struct FlowRing {
    buf: VecDeque<Flow>,
    capacity: usize,
}

impl FlowRing {
    /// Creates a new ring buffer, promoting zero capacity to one entry.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        if capacity == 0 {
            debug!("requested zero-capacity flow ring; promoting capacity to one");
        }
        let capacity = capacity.max(1);
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Creates a new ring buffer or returns an error for zero capacity.
    pub fn try_new(capacity: usize) -> std::result::Result<Self, HubbleError> {
        if capacity == 0 {
            return Err(HubbleError::InvalidCapacity);
        }
        Ok(Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        })
    }

    /// Pushes a flow, evicting the oldest entry when the ring is full.
    pub fn push(&mut self, flow: Flow) {
        if self.buf.len() == self.capacity {
            debug!(capacity = self.capacity, "evicting oldest flow from ring");
            let _ = self.buf.pop_front();
        }
        self.buf.push_back(flow);
    }

    /// Returns the number of stored flows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns true when the ring is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Iterates over flows from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &Flow> + '_ {
        self.buf.iter()
    }

    /// Returns the last n flows, ordered from oldest to newest within the slice.
    #[must_use]
    pub fn last_n(&self, n: usize) -> Vec<&Flow> {
        let start = self.buf.len().saturating_sub(n);
        self.buf.iter().skip(start).collect()
    }
}

/// Connection status for a Hubble node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// The node is connected and serving flows.
    Connected,
    /// The node is currently unavailable.
    Unavailable,
    /// The node reported an error message.
    Error(String),
}

/// Aggregated status for a Hubble node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubbleNodeState {
    /// Node name.
    pub name: String,
    /// Current node status.
    pub status: NodeStatus,
    /// Observed flow rate in flows per second.
    pub flows_per_second: f64,
    /// Number of flows currently stored.
    pub num_flows: u64,
    /// Maximum configured flow capacity.
    pub max_flows: u64,
    /// Total number of flows seen since startup.
    pub seen_flows: u64,
}

/// Errors returned by the Hubble data model.
#[derive(Debug, thiserror::Error)]
pub enum HubbleError {
    /// The configured ring capacity was invalid.
    #[error("ring buffer capacity must be > 0")]
    InvalidCapacity,
    /// The observer is not running.
    #[error("observer not running")]
    NotRunning,
}

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
                self.forwarded = self.forwarded.saturating_add(observation.count);
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

/// Simple metadata for reports
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Component that generated the message
    pub component: String,
    /// Optional trace ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl MessageMetadata {
    /// Creates new metadata
    #[must_use]
    pub fn new(component: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            trace_id: None,
        }
    }

    /// Sets trace ID
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
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
const FLOW_PRINT_COUNT: u64 = 20;

#[cfg(test)]
const MONITOR_MESSAGE_TYPE_NAMES: [&str; 8] = [
    "capture",
    "drop",
    "trace",
    "policy-verdict",
    "debug",
    "l7",
    "agent",
    "trace-sock",
];

#[cfg(test)]
const FLOW_EVENT_TYPES: [&str; 6] = [
    "capture",
    "drop",
    "l7",
    "policy-verdict",
    "trace",
    "trace-sock",
];

#[cfg(test)]
const DEFAULT_FIELD_MASK: [&str; 20] = [
    "time",
    "verdict",
    "ethernet",
    "IP",
    "l4",
    "source.identity",
    "source.namespace",
    "source.pod_name",
    "destination.identity",
    "destination.namespace",
    "destination.pod_name",
    "Type",
    "node_name",
    "l7",
    "event_type",
    "source_service",
    "destination_service",
    "is_reply",
    "Summary",
    "ip_trace_id",
];

#[cfg(test)]
type RawFlowFilter = serde_json::Map<String, serde_json::Value>;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SelectorOptions {
    all: bool,
    last: u64,
    since: Option<String>,
    until: Option<String>,
    follow: bool,
    first: u64,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct MaskOptions {
    field_mask: Vec<String>,
    use_default_masks: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct FormattingOptions {
    output: String,
}

#[cfg(test)]
impl Default for FormattingOptions {
    fn default() -> Self {
        Self {
            output: String::from("compact"),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct OtherOptions {
    input_file: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct FlowRequest {
    number: u64,
    follow: bool,
    first: bool,
    since: Option<String>,
    until: Option<String>,
    whitelist: Vec<RawFlowFilter>,
    blacklist: Vec<RawFlowFilter>,
    field_mask: Vec<String>,
}

#[cfg(test)]
fn apply_flow_args(
    formatting: &FormattingOptions,
    masks: &mut MaskOptions,
) -> std::result::Result<(), String> {
    let json_output = matches!(formatting.output.as_str(), "json" | "jsonpb" | "JSON");
    if !json_output {
        if !masks.field_mask.is_empty() {
            return Err(format!(
                "{} output format is not compatible with custom field mask",
                formatting.output
            ));
        }
        if masks.use_default_masks {
            masks.field_mask = DEFAULT_FIELD_MASK
                .iter()
                .map(|path| (*path).to_owned())
                .collect();
        }
    }
    Ok(())
}

#[cfg(test)]
fn parse_raw_filters(filters: &[String]) -> std::result::Result<Vec<RawFlowFilter>, String> {
    let mut parsed = Vec::new();
    for raw in filters {
        let stream = serde_json::Deserializer::from_str(raw).into_iter::<RawFlowFilter>();
        for item in stream {
            let filter = item.map_err(|error| format!("failed to decode '{raw}': {error}"))?;
            for key in filter.keys() {
                if !is_valid_flow_filter_key(key) {
                    return Err(format!(
                        "failed to decode '{raw}': unknown filter key '{key}'"
                    ));
                }
            }
            parsed.push(filter);
        }
    }
    Ok(parsed)
}

#[cfg(test)]
fn is_valid_flow_filter_key(key: &str) -> bool {
    matches!(
        key,
        "source_pod"
            | "destination_pod"
            | "source_ip"
            | "destination_ip"
            | "source_label"
            | "destination_label"
            | "source_port"
            | "destination_port"
            | "source_service"
            | "destination_service"
            | "source_identity"
            | "destination_identity"
            | "event_type"
            | "verdict"
            | "protocol"
            | "reply"
    )
}

#[cfg(test)]
fn get_flow_filters_yaml(request: &FlowRequest) -> std::result::Result<String, String> {
    let allowlist = request
        .whitelist
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let denylist = request
        .blacklist
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;

    let mut out = String::new();
    if !allowlist.is_empty() {
        out.push_str("allowlist:\n");
        for filter in allowlist {
            out.push_str("    - '");
            out.push_str(&filter);
            out.push_str("'\n");
        }
    }
    if !denylist.is_empty() {
        out.push_str("denylist:\n");
        for filter in denylist {
            out.push_str("    - '");
            out.push_str(&filter);
            out.push_str("'\n");
        }
    }
    Ok(out)
}

#[cfg(test)]
fn get_flows_request(
    selector: &mut SelectorOptions,
    masks: &MaskOptions,
    other: &OtherOptions,
    allowlist: &[String],
    denylist: &[String],
) -> std::result::Result<FlowRequest, String> {
    let first = selector.first > 0;
    let last = selector.last > 0;
    if first && last {
        return Err(String::from("cannot set both --first and --last"));
    }
    if first && selector.all {
        return Err(String::from("cannot set both --first and --all"));
    }
    if first && selector.follow {
        return Err(String::from("cannot set both --first and --follow"));
    }
    if last && selector.all {
        return Err(String::from("cannot set both --last and --all"));
    }

    let since = selector.since.clone();
    let until = if selector.follow {
        None
    } else {
        selector.until.clone()
    };

    if since.is_none() && until.is_none() && !first {
        if selector.all {
            selector.last = u64::MAX;
        } else if selector.last == 0 && !selector.follow && other.input_file.is_none() {
            selector.last = FLOW_PRINT_COUNT;
        }
    }

    let whitelist = parse_raw_filters(allowlist)
        .map_err(|error| format!("invalid --allowlist flag: {error}"))?;
    let blacklist =
        parse_raw_filters(denylist).map_err(|error| format!("invalid --denylist flag: {error}"))?;

    let number = if first { selector.first } else { selector.last };
    if let Some(path) = masks
        .field_mask
        .iter()
        .find(|path| !is_valid_field_mask_path(path.as_str()))
    {
        return Err(format!(
            "failed to construct field mask: invalid path '{path}'"
        ));
    }

    Ok(FlowRequest {
        number,
        follow: selector.follow,
        first,
        since,
        until,
        whitelist,
        blacklist,
        field_mask: masks.field_mask.clone(),
    })
}

#[cfg(test)]
fn is_valid_field_mask_path(path: &str) -> bool {
    DEFAULT_FIELD_MASK.contains(&path)
}

#[cfg(test)]
const HUBBLE_ROOT_HELP_TEMPLATE: &str = r#"Hubble is a utility to observe and inspect recent Cilium routed traffic in a cluster.

Usage:
  hubble [command]

Available Commands:
  completion  Generate the autocompletion script for the specified shell
  config      Modify or view hubble config
  help        Help about any command
  list        List Hubble objects
  observe     Observe flows and events of a Hubble server
  status      Display status of Hubble server
  version     Display detailed version information

Global Flags:
      --config string   Optional config file (default "{}")
  -D, --debug           Enable debug messages

Get help:
  -h, --help	Help for any command or subcommand

Use "hubble [command] --help" for more information about a command.
"#;

#[cfg(test)]
const HUBBLE_OBSERVE_HELP_TEMPLATE: &str = include_str!("observe_help.txt");

#[cfg(test)]
const OBSERVE_RAW_FILTER_ARGS: [&str; 5] = [
    "--allowlist",
    r#"{"source_pod":["kube-system/"]}"#,
    "--denylist",
    r#"{"source_ip":["1.1.1.1"]}"#,
    "--print-raw-filters",
];

#[cfg(test)]
const OBSERVE_RAW_FILTER_OUTPUT: &str = r#"allowlist:
    - '{"source_pod":["kube-system/"]}'
denylist:
    - '{"source_ip":["1.1.1.1"]}'
"#;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum HubbleCliAction {
    RootHelp,
    ObserveHelp,
    ObserveRequest(Box<ObserveCliRequest>),
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ObserveServerOptions {
    server: String,
    tls: bool,
    tls_allow_insecure: bool,
}

#[cfg(test)]
impl Default for ObserveServerOptions {
    fn default() -> Self {
        Self {
            server: String::from("localhost:4245"),
            tls: false,
            tls_allow_insecure: false,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ObserveFilterOptions {
    from_pod: Vec<String>,
    event_types: Vec<String>,
}

#[cfg(test)]
impl ObserveFilterOptions {
    fn raw_allowlist(&self) -> std::result::Result<Vec<String>, String> {
        let mut allowlist = Vec::new();
        append_json_filter(&mut allowlist, "source_pod", &self.from_pod)?;
        append_json_filter(&mut allowlist, "event_type", &self.event_types)?;
        Ok(allowlist)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ObserveCliRequest {
    formatting: FormattingOptions,
    selector: SelectorOptions,
    masks: MaskOptions,
    other: OtherOptions,
    server: ObserveServerOptions,
    filters: ObserveFilterOptions,
    allowlist: Vec<String>,
    denylist: Vec<String>,
    print_raw_filters: bool,
}

#[cfg(test)]
impl ObserveCliRequest {
    fn to_flow_request(&self) -> std::result::Result<FlowRequest, String> {
        let mut masks = self.masks.clone();
        apply_flow_args(&self.formatting, &mut masks)?;

        let mut selector = self.selector.clone();
        let mut allowlist = self.filters.raw_allowlist()?;
        allowlist.extend(self.allowlist.iter().cloned());

        get_flows_request(
            &mut selector,
            &masks,
            &self.other,
            &allowlist,
            &self.denylist,
        )
    }
}

#[cfg(test)]
fn append_json_filter(
    filters: &mut Vec<String>,
    key: &str,
    values: &[String],
) -> std::result::Result<(), String> {
    if values.is_empty() {
        return Ok(());
    }

    let values = serde_json::to_string(values).map_err(|error| error.to_string())?;
    filters.push(format!(r#"{{"{key}":{values}}}"#));
    Ok(())
}

#[cfg(test)]
fn default_hubble_config_file() -> String {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return std::path::PathBuf::from(config_home)
            .join("hubble")
            .join("config.yaml")
            .display()
            .to_string();
    }

    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home)
            .join(".config")
            .join("hubble")
            .join("config.yaml")
            .display()
            .to_string();
    }

    String::new()
}

#[cfg(test)]
fn render_hubble_root_help() -> String {
    HUBBLE_ROOT_HELP_TEMPLATE.replacen("{}", &default_hubble_config_file(), 1)
}

#[cfg(test)]
fn render_hubble_observe_help() -> String {
    HUBBLE_OBSERVE_HELP_TEMPLATE.replacen("%s", &default_hubble_config_file(), 1)
}

#[cfg(test)]
fn parse_hubble_cli(args: &[&str]) -> std::result::Result<HubbleCliAction, String> {
    if args.is_empty() || matches!(args[0], "--help" | "-h") {
        return Ok(HubbleCliAction::RootHelp);
    }
    if args[0] != "observe" {
        return Err(format!("unsupported hubble command '{}'", args[0]));
    }

    let mut position = 1;
    if matches!(args.get(position), Some(&"flows")) {
        position += 1;
    }
    if matches!(args.get(position), Some(&"--help" | &"-h")) {
        return Ok(HubbleCliAction::ObserveHelp);
    }

    let mut request = ObserveCliRequest::default();
    while let Some(arg) = args.get(position) {
        match *arg {
            "-o" | "--output" => {
                request.formatting.output = take_flag_value(args, &mut position, arg)?;
            }
            "--server" => {
                request.server.server = take_flag_value(args, &mut position, arg)?;
            }
            "--tls" => {
                request.server.tls = true;
                position += 1;
            }
            "--tls-allow-insecure" => {
                request.server.tls_allow_insecure = true;
                position += 1;
            }
            "--from-pod" => {
                request
                    .filters
                    .from_pod
                    .push(take_flag_value(args, &mut position, arg)?);
            }
            "-t" | "--type" => {
                request
                    .filters
                    .event_types
                    .push(take_flag_value(args, &mut position, arg)?);
            }
            "--allowlist" => {
                request
                    .allowlist
                    .push(take_flag_value(args, &mut position, arg)?);
            }
            "--denylist" => {
                request
                    .denylist
                    .push(take_flag_value(args, &mut position, arg)?);
            }
            "--print-raw-filters" => {
                request.print_raw_filters = true;
                position += 1;
            }
            "--help" | "-h" => {
                return Ok(HubbleCliAction::ObserveHelp);
            }
            _ => {
                return Err(format!("unsupported hubble observe argument '{arg}'"));
            }
        }
    }

    Ok(HubbleCliAction::ObserveRequest(Box::new(request)))
}

#[cfg(test)]
fn take_flag_value(
    args: &[&str],
    position: &mut usize,
    flag: &str,
) -> std::result::Result<String, String> {
    let value = args
        .get(*position + 1)
        .ok_or_else(|| format!("missing value for {flag}"))?;
    *position += 2;
    Ok((*value).to_owned())
}

#[cfg(test)]
fn execute_hubble_cli(args: &[&str]) -> std::result::Result<String, String> {
    match parse_hubble_cli(args)? {
        HubbleCliAction::RootHelp => Ok(render_hubble_root_help()),
        HubbleCliAction::ObserveHelp => Ok(render_hubble_observe_help()),
        HubbleCliAction::ObserveRequest(request) => {
            let flow_request = request.to_flow_request()?;
            if request.print_raw_filters {
                get_flow_filters_yaml(&flow_request)
            } else {
                Ok(String::new())
            }
        }
    }
}

// ============================================================================
// Parity tests — ported from hubble/cmd/cli_test.go and
//                           hubble/cmd/observe/flows_test.go
// ============================================================================

#[cfg(test)]
mod parity_tests {
    use super::*;

    // ------ hubble/cmd/cli_test.go ------

    /// Exercises the Hubble observe CLI surface with the same argument combinations
    /// covered by hubble/cmd/cli_test.go, using a lightweight parser and request
    /// builder instead of the full cobra command tree.
    #[test]
    fn parity_hubble_observe_cli_variants() {
        assert_eq!(
            execute_hubble_cli(&["observe"]).expect("observe without flags"),
            ""
        );

        let formatting_request = match parse_hubble_cli(&["observe", "-o", "json"])
            .expect("formatting flags should parse")
        {
            HubbleCliAction::ObserveRequest(request) => request,
            action => panic!("expected observe request, got {action:?}"),
        };
        let formatting_request = formatting_request
            .to_flow_request()
            .expect("json formatting should build a request");
        assert_eq!(formatting_request.number, FLOW_PRINT_COUNT);
        assert!(formatting_request.field_mask.is_empty());

        let server_request = match parse_hubble_cli(&[
            "observe",
            "--server",
            "foo.example.org",
            "--tls",
            "--tls-allow-insecure",
        ])
        .expect("server flags should parse")
        {
            HubbleCliAction::ObserveRequest(request) => request,
            action => panic!("expected observe request, got {action:?}"),
        };
        assert_eq!(server_request.server.server, "foo.example.org");
        assert!(server_request.server.tls);
        assert!(server_request.server.tls_allow_insecure);
        assert_eq!(
            execute_hubble_cli(&[
                "observe",
                "--server",
                "foo.example.org",
                "--tls",
                "--tls-allow-insecure",
            ])
            .expect("server flags should execute"),
            ""
        );

        let filter_request =
            match parse_hubble_cli(&["observe", "--from-pod", "foo/test-pod-1234", "--type", "l7"])
                .expect("filter flags should parse")
            {
                HubbleCliAction::ObserveRequest(request) => request,
                action => panic!("expected observe request, got {action:?}"),
            };
        let filter_request = filter_request
            .to_flow_request()
            .expect("filter flags should build a request");
        let encoded_allowlist =
            serde_json::to_string(&filter_request.whitelist).expect("allowlist should encode");
        assert_eq!(
            encoded_allowlist,
            r#"[{"source_pod":["foo/test-pod-1234"]},{"event_type":["l7"]}]"#
        );

        assert_eq!(
            execute_hubble_cli(&["--help"]).expect("global help should render"),
            render_hubble_root_help()
        );
        assert_eq!(
            execute_hubble_cli(&["observe", "--help"]).expect("observe help should render"),
            render_hubble_observe_help()
        );

        let mut observe_raw_args = vec!["observe"];
        observe_raw_args.extend(OBSERVE_RAW_FILTER_ARGS);
        assert_eq!(
            execute_hubble_cli(&observe_raw_args).expect("observe raw filters should render"),
            OBSERVE_RAW_FILTER_OUTPUT
        );

        let mut observe_flows_raw_args = vec!["observe", "flows"];
        observe_flows_raw_args.extend(OBSERVE_RAW_FILTER_ARGS);
        assert_eq!(
            execute_hubble_cli(&observe_flows_raw_args)
                .expect("observe flows raw filters should render"),
            OBSERVE_RAW_FILTER_OUTPUT
        );
    }

    // ------ hubble/cmd/observe/flows_test.go ------

    /// Validates that flowEventTypes slice stays in sync with monitorAPI.MessageTypeNames
    /// (excluding agent and debug types).
    #[test]
    fn parity_event_types_sync_with_monitor_api() {
        assert_eq!(FLOW_EVENT_TYPES.len(), MONITOR_MESSAGE_TYPE_NAMES.len() - 2);
        for event_type in FLOW_EVENT_TYPES {
            assert!(MONITOR_MESSAGE_TYPE_NAMES.contains(&event_type));
        }
        for event_type in MONITOR_MESSAGE_TYPE_NAMES {
            if event_type == "agent" || event_type == "debug" {
                continue;
            }
            assert!(FLOW_EVENT_TYPES.contains(&event_type));
        }
    }

    /// Tests getFlowsRequest with no since/until sets Number=defaults.FlowPrintCount;
    /// with both since and until set, the Since/Until protobuf timestamps are populated.
    #[test]
    fn parity_get_flows_request_since_until() {
        let mut selector = SelectorOptions::default();
        let masks = MaskOptions {
            field_mask: DEFAULT_FIELD_MASK.iter().map(ToString::to_string).collect(),
            use_default_masks: true,
        };
        let other = OtherOptions::default();
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.number, FLOW_PRINT_COUNT);
        assert_eq!(request.field_mask, masks.field_mask);

        selector.since = Some(String::from("2021-03-23T00:00:00Z"));
        selector.until = Some(String::from("2021-03-24T00:00:00Z"));
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.number, FLOW_PRINT_COUNT);
        assert_eq!(request.since.as_deref(), Some("2021-03-23T00:00:00Z"));
        assert_eq!(request.until.as_deref(), Some("2021-03-24T00:00:00Z"));
    }

    /// Tests getFlowsRequest when only until is set (no since).
    #[test]
    fn parity_get_flows_request_without_since() {
        let mut selector = SelectorOptions::default();
        let masks = MaskOptions {
            field_mask: DEFAULT_FIELD_MASK.iter().map(ToString::to_string).collect(),
            use_default_masks: true,
        };
        let other = OtherOptions::default();
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.number, FLOW_PRINT_COUNT);
        assert!(request.since.is_none());
        assert!(request.until.is_none());

        selector.until = Some(String::from("2021-03-24T00:00:00Z"));
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.until.as_deref(), Some("2021-03-24T00:00:00Z"));
    }

    /// Tests getFlowsRequest correctly encodes raw allowlist/denylist JSON filters into
    /// the protobuf whitelist/blacklist fields.
    #[test]
    fn parity_get_flows_request_raw_filters() {
        let mut selector = SelectorOptions::default();
        let masks = MaskOptions::default();
        let other = OtherOptions::default();
        let allowlist = vec![
            String::from(
                r#"{"source_label":["k8s:io.kubernetes.pod.namespace=kube-system","reserved:host"]}"#,
            ),
            String::from(
                r#"{"destination_label":["k8s:io.kubernetes.pod.namespace=kube-system","reserved:host"]}"#,
            ),
        ];
        let denylist = vec![
            String::from(r#"{"source_label":["k8s:k8s-app=kube-dns"]}"#),
            String::from(r#"{"destination_label":["k8s:k8s-app=kube-dns"]}"#),
        ];

        let request = get_flows_request(&mut selector, &masks, &other, &allowlist, &denylist)
            .expect("request");
        let encoded_allowlist =
            serde_json::to_string(&request.whitelist).expect("allowlist should serialize");
        let encoded_denylist =
            serde_json::to_string(&request.blacklist).expect("denylist should serialize");
        assert_eq!(encoded_allowlist, format!("[{}]", allowlist.join(",")));
        assert_eq!(encoded_denylist, format!("[{}]", denylist.join(",")));
    }

    /// Tests that invalid raw filter JSON returns a descriptive error for both allowlist
    /// and denylist.
    #[test]
    fn parity_get_flows_request_invalid_raw_filters() {
        let mut selector = SelectorOptions::default();
        let masks = MaskOptions::default();
        let other = OtherOptions::default();
        let filters = vec![String::from(r#"{"invalid":["filters"]}"#)];

        let error = get_flows_request(&mut selector, &masks, &other, &filters, &[])
            .expect_err("allowlist should fail");
        assert!(error.contains("invalid --allowlist flag"));
        assert!(error.contains(r#"{"invalid":["filters"]}"#));

        let error = get_flows_request(&mut selector, &masks, &other, &[], &filters)
            .expect_err("denylist should fail");
        assert!(error.contains("invalid --denylist flag"));
        assert!(error.contains(r#"{"invalid":["filters"]}"#));
    }

    /// Tests getFlowFiltersYAML renders whitelist/blacklist filters as YAML with
    /// allowlist/denylist keys.
    #[test]
    fn parity_get_flow_filters_yaml() {
        let request = FlowRequest {
            whitelist: vec![
                serde_json::from_str(r#"{"source_ip":["1.2.3.4/16"]}"#).expect("valid filter"),
            ],
            blacklist: vec![
                serde_json::from_str(r#"{"source_port":["80"]}"#).expect("valid filter"),
            ],
            ..Default::default()
        };

        let output = get_flow_filters_yaml(&request).expect("yaml output");
        let expected = "allowlist:\n    - '{\"source_ip\":[\"1.2.3.4/16\"]}'\ndenylist:\n    - '{\"source_port\":[\"80\"]}'\n";
        assert_eq!(output, expected);
    }

    /// Tests getFlowsRequest with a valid explicit field mask.
    #[test]
    fn parity_get_flows_request_field_mask_valid() {
        let mut selector = SelectorOptions::default();
        let masks = MaskOptions {
            field_mask: vec![String::from("time"), String::from("verdict")],
            use_default_masks: false,
        };
        let other = OtherOptions::default();
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.number, FLOW_PRINT_COUNT);
        assert_eq!(request.field_mask, vec!["time", "verdict"]);
    }

    /// Tests getFlowsRequest returns an error for an invalid field mask path.
    #[test]
    fn parity_get_flows_request_field_mask_invalid() {
        let mut selector = SelectorOptions::default();
        let masks = MaskOptions {
            field_mask: vec![
                String::from("time"),
                String::from("verdict"),
                String::from("invalid-field"),
            ],
            use_default_masks: false,
        };
        let other = OtherOptions::default();
        let error = get_flows_request(&mut selector, &masks, &other, &[], &[])
            .expect_err("invalid field mask should fail");
        assert!(error.contains("invalid-field"));
    }

    /// Tests that maskOpts.useDefaultMasks + dict output applies the default field mask.
    #[test]
    fn parity_get_flows_request_use_default_field_mask() {
        let formatting = FormattingOptions {
            output: String::from("dict"),
        };
        let mut masks = MaskOptions {
            field_mask: Vec::new(),
            use_default_masks: true,
        };
        apply_flow_args(&formatting, &mut masks).expect("dict output should be valid");

        let mut selector = SelectorOptions::default();
        let other = OtherOptions::default();
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        let default_mask = DEFAULT_FIELD_MASK
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(request.field_mask, default_mask);
    }

    /// Tests that a non-JSON output format (compact) combined with an explicit field mask
    /// returns a "not compatible" error from handleFlowArgs.
    #[test]
    fn parity_get_flows_request_field_mask_non_json_output() {
        let formatting = FormattingOptions {
            output: String::from("compact"),
        };
        let mut masks = MaskOptions {
            field_mask: vec![String::from("time"), String::from("verdict")],
            use_default_masks: true,
        };
        let error = apply_flow_args(&formatting, &mut masks).expect_err("should be incompatible");
        assert!(error.contains("not compatible"));
    }

    /// Tests that --input-file suppresses the default Number in GetFlowsRequest, but an
    /// explicit --last flag overrides that.
    #[test]
    fn parity_get_flows_request_with_input_file() {
        let masks = MaskOptions::default();
        let mut selector = SelectorOptions::default();
        let mut other = OtherOptions {
            input_file: Some(String::from("myfile")),
        };
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.number, 0);

        selector.last = 42;
        other.input_file = Some(String::from("myfile"));
        let request = get_flows_request(&mut selector, &masks, &other, &[], &[]).expect("request");
        assert_eq!(request.number, 42);
    }
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
                .with_identity(SecurityIdentity::remote_node()),
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
        assert_eq!(report.summary, FlowSummary::default());
        assert!(report.observations.is_empty());
    }

    fn make_test_flow() -> Flow {
        Flow {
            time: None,
            verdict: Verdict::Forwarded,
            drop_reason: 0,
            source: Endpoint::default(),
            destination: Endpoint::default(),
            l4: None,
            traffic_direction: TrafficDirection::Egress,
            reply: false,
            is_reply: None,
            node_name: String::from("node1"),
            event_type: None,
        }
    }

    #[test]
    fn test_flow_ring_eviction() {
        let mut ring = FlowRing::new(3);
        for i in 0..5_u32 {
            let mut flow = make_test_flow();
            flow.drop_reason = i;
            ring.push(flow);
        }

        assert_eq!(ring.len(), 3);
        let reasons: Vec<u32> = ring.iter().map(|flow| flow.drop_reason).collect();
        assert_eq!(reasons, vec![2, 3, 4]);
    }

    #[test]
    fn test_verdict_filter() {
        let filter = VerdictFilter {
            verdicts: vec![Verdict::Dropped],
        };
        let mut flow = make_test_flow();
        flow.verdict = Verdict::Dropped;
        assert!(filter.matches(&flow));

        flow.verdict = Verdict::Forwarded;
        assert!(!filter.matches(&flow));
    }

    #[test]
    fn test_and_filter() {
        let filter = AndFilter {
            filters: vec![
                Box::new(VerdictFilter {
                    verdicts: vec![Verdict::Dropped],
                }),
                Box::new(DirectionFilter {
                    direction: TrafficDirection::Ingress,
                }),
            ],
        };
        let mut flow = make_test_flow();
        flow.verdict = Verdict::Dropped;
        flow.traffic_direction = TrafficDirection::Ingress;
        assert!(filter.matches(&flow));

        flow.verdict = Verdict::Forwarded;
        assert!(!filter.matches(&flow));
    }

    #[test]
    fn test_last_n() {
        let mut ring = FlowRing::new(10);
        for i in 0..7_u32 {
            let mut flow = make_test_flow();
            flow.drop_reason = i;
            ring.push(flow);
        }

        let last_three = ring.last_n(3);
        assert_eq!(last_three.len(), 3);
        assert_eq!(last_three[2].drop_reason, 6);
    }

    #[test]
    fn test_try_new_rejects_zero_capacity() {
        assert!(matches!(
            FlowRing::try_new(0),
            Err(HubbleError::InvalidCapacity)
        ));
    }
}
