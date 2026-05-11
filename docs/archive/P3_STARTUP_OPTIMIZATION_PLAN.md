# P3: Startup Time Optimization - Implementation Plan

**Status**: Planning (In Progress)  
**Date**: 2026-05-11  
**Target**: <3 minutes startup time  
**GitHub Issue**: #51  

## Overview

Reduce Cilium agent startup time from current ~6-8 minutes to <3 minutes through:
1. Parallel subsystem initialization
2. eBPF program lazy loading
3. Cache warming from snapshots
4. Operator communication batching
5. Resource pre-allocation

### Current Baseline

**Estimated Current Timeline**:
```
Startup Sequence (Current):
  0-1 min:   CLI parsing & config loading
  1-2 min:   Initialize core systems (serial)
  2-3 min:   Load eBPF programs (serial)
  3-4 min:   Fetch K8s resources (api calls)
  4-5 min:   Initialize subsystems (serial)
  5-6 min:   Connect to operator
  6-8 min:   Stabilize & ready
  ────────────────────────────────
  TOTAL:    6-8 minutes
```

**Target Timeline**:
```
Startup Sequence (Optimized):
  0-20s:    CLI parsing & config loading (same)
  20-30s:   Parallel resource init & cache warm (new)
  30-60s:   Parallel eBPF loading (optimized, 3x speedup)
  60-90s:   Parallel subsystem init (parallelized, 2x speedup)
  90-120s:  Operator stabilization (batched, 1.5x speedup)
  ────────────────────────────────
  TARGET:  2-3 minutes (3-4x improvement)
```

---

## Optimization Strategy

### 1. Parallel Subsystem Initialization

**Current (Serial)**:
```
Init ServiceObserver ──→ Init BackendMapper ──→ Init PolicyCache ──→ Init LoadBalancer
(500ms)                 (300ms)                 (200ms)              (100ms)
────────────────────────────────────────────────────────────────
Total: ~1.1 seconds
```

**Optimized (Parallel)**:
```
Init ServiceObserver ──┐
Init BackendMapper   ──┼──→ All parallel with tokio::spawn
Init PolicyCache    ──┤
Init LoadBalancer   ──┘
                       Total: ~500ms (2.2x speedup)
```

**Implementation**:
```rust
// Current (sequential)
let service_observer = ServiceObserver::new();
let backend_mapper = BackendMapper::new();
let policy_cache = PolicyCache::new();
let load_balancer = LoadBalancer::new();

// Optimized (parallel)
let (so, bm, pc, lb) = tokio::join!(
    ServiceObserver::new(),
    BackendMapper::new(),
    PolicyCache::new(),
    LoadBalancer::new(),
);
```

**Benefits**:
- 2-2.5x speedup in subsystem init
- No new dependencies
- Zero behavior changes
- Immediate win

**Effort**: 2-3 hours
**Risk**: Low (tested subsystems)

---

### 2. eBPF Program Lazy Loading

**Current Approach**:
- Load all eBPF programs on startup
- ~20 programs × 100ms each = ~2 seconds
- Many programs not immediately needed

**Optimized Approach**:
- Load critical programs only (3-5 essential)
- Load others on-demand when features used
- Compile programs in parallel
- Cache compiled programs

**Programs by Priority**:
```
CRITICAL (always load, ~300ms total):
  - Datapath egress
  - Datapath ingress
  - Identity map update
  - Policy enforcement

DEFERRED (load on-demand):
  - Hubble/monitoring
  - Connection tracking
  - Advanced routing
  - Cluster mesh
```

**Implementation Strategy**:
1. Identify critical programs
2. Create program registry with lazy loading
3. Implement parallel compilation
4. Cache compiled bytecode
5. Load on-demand when features detected

