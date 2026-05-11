# Service Subsystem Implementation Specification

**Version**: 1.0  
**Status**: ACTIVE  
**Last Updated**: May 11, 2026  
**Target Release**: v0.1.0

---

## Overview

This document defines the technical specifications for implementing the Cilium service subsystem in Rust, enabling load balancing of Kubernetes services via eBPF-based datapath.

The service subsystem consists of 4 parallel implementation tracks that together enable service discovery, backend mapping, load balancing, and health tracking.

---

## Architecture

### High-Level Flow

```
Kubernetes API Server
        ↓
    [Service Watch]  ←─ Track 1: Service Observer
        ↓
    [Cache Layer]    ←─ In-memory service/endpoint cache
        ↓
    [Backend Discovery]  ←─ Track 3: Backend Mapping
        ↓
    [eBPF Maps]  ←─ Track 2: eBPF Maps  
        ↓
    [Load Balancer]  ←─ Track 4: Load Balancing Algorithm
        ↓
    Kernel eBPF Programs
        ↓
    Datapath (packet forwarding)
```

### Component Responsibilities

| Track | Component | Role |
|-------|-----------|------|
| **1** | Service Observer | Watch K8s services, maintain cache, dispatch events |
| **2** | eBPF Maps | Store service/backend data in BPF maps, provide queries |
| **3** | Backend Mapping | Discover backends from services, keep mapping updated |
| **4** | Load Balancer | Select backends, apply algorithms, track affinity |

---

## Track 1: Service Observer (crates/service-observer)

### Purpose
Watch Kubernetes services and maintain an in-memory cache of service state.

### Core Types

