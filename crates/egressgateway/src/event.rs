// SPDX-License-Identifier: Apache-2.0
// Copyright Authors of Cilium

//! Event handling for egress gateway

use std::fmt;

use crate::endpoint::EndpointMetadata;
use crate::policy::PolicyConfig;
use crate::types::Node;

/// Event type for egress gateway resource changes
#[derive(Debug, Clone)]
pub enum ResourceEvent {
    /// Policy was added or updated
    PolicyUpsert(Box<PolicyConfig>),
    /// Policy was deleted
    PolicyDelete(String), // policy ID

    /// Endpoint was added or updated
    EndpointUpsert(Box<EndpointMetadata>),
    /// Endpoint was deleted
    EndpointDelete(String), // endpoint ID

    /// Node was added or updated
    NodeUpsert(Box<Node>),
    /// Node was deleted
    NodeDelete(String), // node name

    /// K8s initial sync complete
    SyncComplete,
}

impl fmt::Display for ResourceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyUpsert(_) => write!(f, "PolicyUpsert"),
            Self::PolicyDelete(id) => write!(f, "PolicyDelete({id})"),
            Self::EndpointUpsert(ep) => write!(f, "EndpointUpsert({:?})", ep.id),
            Self::EndpointDelete(id) => write!(f, "EndpointDelete({id})"),
            Self::NodeUpsert(n) => write!(f, "NodeUpsert({name})", name = n.name),
            Self::NodeDelete(name) => write!(f, "NodeDelete({name})"),
            Self::SyncComplete => write!(f, "SyncComplete"),
        }
    }
}

/// Event handler trait
pub trait EventHandler: Send + Sync {
    /// Handle a resource event
    fn handle_event(&self, event: ResourceEvent);
}

/// Multi-handler for delegating to multiple handlers
pub struct MultiHandler {
    handlers: Vec<Box<dyn EventHandler>>,
}

impl MultiHandler {
    /// Create a new multi-handler
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Add an event handler
    pub fn add_handler(&mut self, handler: Box<dyn EventHandler>) {
        self.handlers.push(handler);
    }
}

impl Default for MultiHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHandler for MultiHandler {
    fn handle_event(&self, event: ResourceEvent) {
        for handler in &self.handlers {
            handler.handle_event(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountingHandler {
        count: Arc<AtomicUsize>,
    }

    impl EventHandler for CountingHandler {
        fn handle_event(&self, _event: ResourceEvent) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_resource_event_display() {
        let event = ResourceEvent::SyncComplete;
        assert_eq!(event.to_string(), "SyncComplete");
    }

    #[test]
    fn test_multi_handler() {
        let count1 = Arc::new(AtomicUsize::new(0));
        let count2 = Arc::new(AtomicUsize::new(0));

        let mut multi = MultiHandler::new();
        multi.add_handler(Box::new(CountingHandler {
            count: count1.clone(),
        }));
        multi.add_handler(Box::new(CountingHandler {
            count: count2.clone(),
        }));

        let event = ResourceEvent::SyncComplete;
        multi.handle_event(event);

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }
}