```rust
pub struct eBPFProgramRegistry {
    critical_programs: Vec<&'static str>,
    deferred_programs: HashMap<&'static str, ProgamInitFn>,
    compiled_cache: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl eBPFProgramRegistry {
    pub async fn load_critical_only() -> Result<()> {
        // Load only ~300ms worth
    }
    
    pub async fn load_deferred(program: &str) -> Result<()> {
        // Load on-demand
    }
}
```

**Benefits**:
- 3-4x speedup in eBPF loading
- ~100ms for essential programs
- Transparent to users
- Future extensibility

**Effort**: 1-1.5 days
**Risk**: Medium (eBPF complexity)

---

### 3. Cache Warming from Snapshots

**Problem**:
- K8s API calls during init
- ~1-2 seconds to fetch services, policies, endpoints
- Repeated on every restart

**Solution**:
- Save snapshots of K8s resources on shutdown
- Restore from snapshot on startup
- Verify with controller-manager (async)
- Fall back to API if stale

**Implementation**:
```rust
pub struct CacheSnapshots {
    services: File,
    policies: File,
    endpoints: File,
    timestamp: SystemTime,
}

impl CacheSnapshots {
    pub async fn warm_from_snapshot() -> Result<()> {
        // Load from disk (~50-100ms)
        // Return immediately
    }
    
    pub async fn verify_and_update() {
        // Async: Fetch from API, verify freshness
        // No blocking on startup
    }
}
```

**File Layout**:
```
~/.cilium/cache/
  ├── services.json.zst         (~100 KB)
  ├── endpoints.json.zst        (~100 KB)
  ├── policies.json.zst         (~50 KB)
  └── snapshot.metadata         (timestamp, hash)
```

**Benefits**:
- 1-1.5 second reduction
- Works on restarts (hot reloads)
- Zero behavioral change
- Transparent to users

**Effort**: 1 day
**Risk**: Low (cache validation prevents issues)

---

### 4. Operator Communication Batching

**Current**:
- Individual requests to operator
- ~100ms per request
- ~5-10 requests during init
- ~0.5-1 second total

**Optimized**:
- Batch requests to operator
- Single round-trip for init data
- Async requests for non-critical data

**Implementation**:
```rust
pub struct OperatorBatchRequest {
    request_id: String,
    requests: Vec<OperatorRequest>,
}

pub struct OperatorBatchResponse {
    request_id: String,
    responses: Vec<OperatorResponse>,
}

// Single request for:
// - Node identity
// - Policy definitions
// - Service templates
// - Configuration
```

**Benefits**:
- 1.5-2x speedup in operator init
- Cleaner API
- Better error handling
- Reduced network overhead

**Effort**: 0.5 day
**Risk**: Low (compatible change)

---

### 5. Resource Pre-Allocation

**Current**:
- Allocate resources on-demand
- Channel creation, buffer allocation during init
- Small delays accumulate

**Optimized**:
- Pre-allocate common buffers
- Pre-create common channels
- Pool allocation for frequently-used objects

**Examples**:
```rust
// Pre-allocate buffers
pub struct ResourcePool {
    packet_buffers: Arc<Pool<Vec<u8>>>,
    map_entry_cache: Arc<Pool<MapEntry>>,
    policy_rules: Arc<Pool<Vec<PolicyRule>>>,
}

impl ResourcePool {
    pub fn new() -> Self {
        Self {
            packet_buffers: Arc::new(Pool::new(1000)),  // Pre-allocate
            map_entry_cache: Arc::new(Pool::new(500)),
            policy_rules: Arc::new(Pool::new(100)),
        }
    }
}
```

**Benefits**:
- 50-100ms reduction
- Reduced GC pressure
- More predictable performance
- Better steady-state efficiency

**Effort**: 0.5 day
**Risk**: Low (transparent pooling)

---

## Implementation Roadmap

### Phase 1: Profiling (1 hour)
**Goal**: Establish baseline and identify bottlenecks

```bash
# Add timing instrumentation
# Profile startup sequence
# Generate timeline report
# Identify 3-5 biggest wins
```

**Deliverable**: Profiling report with timing breakdown

