# P1 Implementation Execution Plan

**Status**: IN PROGRESS  
**Start Date**: May 11, 2026  
**Target Completion**: May 25, 2026 (14 days)  
**Target**: 40+/50 service specs passing (80%)

---

## Track Overview

### Track 1: Service Observer (Issue #44)
- **Crate**: `crates/service-observer`
- **Duration**: 5-7 days
- **Owner**: Developer #1 (or parallel with Track 2)
- **Key Responsibilities**:
  - Implement K8s service watch (kube-rs integration)
  - Service cache (HashMap-based in-memory store)
  - Event dispatcher (notify on add/update/delete)
  - Integration with agent lifecycle
  - Unit tests for all components

**Key Files to Implement**:
```rust
// src/lib.rs
pub struct ServiceWatcher { }
pub struct ServiceCache { }
pub struct ServiceObserver { }

impl ServiceWatcher {
  pub async fn watch() -> Result<()> { }
  pub async fn watch_services(selector: LabelSelector) -> Result<()> { }
}

impl ServiceCache {
  pub fn add_service(&mut self, svc: K8sService) { }
  pub fn update_service(&mut self, svc: K8sService) { }
  pub fn delete_service(&mut self, key: String) { }
  pub fn get_service(&self, key: &str) -> Option<&K8sService> { }
  pub fn list_services(&self) -> Vec<&K8sService> { }
}

impl ServiceObserver {
  pub async fn new() -> Self { }
  pub async fn start(&mut self) -> Result<()> { }
  pub async fn get_service(&self, ns: &str, name: &str) -> Option<K8sService> { }
}
```

**Tests**: 10+ unit tests targeting specific cache operations

**Success Criteria**:
- [ ] K8s service watch working
- [ ] Service cache operations (add/update/delete) functional
- [ ] Event dispatch working
- [ ] All unit tests passing
- [ ] No clippy warnings

---

### Track 2: eBPF Maps (Issue #45)
- **Crate**: `crates/ebpf`
- **Duration**: 5-7 days  
- **Owner**: Developer #2 (or parallel with Track 1)
- **Key Responsibilities**:
  - eBPF map declarations (SVC_MAP, BACKEND_MAP, LB_AFFINITY_MAP)
  - Map operations (create, update, delete, query)
  - Serialization/deserialization for map data
  - Connection to BPF programs
  - Unit tests

**Key Files to Implement**:
```rust
// src/maps.rs
pub struct ServiceMap { }
pub struct BackendMap { }
pub struct AffinityMap { }

impl ServiceMap {
  pub fn create() -> Result<Self> { }
  pub fn add_service(&mut self, svc_key: u32, backends: Vec<BackendEntry>) -> Result<()> { }
  pub fn update_service(&mut self, svc_key: u32, backends: Vec<BackendEntry>) -> Result<()> { }
  pub fn get_service(&self, svc_key: u32) -> Result<Option<Vec<BackendEntry>>> { }
  pub fn delete_service(&mut self, svc_key: u32) -> Result<()> { }
}

impl BackendMap {
  pub fn create() -> Result<Self> { }
  pub fn add_backend(&mut self, backend: BackendEntry) -> Result<()> { }
  pub fn get_backend(&self, idx: u32) -> Result<Option<BackendEntry>> { }
  pub fn count_healthy(&self) -> Result<usize> { }
}

impl AffinityMap {
  pub fn create() -> Result<Self> { }
  pub fn set_affinity(&mut self, client_ip: IpAddr, backend: BackendEntry) -> Result<()> { }
  pub fn get_affinity(&self, client_ip: IpAddr) -> Result<Option<BackendEntry>> { }
}
```

**Tests**: 15+ unit tests for map operations

**Success Criteria**:
- [ ] All 3 map types functional
- [ ] Map operations (create/add/update/delete/get) working
- [ ] No resource leaks
- [ ] All unit tests passing
- [ ] No clippy warnings

---

### Track 3: Backend Mapping Engine (Issue #46)
- **Crate**: `crates/backend-mapping`
- **Duration**: 7-10 days
- **Owner**: Developer #1 + Developer #2 (after foundations)
- **Dependencies**: Track 1 (Service Observer) + Track 2 (eBPF Maps)
- **Key Responsibilities**:
  - Service-to-backend discovery
  - Label selector matching
  - Backend pool creation
  - Health check integration
  - Dynamic backend updates

**Implementation Flow**:
1. Watch for service changes (via ServiceObserver from Track 1)
2. When service updated, find all matching pods
3. Convert pods to backend entries
4. Create/update backend pool in eBPF maps (Track 2)
5. Handle health status updates

**Success Criteria**:
- [ ] Backend discovery working
- [ ] Label selector matching correct
- [ ] Backend pools created in eBPF maps
- [ ] Integration with Track 1 & 2 working
- [ ] Healthy/unhealthy backend tracking
- [ ] All tests passing

---

### Track 4: Load Balancing Algorithm (Issue #47)
- **Crate**: `crates/loadbalancer`
- **Duration**: 5-7 days
- **Owner**: Developer #2 (after Track 2)
- **Dependencies**: Track 1 + Track 2
- **Key Responsibilities**:
  - Hash-based consistent hashing
  - Round-robin selection
  - Session affinity (client IP tracking)
  - Health-aware selection
  - Connection tracking

