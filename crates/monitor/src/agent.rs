use std::sync::Arc;

use tokio::sync::RwLock;

use crate::Payload;

/// A sink for monitor payloads.
pub trait MonitorConsumer: Send + Sync {
    /// Receives a monitor payload.
    fn notify(&self, payload: &Payload);
}

/// In-process monitor event fan-out for registered consumers.
#[derive(Clone, Default)]
pub struct AgentMonitor {
    consumers: Arc<RwLock<Vec<Arc<dyn MonitorConsumer>>>>,
}

impl AgentMonitor {
    /// Creates an empty agent monitor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a consumer to receive future monitor payloads.
    pub fn register_consumer(&self, consumer: Arc<dyn MonitorConsumer>) {
        let mut consumers = self.consumers.blocking_write();
        consumers.push(consumer);
        tracing::debug!(consumers = consumers.len(), "registered monitor consumer");
    }

    /// Removes all registered consumers.
    pub fn unregister_all(&self) {
        self.consumers.blocking_write().clear();
        tracing::debug!("unregistered all monitor consumers");
    }

    /// Sends an event to every currently registered consumer.
    pub fn send_event(&self, payload: &Payload) {
        let consumers = self.consumers.blocking_read().clone();
        tracing::debug!(
            consumers = consumers.len(),
            payload_type = ?payload.payload_type,
            "dispatching monitor payload"
        );
        for consumer in consumers {
            consumer.notify(payload);
        }
    }

    /// Returns the number of registered consumers.
    #[must_use]
    pub fn consumer_count(&self) -> usize {
        self.consumers.blocking_read().len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    struct CountingConsumer {
        counter: Arc<AtomicU32>,
    }

    impl MonitorConsumer for CountingConsumer {
        fn notify(&self, _payload: &Payload) {
            self.counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn agent_monitor_registers_and_dispatches() {
        let monitor = AgentMonitor::new();
        let counter = Arc::new(AtomicU32::new(0));
        let consumer = Arc::new(CountingConsumer {
            counter: Arc::clone(&counter),
        });

        monitor.register_consumer(consumer);
        assert_eq!(monitor.consumer_count(), 1);

        monitor.send_event(&Payload::new_event(vec![1, 2, 3], 0));
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        monitor.unregister_all();
        assert_eq!(monitor.consumer_count(), 0);

        monitor.send_event(&Payload::new_lost(4, 1));
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }
}