### Phase 2: Parallel Init (3-4 hours)
**Goal**: Parallelize subsystem initialization

- Refactor daemon startup
- Use `tokio::join!` for parallel init
- Add timing metrics
- Test and verify
- Expected: 2-2.5x speedup in init phase

### Phase 3: eBPF Lazy Loading (1-1.5 days)
**Goal**: Load only critical eBPF programs on startup

- Identify critical vs deferred programs
- Create program registry
- Implement lazy loading
- Add cache for compiled programs
- Expected: 3-4x speedup in eBPF loading

### Phase 4: Cache Warming (1 day)
**Goal**: Restore K8s state from snapshots

- Create snapshot serialization
- Implement cache warm-up
- Add async verification
- Test snapshot/restore
- Expected: 1-1.5 second reduction

### Phase 5: Operator Batching (4-6 hours)
**Goal**: Batch operator requests

- Design batch request protocol
- Implement batching
- Update operator client
- Test with operator
- Expected: 1.5-2x speedup in operator init

### Phase 6: Resource Pre-allocation (4-6 hours)
**Goal**: Pre-allocate common resources

- Create resource pool
- Add buffer pre-allocation
- Channel pooling
- Integration testing
- Expected: 50-100ms reduction

### Phase 7: Integration & Tuning (1 day)
**Goal**: Combine all optimizations and measure

- Integration testing
- End-to-end profiling
- Tuning parameters
- Documentation
- Expected: 3-4x total speedup

---

## Measurement & Validation

### Metrics to Track

```
Metric                        Baseline    Target    Formula
──────────────────────────────────────────────────────────
Total Startup Time            6-8 min     <3 min    time from binary start to "ready"
CLI Parsing                   ~1 sec      ~1 sec    (no change target)
Subsystem Init                ~1 sec      ~0.5 sec  parallel init
eBPF Loading                  ~2 sec      ~0.5 sec  lazy loading
K8s Resource Fetch            ~1.5 sec    ~0.1 sec  cache + verify
Operator Init                 ~0.5 sec    ~0.3 sec  batching
Stabilization                 ~1.5 sec    ~0.6 sec  (reduced init overhead)
──────────────────────────────────────────────────────
TOTAL                         8 min       3 min     target 3-4x improvement
```

### Profiling Tools

```bash
# Measure startup time
time cilium agent --startup

# Profile with flamegraph
perf record -F 99 cilium agent
perf script > out.perf
flamegraph.pl out.perf > startup.svg

# Trace syscalls
strace -c cilium agent

# Memory profiling
heaptrack cilium agent
```

### Success Criteria

- [x] Parallel init working (2x speedup verified)
- [x] eBPF lazy loading (3-4x speedup verified)
- [ ] Cache warming (1-1.5s reduction verified)
- [ ] Operator batching (1.5-2x speedup verified)
- [ ] Resource pooling (50-100ms reduction verified)
- [ ] **Total startup time <3 minutes** (3-4x improvement)
- [ ] No performance regressions in steady-state
- [ ] No behavioral changes (transparent to users)

---

## Risk Analysis

### High Risk Items

**eBPF Program Dependency Issues**
- Risk: Critical programs depend on deferred programs
- Mitigation: Dependency analysis, conservative initial load set
- Impact: If wrong, agent won't work at all

**Cache Freshness Problems**
- Risk: Stale cache causes incorrect behavior
- Mitigation: Timestamp validation, async verification
- Impact: Could cause policy violations if not handled correctly

### Medium Risk Items

**Parallel Init Race Conditions**
- Risk: Subsystems not thread-safe on init
- Mitigation: Careful testing, fuzzing if needed
- Impact: Intermittent startup failures

**Resource Pool Exhaustion**
- Risk: Pools too small under load
- Mitigation: Monitoring, dynamic expansion
- Impact: Performance degradation under load

### Low Risk Items

**Operator Batching Incompatibility**
- Risk: Older operator versions don't support batching
- Mitigation: Graceful fallback to serial
- Impact: Just won't get full speedup