**Implementation Flow**:
1. When packet arrives for a service:
   - Look up service in eBPF SVC_MAP (Track 2)
   - Get list of available backends
   - If client has affinity (AffinityMap), use that backend
   - Otherwise, select based on algorithm (hash/RR)
   - Store affinity if needed
2. Update counters for monitoring

**Success Criteria**:
- [ ] Hash-based selection working
- [ ] Round-robin working
- [ ] Session affinity working
- [ ] Health check integration
- [ ] Connection tracking
- [ ] All tests passing

---

## Development Workflow

### Daily Standup
- Current progress (Track 1/2)
- Blockers or issues
- Planned work for next 4 hours
- Test results from integration runs

### Continuous Testing Strategy
```bash
# Every 2 hours during development:
just run K8sDatapathServicesTest 30m

# Track progress:
# Day 1: Baseline (10-15/50)
# Day 3: Mid-point (20-25/50)
# Day 5: Final-prep (30+/50)
# Day 7: Ready (35+/50)
# Day 9: Validation (40+/50)
```

### Progress Tracking
```
Week 1 - Track 1 & 2 Foundations:
  Mon (Day 1): Setup, initial implementation
  Tue (Day 2): Core components, unit testing
  Wed (Day 3): First integration test run (expect 15-20/50)
  Thu (Day 4): Bug fixes, edge cases
  Fri (Day 5): Polish, code review, documentation

Week 2 - Track 3 & 4 Integration:
  Mon (Day 6): Track 3 starts (depends on 1+2)
  Tue (Day 7): Track 4 starts (depends on 1+2)
  Wed (Day 8): Full integration testing (expect 30+/50)
  Thu (Day 9): Final testing, optimization (expect 40+/50)
  Fri (Day 10): P1 Validation run
```

---

## What "Done" Looks Like

### Each Track
- [ ] All required functions implemented
- [ ] All unit tests passing
- [ ] No clippy warnings
- [ ] Code reviewed
- [ ] Documentation complete
- [ ] Integration tests passing

### Overall P1
- [ ] 40+/50 service specs passing (80%)
- [ ] All 4 tracks integrated
- [ ] No critical bugs
- [ ] Performance acceptable (<1s per LB decision)
- [ ] Ready for v0.1.0 release

---

## Debugging Guide

### Common Issues

**Track 1 (Service Observer) Issues**:
- K8s client connection fails: Check KUBECONFIG, cluster connectivity
- Service watch not firing: Verify label selector, check K8s events
- Cache corruption: Check concurrent access, use Arc<Mutex<>> if needed

**Track 2 (eBPF Maps) Issues**:
- Map creation fails: Check BPF permissions, kernel version
- Data serialization fails: Verify struct padding, alignment
- Performance issues: Check map size, optimize lookups

**Track 3 (Backend Mapping) Issues**:
- Backends not discovered: Check label matching logic
- Stale backends: Verify cleanup on pod deletion
- Health check not working: Check health check interval

**Track 4 (LB Algorithm) Issues**:
- Uneven load distribution: Check hash function
- Session affinity not working: Verify affinity map persistence
- High tail latency: Check connection tracking overhead

---

## Execution Commands

### Build Individual Tracks
```bash
# Track 1: Service Observer
cd crates/service-observer
cargo build --release
cargo test --release

# Track 2: eBPF Maps
cd crates/ebpf
cargo build --release
cargo test --release

# Track 3: Backend Mapping
cd crates/backend-mapping
cargo build --release
cargo test --release

# Track 4: Load Balancer
cd crates/loadbalancer
cargo build --release
cargo test --release
```

### Continuous Development
```bash
# Terminal 1: Track 1
cd crates/service-observer
cargo watch -x "build --release" -x "test --release"

# Terminal 2: Track 2
cd crates/ebpf
cargo watch -x "build --release" -x "test --release"

# Terminal 3: Integration testing (every 2 hours)
while true; do
  just run K8sDatapathServicesTest 30m
  sleep 7200
done
```

### Full Integration Test
```bash
# Run all tests
cargo test --workspace --release

# Run integration tests
just run K8sDatapathServicesTest 30m

# View results
just test-parallel-report
```

---

## Success Metrics

**Code Quality**:
- 100% pass on cargo clippy (strict)
- 100% pass on cargo fmt (check)
- 90%+ test coverage for critical paths

**Performance**:
- Service lookup: <100μs
- Backend selection: <100μs
- Total LB decision: <1ms

**Functionality**:
- K8sDatapathServicesTest: 40+/50 (80%)
- No regressions in K8sAgentFQDNTest (3/3)
- All 4 tracks integrated without conflicts

**Process**:
- Daily standups completed
- Blockers resolved within 2 hours
- No more than 1 critical bug per day
- Code reviewed before merge

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Track 3/4 blocked by Track 1/2 | Minimize dependencies, create interfaces early |
| Integration issues surface late | Run integration tests every 2 hours |
| Performance not meeting targets | Profile early, optimize mid-week |
| Developer conflict (file merge) | Use git worktrees if parallel work needed |
| Test infrastructure failures | Have manual test backup ready |

---

## Next Steps

1. ✅ Clone crates from stubs
2. ✅ Set up development environment
3. **→ START IMPLEMENTATION** (Now)
4. Run tests mid-week to track progress
5. Iterate based on test feedback
6. Final validation Day 9

---

**Status**: READY TO START  
**Target Completion**: May 25, 2026  
**Success Criteria**: 40+/50 service specs passing  
**Release Target**: v0.1.0 (June 2026)
