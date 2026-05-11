//! Priority queue for flow sorting by timestamp.
//!
//! Flows are ordered by their timestamp, with older flows having higher priority.
//! This enables chronological delivery of flow observations.

use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

/// A flow record with timestamp
#[derive(Debug, Clone)]
pub struct Flow {
    /// Unique flow identifier
    pub id: String,
    /// Timestamp when flow was observed (seconds since epoch)
    pub timestamp_secs: u64,
    /// Timestamp nanoseconds component
    pub timestamp_nanos: u32,
    /// Source address
    pub src_addr: String,
    /// Destination address
    pub dst_addr: String,
    /// Protocol (tcp, udp, etc.)
    pub protocol: String,
}

impl Flow {
    /// Creates a new flow record
    pub fn new(
        id: impl Into<String>,
        timestamp_secs: u64,
        timestamp_nanos: u32,
        src_addr: impl Into<String>,
        dst_addr: impl Into<String>,
        protocol: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            timestamp_secs,
            timestamp_nanos,
            src_addr: src_addr.into(),
            dst_addr: dst_addr.into(),
            protocol: protocol.into(),
        }
    }

    /// Returns the flow's timestamp as SystemTime
    pub fn as_system_time(&self) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::new(self.timestamp_secs, self.timestamp_nanos)
    }
}

impl Ord for Flow {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse order so oldest flows have highest priority (min-heap behavior)
        if self.timestamp_secs == other.timestamp_secs {
            other.timestamp_nanos.cmp(&self.timestamp_nanos)
        } else {
            other.timestamp_secs.cmp(&self.timestamp_secs)
        }
    }
}

impl PartialOrd for Flow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Flow {}

impl PartialEq for Flow {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.timestamp_secs == other.timestamp_secs
    }
}

/// Priority queue for flows sorted by timestamp (oldest first)
pub struct PriorityQueue {
    /// Internal heap (min-heap ordered by Flow's Ord impl)
    heap: Vec<Flow>,
    /// Maximum capacity
    max_len: usize,
}

impl PriorityQueue {
    /// Creates a new priority queue with initial capacity
    pub fn new(max_len: usize) -> Self {
        Self {
            heap: Vec::with_capacity(max_len),
            max_len,
        }
    }

    /// Returns the number of flows in the queue
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Returns true if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Returns true if the queue is at maximum capacity
    pub fn is_full(&self) -> bool {
        self.heap.len() >= self.max_len
    }

    /// Pushes a flow into the queue
    pub fn push(&mut self, flow: Flow) {
        self.heap.push(flow);
        self.heapify_up(self.heap.len() - 1);
    }

    /// Pops the oldest (highest priority) flow from the queue
    pub fn pop(&mut self) -> Option<Flow> {
        if self.heap.is_empty() {
            return None;
        }
        let result = self.heap.swap_remove(0);
        if !self.heap.is_empty() {
            self.heapify_down(0);
        }
        Some(result)
    }

    /// Pops all flows older than the given timestamp
    pub fn pop_older_than(&mut self, cutoff_secs: u64, cutoff_nanos: u32) -> Vec<Flow> {
        let mut result = Vec::new();

        while let Some(flow) = self.pop() {
            if flow.timestamp_secs > cutoff_secs
                || (flow.timestamp_secs == cutoff_secs && flow.timestamp_nanos >= cutoff_nanos)
            {
                // Flow is too new, put it back
                self.push(flow);
                break;
            }
            result.push(flow);
        }

        result
    }

    /// Returns all flows without removing them
    pub fn peek_all(&self) -> Vec<&Flow> {
        self.heap.iter().collect()
    }

    fn heapify_up(&mut self, mut idx: usize) {
        while idx > 0 {
            let parent = (idx - 1) / 2;
            if self.heap[idx] > self.heap[parent] {
                self.heap.swap(idx, parent);
                idx = parent;
            } else {
                break;
            }
        }
    }

    fn heapify_down(&mut self, mut idx: usize) {
        let len = self.heap.len();
        loop {
            let mut largest = idx;
            let left = 2 * idx + 1;
            let right = 2 * idx + 2;

            if left < len && self.heap[left] > self.heap[largest] {
                largest = left;
            }
            if right < len && self.heap[right] > self.heap[largest] {
                largest = right;
            }

            if largest == idx {
                break;
            }
            self.heap.swap(idx, largest);
            idx = largest;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_flow(id: &str, secs: u64, nanos: u32) -> Flow {
        Flow::new(id, secs, nanos, "10.0.0.1", "10.0.0.2", "tcp")
    }

    #[test]
    fn priority_queue_maintains_order() {
        let mut pq = PriorityQueue::new(10);

        // Push flows out of order
        pq.push(sample_flow("flow-3", 300, 0));
        pq.push(sample_flow("flow-1", 100, 0));
        pq.push(sample_flow("flow-2", 200, 0));

        // Pop should return in chronological order (oldest first)
        assert_eq!(pq.pop().map(|f| f.id), Some("flow-1".to_string()));
        assert_eq!(pq.pop().map(|f| f.id), Some("flow-2".to_string()));
        assert_eq!(pq.pop().map(|f| f.id), Some("flow-3".to_string()));
        assert_eq!(pq.pop(), None);
    }

    #[test]
    fn priority_queue_handles_same_seconds_different_nanos() {
        let mut pq = PriorityQueue::new(10);

        pq.push(sample_flow("flow-2", 100, 500));
        pq.push(sample_flow("flow-1", 100, 100));

        // Older nanos should come first
        assert_eq!(pq.pop().map(|f| f.id), Some("flow-1".to_string()));
        assert_eq!(pq.pop().map(|f| f.id), Some("flow-2".to_string()));
    }

    #[test]
    fn priority_queue_pop_older_than() {
        let mut pq = PriorityQueue::new(10);

        pq.push(sample_flow("flow-1", 100, 0));
        pq.push(sample_flow("flow-2", 200, 0));
        pq.push(sample_flow("flow-3", 300, 0));

        let older = pq.pop_older_than(250, 0);

        // Should return flows older than 250 seconds
        assert_eq!(older.len(), 2);
        assert_eq!(older[0].id, "flow-1");
        assert_eq!(older[1].id, "flow-2");

        // Queue should still have flow-3
        assert_eq!(pq.pop().map(|f| f.id), Some("flow-3".to_string()));
    }

    #[test]
    fn priority_queue_capacity() {
        let pq = PriorityQueue::new(5);
        assert!(!pq.is_full());
        assert!(pq.is_empty());
        assert_eq!(pq.len(), 0);
    }

    #[test]
    fn priority_queue_peek_all() {
        let mut pq = PriorityQueue::new(10);
        pq.push(sample_flow("flow-1", 100, 0));
        pq.push(sample_flow("flow-2", 200, 0));

        let all = pq.peek_all();
        assert_eq!(all.len(), 2);
    }
}