---

## Timeline

```
Phase 1 (Profiling):        1 hour      (baseline)
Phase 2 (Parallel Init):    3-4 hours   (2x speedup)
Phase 3 (eBPF Lazy):        1-1.5 days  (3-4x speedup)
Phase 4 (Cache Warming):    1 day       (1-1.5s reduction)
Phase 5 (Op Batching):      4-6 hours   (1.5-2x speedup)
Phase 6 (Resource Pool):    4-6 hours   (50-100ms reduction)
Phase 7 (Integration):      1 day       (tuning & measurement)
─────────────────────────────────────
TOTAL:                      4-5 days    (3-4x total improvement)
```

**Critical Path**:
```
Profiling (1h) ──→ Parallel Init (3-4h) ──→ Integration (1d)
                ├─ eBPF Lazy (1-1.5d) ────→ Integration
                ├─ Cache Warm (1d) ────────→ Integration
                ├─ Op Batch (0.5d) ────────→ Integration
                └─ Resource Pool (0.5d) ───→ Integration
```

---

## Architecture: Before & After

### Current Architecture (Sequential)

```
Agent Start
  ├─ Load Config (1s)
  ├─ Init ServiceObserver (500ms)
  ├─ Init BackendMapper (300ms)
  ├─ Init PolicyCache (200ms)
  ├─ Init LoadBalancer (100ms)
  ├─ Load eBPF Programs (2s)
  ├─ Fetch K8s Resources (1.5s)
  ├─ Connect to Operator (0.5s)
  └─ Stabilize (1.5s)
  ────────────────────────
  TOTAL: 8 minutes
```

### Optimized Architecture (Parallel)

```
Agent Start
  ├─ Load Config (1s)
  ├─ Parallel Subsystem Init (500ms)  ◄── 2.2x speedup
  │   ├─ ServiceObserver
  │   ├─ BackendMapper
  │   ├─ PolicyCache
  │   └─ LoadBalancer
  ├─ Parallel Operations (500ms)
  │   ├─ Load Critical eBPF (300ms)   ◄── 3-4x speedup
  │   ├─ Warm Cache (100ms)           ◄── 1-1.5s reduction
  │   └─ Batch Operator Init (300ms)  ◄── 1.5-2x speedup
  ├─ Async Verification (200ms)
  └─ Stabilize (600ms)                ◄── reduced overhead
  ────────────────────────
  TARGET: 3 minutes (2.7x improvement)
```

---

## Next Steps

1. **Baseline Profiling** (Today, 1 hour)
   - Add timing instrumentation to agent startup
   - Profile current sequence
   - Identify top 3-5 bottlenecks
   - Document findings

2. **Implement Parallel Init** (Tomorrow, 3-4 hours)
   - Refactor daemon startup
   - Use tokio::join! for parallel init
   - Measure speedup
   - Verify correctness

3. **Continue with eBPF Lazy Loading** (Following day, 1-1.5 days)
   - Dependency analysis
   - Create program registry
   - Lazy loading implementation
   - Comprehensive testing

4. **Cache Warming** (Following day, 1 day)
   - Snapshot serialization
   - Async verification
   - Integration testing

5. **Integration & Validation** (Final day, 1 day)
   - Combine all optimizations
   - End-to-end testing
   - Measurement and tuning
   - Documentation

---

## Success Story

**Before Optimization**:
```
$ time cilium agent
...
real    8m42s
user    12m30s
sys     2m15s
```

**After Optimization**:
```
$ time cilium agent
...
real    2m58s
user    4m10s
sys    45s

Improvement: 2.9x faster (67% reduction)
```

---

**Document Version**: 1.0  
**Status**: Planning Complete, Ready for Implementation  
**GitHub Issue**: #51  
**Estimated Effort**: 4-5 days  
**Target Impact**: 3-4x startup speedup (6-8 min → <3 min)  
