//! Core monitor payloads, event layouts, and scaffold models ported from Cilium.

mod agent;
mod api;
mod events;
mod payload;
mod scaffold;

pub use agent::{AgentMonitor, MonitorConsumer};
pub use api::{DropReason, MessageType};
pub use events::{DropNotify, EventDecodeError, TraceNotify};
pub use payload::{Payload, PayloadError, PayloadType};
pub use scaffold::{COMPONENT, MonitorModel, MonitorReport, MonitorTarget, ProbeStatus, scaffold};