#### ServiceInfo
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub namespace: String,
    pub name: String,
    pub service_type: ServiceType,  // ClusterIP, NodePort, LoadBalancer
    pub cluster_ip: Option<IpAddr>,
    pub selector: HashMap<String, String>,
    pub ports: Vec<ServicePort>,
    pub session_affinity: SessionAffinity,
    pub load_balancer_ip: Option<IpAddr>,
    pub external_ips: Vec<IpAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceType {
    ClusterIP,
    NodePort,
    LoadBalancer,
    ExternalName,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePort {
    pub name: Option<String>,
    pub protocol: String,  // TCP, UDP
    pub port: u16,         // Service port
    pub target_port: u16,  // Pod port
    pub node_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionAffinity {
    None,
    ClientIP { timeout_seconds: u32 },
}
```

#### ServiceCache
```rust
pub struct ServiceCache {
    services: HashMap<ServiceKey, ServiceInfo>,
    selectors: HashMap<String, Vec<ServiceKey>>,  // Fast selector lookup
}

pub type ServiceKey = String;  // Format: "namespace/name"

impl ServiceCache {
    pub fn new() -> Self;
    
    // Mutations
    pub fn add_service(&mut self, svc: ServiceInfo) -> Result<()>;
    pub fn update_service(&mut self, svc: ServiceInfo) -> Result<()>;
    pub fn delete_service(&mut self, key: &ServiceKey) -> Result<()>;
    
    // Queries
    pub fn get_service(&self, key: &ServiceKey) -> Option<&ServiceInfo>;
    pub fn get_service_by_ip(&self, ip: &IpAddr) -> Option<&ServiceInfo>;
    pub fn list_services(&self) -> Vec<&ServiceInfo>;
    pub fn find_services_by_selector(&self, selector: &HashMap<String, String>) -> Vec<&ServiceInfo>;
    pub fn services_for_namespace(&self, namespace: &str) -> Vec<&ServiceInfo>;
    pub fn service_count(&self) -> usize;
    
    // Watch support
    pub fn version(&self) -> u64;  // For incremental updates
}
```

#### ServiceWatcher
```rust
pub struct ServiceWatcher {
    client: Client,
    namespace: String,  // "" for all namespaces
}

impl ServiceWatcher {
    pub async fn new() -> Result<Self>;
    pub async fn new_for_namespace(namespace: String) -> Result<Self>;
    
    pub async fn watch_services<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(WatchEvent) + Send + 'static;
}

#[derive(Debug, Clone)]
pub enum WatchEvent {
    Added(ServiceInfo),
    Modified(ServiceInfo),
    Deleted(ServiceKey),
}
```

#### ServiceObserver
```rust
pub struct ServiceObserver {
    cache: ServiceCache,
    watcher: Option<ServiceWatcher>,
    event_handlers: Vec<Box<dyn EventHandler>>,
}

pub trait EventHandler: Send {
    async fn on_service_added(&self, svc: &ServiceInfo) -> Result<()>;
    async fn on_service_updated(&self, svc: &ServiceInfo) -> Result<()>;
    async fn on_service_deleted(&self, key: &ServiceKey) -> Result<()>;
}

impl ServiceObserver {
    pub async fn new() -> Result<Self>;
    pub async fn start(&mut self) -> Result<()>;
    pub async fn stop(&mut self) -> Result<()>;
    
    // Cache access
    pub fn get_service(&self, namespace: &str, name: &str) -> Option<ServiceInfo>;
    pub fn list_services(&self) -> Vec<ServiceInfo>;
    pub fn find_services(&self, selector: &HashMap<String, String>) -> Vec<ServiceInfo>;
    
    // Event registration
    pub fn register_handler(&mut self, handler: Box<dyn EventHandler>);
    pub fn unregister_handler(&mut self);
}
```

### Data Flow

1. **Initialization**:
   - Create Client connection to K8s API
   - Initialize empty ServiceCache
   - Create ServiceWatcher

2. **Watch Loop**:
   - Watch ServiceList for changes
   - Receive ADDED/MODIFIED/DELETED events
   - Update local cache
   - Dispatch events to handlers

3. **Event Handlers**:
   - Backend Mapping (Track 3) subscribes to changes
   - Updates backend pools when services change
   - Updates eBPF maps when backends change

### Key Implementation Details

**Cache Key Format**:
```
namespace/name
Example: "default/nginx-svc"
```

**Selector Indexing**:
- Store selector → ServiceKey mapping for fast lookup
- Update indexes on service mutations
- Support efficient pod-to-service matching

**Watch Resilience**:
- Handle connection drops gracefully
- Implement exponential backoff for reconnection
- Verify cache consistency on reconnect

### Success Criteria (Track 1)

- [ ] K8s service watch connects and receives events
- [ ] ServiceCache add/update/delete working correctly
- [ ] All cache queries efficient (<1ms for typical operations)
- [ ] Event handlers called with correct service data
- [ ] Handles service type changes (ClusterIP → LoadBalancer)
- [ ] Selectors indexed and queryable
- [ ] 15+ unit tests, all passing
- [ ] No clippy warnings

---

## Track 2: eBPF Maps (crates/ebpf)

### Purpose
Define and manage eBPF maps for storing service and backend data, accessible from kernel datapath.

### eBPF Map Definitions

#### SVC_MAP (Service Lookup)
```c
// In eBPF program
BPF_HASH(SVC_MAP, u32, svc_entry_t);  // Key: service_id, Value: service metadata

typedef struct {
    u32 vip;           // Virtual IP (cluster IP)
    u16 port;          // Service port
    u8  protocol;      // IPPROTO_TCP, IPPROTO_UDP
    u8  flags;         // Service flags
    u32 backend_count; // Number of backends
    u32 backend_base;  // Base index in BACKEND_MAP
    u32 session_affinity; // 0=None, 1=ClientIP
} svc_entry_t;
```

#### BACKEND_MAP (Backend List)
```c
BPF_ARRAY(BACKEND_MAP, backend_entry_t, MAX_BACKENDS);

typedef struct {
    u32 ip;           // Backend IP (pod IP)
    u16 port;         // Backend port (target port)
    u8  protocol;     // IPPROTO_TCP, IPPROTO_UDP
    u8  state;        // 0=healthy, 1=unhealthy, 2=draining
    u32 weight;       // For weighted LB (default: 1)
    u32 connection_count;  // For monitoring
} backend_entry_t;
```

#### AFFINITY_MAP (Session Affinity)
```c
BPF_HASH(AFFINITY_MAP, client_key_t, u32);  // Key: client info, Value: selected backend index

typedef struct {
    u32 client_ip;     // Client source IP
    u16 client_port;   // Client source port
    u32 service_id;    // Service identifier
    u8  protocol;      // IPPROTO_TCP, IPPROTO_UDP
} client_key_t;
```

#### COUNTERS_MAP (Monitoring)
```c
BPF_HASH(COUNTERS_MAP, counter_key_t, u64);

typedef struct {
    u32 service_id;
    u32 backend_index;
    u8  counter_type;  // 0=packets, 1=bytes, 2=errors
} counter_key_t;
```

### Rust Map Wrapper Types

```rust
pub use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ServiceMapEntry {
    pub vip: u32,
    pub port: u16,
    pub protocol: u8,
    pub flags: u8,
    pub backend_count: u32,
    pub backend_base: u32,
    pub session_affinity: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BackendEntry {
    pub ip: u32,
    pub port: u16,
    pub protocol: u8,
    pub state: u8,  // 0=healthy, 1=unhealthy, 2=draining
    pub weight: u32,
    pub connection_count: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct ClientKey {
    pub client_ip: u32,
    pub client_port: u16,
    pub service_id: u32,
    pub protocol: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CounterKey {
    pub service_id: u32,
    pub backend_index: u32,
    pub counter_type: u8,  // 0=packets, 1=bytes, 2=errors
}
```

### Rust Map Operations

```rust
pub struct ServiceMap {
    fd: i32,  // File descriptor for BPF map
    max_entries: usize,
}

impl ServiceMap {
    pub fn create(name: &str, max_entries: usize) -> Result<Self>;
    
    pub fn add_service(&mut self, key: u32, entry: ServiceMapEntry) -> Result<()>;
    pub fn update_service(&mut self, key: u32, entry: ServiceMapEntry) -> Result<()>;
    pub fn get_service(&self, key: u32) -> Result<Option<ServiceMapEntry>>;
    pub fn delete_service(&mut self, key: u32) -> Result<()>;
    pub fn iterate_services(&self) -> Result<Vec<(u32, ServiceMapEntry)>>;
}

pub struct BackendMap {
    fd: i32,
    max_entries: usize,
}

impl BackendMap {
    pub fn create(name: &str, max_entries: usize) -> Result<Self>;
    
    pub fn add_backend(&mut self, index: u32, entry: BackendEntry) -> Result<()>;
    pub fn update_backend(&mut self, index: u32, entry: BackendEntry) -> Result<()>;
    pub fn get_backend(&self, index: u32) -> Result<Option<BackendEntry>>;
    pub fn delete_backend(&mut self, index: u32) -> Result<()>;
    pub fn count_healthy(&self) -> Result<usize>;
    pub fn get_backends_for_service(&self, service_id: u32, count: u32) -> Result<Vec<BackendEntry>>;
}

pub struct AffinityMap {
    fd: i32,
}

impl AffinityMap {
    pub fn create(name: &str, max_entries: usize) -> Result<Self>;
    
    pub fn set_affinity(&mut self, key: ClientKey, backend_idx: u32) -> Result<()>;
    pub fn get_affinity(&self, key: ClientKey) -> Result<Option<u32>>;
    pub fn delete_affinity(&mut self, key: ClientKey) -> Result<()>;
    pub fn clear_all(&mut self) -> Result<()>;
}

pub struct CountersMap {
    fd: i32,
}

impl CountersMap {
    pub fn create(name: &str, max_entries: usize) -> Result<Self>;
    
    pub fn increment(&mut self, key: CounterKey, delta: u64) -> Result<()>;
    pub fn get_counter(&self, key: CounterKey) -> Result<u64>;
    pub fn get_service_stats(&self, service_id: u32) -> Result<ServiceStats>;
}

#[derive(Debug, Clone)]
pub struct ServiceStats {
    pub total_packets: u64,
    pub total_bytes: u64,
    pub total_errors: u64,
    pub backend_stats: HashMap<u32, BackendStats>,
}

#[derive(Debug, Clone)]
pub struct BackendStats {
    pub packets: u64,
    pub bytes: u64,
    pub errors: u64,
}
```

### Key Implementation Details

**Map Initialization**:
1. Load eBPF programs (compiled .o files)
2. Create maps with specified sizes
3. Pin maps to /sys/fs/bpf for persistence
4. Wrap with Rust types for safe access

**Serialization**:
- All types must serialize to/from binary
- Network byte order for IP addresses
- Ensure struct padding correct (use #[repr(C)])

**Concurrency**:
- BPF maps support concurrent access
- Use atomic operations for counters
- Handle race conditions in updates

### Success Criteria (Track 2)

- [ ] All 4 map types created successfully
- [ ] Service map operations (add/update/get/delete) working
- [ ] Backend map operations working
- [ ] Affinity map working
- [ ] Counter operations working
- [ ] Proper error handling for map operations
- [ ] No resource leaks (maps properly freed)
- [ ] 15+ unit tests, all passing
- [ ] No clippy warnings

---

## Track 3: Backend Mapping Engine (crates/backend-mapping)

### Purpose
Discover backends for services and maintain mapping in eBPF maps.

### Core Types

```rust
pub struct BackendMapper {
    service_observer: Arc<ServiceObserver>,
    pod_cache: PodCache,
    service_to_backends: HashMap<ServiceKey, Vec<BackendEntry>>,
    backend_map: Arc<Mutex<BackendMap>>,
    dirty: bool,  // Track if map needs sync
}

pub struct PodCache {
    pods: HashMap<PodKey, PodInfo>,
}

pub type PodKey = String;  // Format: "namespace/name"

#[derive(Debug, Clone)]
pub struct PodInfo {
    pub namespace: String,
    pub name: String,
    pub labels: HashMap<String, String>,
    pub ip: IpAddr,
    pub port: u16,
    pub status: PodStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PodStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}
```

### Discovery Algorithm

```
For each Service:
  1. Get service selector labels
  2. Query all pods in same namespace
  3. For each pod:
     a. Check if labels match selector
     b. If running, extract pod IP + target port
     c. Create BackendEntry
  4. Update backend pool in map
  5. On pod change, re-trigger discovery for affected services
```

### Data Flow

1. **Startup**:
   - Initialize PodCache (watch all pods)
   - Subscribe to ServiceObserver events
   - Perform initial discovery for all services

2. **On Service Added/Updated**:
   - Call discover_backends() for that service
   - Match pods using label selector
   - Build backend list
   - Update eBPF maps

3. **On Pod Change**:
   - Update pod cache
   - Find services that might be affected
   - Trigger re-discovery
   - Update backend list in eBPF maps

### Success Criteria (Track 3)

- [ ] Pod watch working
- [ ] Pod cache maintaining accurate state
- [ ] Label selector matching correct
- [ ] Backends discovered for services
- [ ] Backend list updated in eBPF maps
- [ ] Handles pod lifecycle (pending→running→deleted)
- [ ] Efficient queries (<100ms for typical service discovery)
- [ ] 15+ unit tests, all passing
- [ ] Integration with Track 1 & 2 verified
- [ ] No clippy warnings

---

## Track 4: Load Balancing Algorithm (crates/loadbalancer)

### Purpose
Select backends using various load balancing algorithms.

### Core Types

```rust
pub enum LBAlgorithm {
    RoundRobin,
    LeastConnections,
    ConsistentHash,
    Random,
}

pub struct LoadBalancer {
    algorithm: LBAlgorithm,
    affinity_map: Arc<Mutex<AffinityMap>>,
    backend_map: Arc<BackendMap>,
    rr_state: Arc<Mutex<RoundRobinState>>,
}

pub struct RoundRobinState {
    current_index: u32,
}

pub struct LBDecision {
    pub backend: BackendEntry,
    pub backend_index: u32,
    pub algorithm_used: LBAlgorithm,
}
```

### Algorithms

#### Round-Robin
```
- Maintain per-service round-robin counter
- On request: backend = backends[counter % backends.len()]
- Increment counter
- Pros: Simple, even distribution
- Cons: Doesn't account for connection count
```

#### Least Connections
```
- For each backend: count active connections
- Select backend with lowest count
- Pros: Balances load better than RR
- Cons: Requires per-backend connection tracking
```

#### Consistent Hash
```
- Hash(client_ip) → position on hash ring
- Map position to nearest backend
- Pros: Affinity maintained even with backend changes
- Cons: More computation
```

### Load Balancing Decision

```rust
impl LoadBalancer {
    pub async fn select_backend(
        &mut self,
        service_id: u32,
        client_ip: IpAddr,
        protocol: u8,
    ) -> Result<LBDecision>;
}
```

**Decision Process**:
1. If SessionAffinity enabled and client has affinity → use stored backend
2. Otherwise, apply algorithm to get backend
3. If affinity enabled, store selection for next time
4. Return decision with backend info

### Success Criteria (Track 4)

- [ ] Round-robin working
- [ ] Least-connections working
- [ ] Consistent hash working
- [ ] Session affinity working
- [ ] Health checks respected (skip unhealthy backends)
- [ ] Efficient selection (<100μs per decision)
- [ ] 15+ unit tests, all passing
- [ ] Integration with Track 1, 2 & 3 verified
- [ ] No clippy warnings

---

## Integration Points

### Track 1 → Track 3
- ServiceObserver events trigger backend re-discovery
- ServiceObserver provides service metadata (selector, etc.)

### Track 2 ← Track 3
- Backend Mapping Engine updates eBPF maps with backends

### Track 3 → Track 4
- Backend list from Track 3 used by Track 4 algorithms
- Shared eBPF maps for data

### Track 4 → Kernel
- Load balancer decisions inform eBPF program actions
- Affinity data stored in AFFINITY_MAP for kernel to read

---

## Testing Strategy

### Unit Tests (Each Track)
- Component-level tests
- Mock K8s API responses
- Test all code paths
- Target 90%+ coverage

### Integration Tests
- Full flow: Service → Backend Discovery → LB Decision
- Real K8s clusters (via kind/test-harness)
- Full datapath testing
- Target: 40+/50 service specs passing

### Performance Tests
- Cache lookup: <1ms
- Service discovery: <100ms
- LB decision: <100μs
- Memory usage: <100MB for typical clusters

---

## Success Metrics for P1 Validation

**Functional**:
- 40+/50 K8sDatapathServicesTest specs passing (80%)
- All 4 tracks integrated and working
- No critical regressions in other tests

**Code Quality**:
- 100% clippy pass
- 90%+ test coverage
- <200 LoC per file (avg)

**Performance**:
- Service lookup: <100μs
- Backend discovery: <100ms
- LB decision: <100μs

---

## Timeline

| Week | Track 1 | Track 2 | Track 3 | Track 4 |
|------|---------|---------|---------|---------|
| W1 D1-5 | Implement | Implement | - | - |
| W2 D6-7 | Complete | Complete | Implement | Implement |
| W2 D8-9 | Optimize | Optimize | Complete | Complete |
| W2 D10 | Testing | Testing | Testing | Testing |

**Target Completion**: May 25, 2026 (Day 9)  
**P1 Validation**: May 26, 2026 (Day 10)  
**v0.1.0 Release**: May 27-June 2, 2026

---

**Document Status**: ACTIVE  
**Last Review**: May 11, 2026  
**Next Review**: After Track 1 implementation complete
