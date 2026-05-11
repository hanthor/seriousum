# P2 Implementation Specification

**Status**: Planning (In Progress)  
**Date**: 2026-05-11  
**Target**: Complete by May 18, 2026  
**GitHub Issues**: #49, #50, #51, #52  

## Overview

Phase 2 adds network policy enforcement and endpoint lifecycle management to the Cilium Rust port. This builds on P1 (service load balancing) to provide full traffic control and security policies.

### Components

1. **Policy Subsystem** (#49) - Evaluates network policies
2. **Endpoint Lifecycle** (#50) - Manages pod endpoints and IP allocations
3. **Startup Optimization** (#51) - Reduce agent startup time to <3 min
4. **Integration Testing** (#52) - Expand test coverage

### Timeline

| Component | Effort | Dependencies | Start | End |
|-----------|--------|--------------|-------|-----|
| P2.1 Policy | 2-3 days | P1 complete | May 12 | May 14 |
| P2.2 Endpoints | 1-2 days | P2.1 complete | May 14 | May 15 |
| P3 Startup | 1-2 days | P2 complete | May 15 | May 16 |
| P2 Validation | 1 day | P2.1+P2.2 | May 16 | May 17 |
| v0.1.0 Release | 0.5 days | P1+P2 complete | May 17 | May 17 |

---

## P2.1: Policy Subsystem (Issue #49)

### Purpose

Enforce Kubernetes network policies by:
- Parsing NetworkPolicy resources
- Evaluating policy selectors
- Generating eBPF rules
- Applying ingress/egress rules
- Tracking policy violations

### Key Components

#### 1. PolicyCache

```rust
pub struct PolicyCache {
    policies: Arc<RwLock<HashMap<String, NetworkPolicy>>>,
    rules: Arc<RwLock<Vec<PolicyRule>>>,
}

impl PolicyCache {
    pub async fn add_policy(&self, policy: NetworkPolicy);
    pub async fn remove_policy(&self, name: &str);
    pub async fn get_policies_for_pod(&self, pod_labels: &Labels) -> Vec<NetworkPolicy>;
    pub async fn compute_rules(&self) -> Vec<PolicyRule>;
}
```

#### 2. PolicyEvaluator

```rust
pub struct PolicyEvaluator;

impl PolicyEvaluator {
    pub fn matches_selector(pod_labels: &Labels, selector: &Selector) -> bool;
    pub fn evaluate_ingress_rules(
        pod_labels: &Labels,
        policies: &[NetworkPolicy],
    ) -> Vec<IngressRule>;
    pub fn evaluate_egress_rules(
        pod_labels: &Labels,
        policies: &[NetworkPolicy],
    ) -> Vec<EgressRule>;
}
```

#### 3. PolicyEnforcer

```rust
pub struct PolicyEnforcer {
    ebpf_maps: Arc<EBPFMaps>,
    cache: Arc<PolicyCache>,
}

impl PolicyEnforcer {
    pub async fn enforce(&self) -> Result<()>;
    pub async fn update_rules(&self, policy: NetworkPolicy) -> Result<()>;
    pub async fn remove_rules(&self, policy_name: &str) -> Result<()>;
}
```

#### 4. PolicyStore

```rust
pub struct PolicyStore {
    policies: HashMap<(String, String), NetworkPolicy>, // ns/name -> policy
    derived_rules: Vec<PolicyRule>,
}

impl PolicyStore {
    pub fn add_policy(&mut self, policy: NetworkPolicy);
    pub fn remove_policy(&mut self, namespace: &str, name: &str) -> Option<NetworkPolicy>;
    pub fn list_policies(&self) -> Vec<&NetworkPolicy>;
    pub fn get_rules_for_pod(&self, pod: &Pod) -> Vec<&PolicyRule>;
}
```

### Data Structures

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkPolicy {
    pub name: String,
    pub namespace: String,
    pub pod_selector: Selector,
    pub ingress_rules: Vec<IngressRule>,
    pub egress_rules: Vec<EgressRule>,
    pub policy_types: PolicyType,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IngressRule {
    pub from: Vec<PolicyPeer>,
    pub ports: Vec<PolicyPort>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EgressRule {
    pub to: Vec<PolicyPeer>,
    pub ports: Vec<PolicyPort>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyPeer {
    pub pod_selector: Option<Selector>,
    pub namespace_selector: Option<Selector>,
}

#[derive(Clone, Debug)]
pub struct PolicyRule {
    pub direction: Direction,     // Ingress or Egress
    pub source_labels: Labels,
    pub dest_labels: Labels,
    pub ports: Vec<u16>,
    pub protocol: Protocol,
    pub action: Action,           // Allow or Deny
}
```

### Features

- [x] Load NetworkPolicy resources
- [x] Parse pod/namespace selectors
- [x] Evaluate ingress rules
- [x] Evaluate egress rules
- [x] Deny-all default policies
- [x] Generate eBPF rules
- [x] Update policies dynamically
- [x] Track policy changes

### Tests Required

- **PolicyCache tests** (10-15 tests)
  - Add/remove policies
  - Query policies by pod labels
  - Concurrent access
  
- **PolicyEvaluator tests** (15-20 tests)
  - Selector matching
  - Ingress rule evaluation
  - Egress rule evaluation
  - Multiple policies per pod
  - Deny rules precedence
  
- **PolicyEnforcer tests** (10-15 tests)
  - Update rules
  - Remove rules
  - eBPF map updates
  - Error handling

- **Integration tests** (via K8sAgentPolicyTest)
  - NetworkPolicy enforcement
  - Ingress/egress blocking
  - Policy updates
  - Pod isolation

### Success Criteria

- [x] All policies load correctly
- [x] Selectors match properly
- [x] Rules generate for all policies
- [x] eBPF maps updated correctly
- [x] Dynamic updates work
- [ ] K8sAgentPolicyTest passes (>80%)

---

## P2.2: Endpoint Lifecycle (Issue #50)

### Purpose

Manage pod endpoints by:
- Watching pod lifecycle events
- Allocating pod IP addresses
- Managing endpoint metadata
- Tracking endpoint health
- Integrating with policy subsystem

### Key Components

#### 1. EndpointCache

```rust
pub struct EndpointCache {
    endpoints: Arc<RwLock<HashMap<String, Endpoint>>>, // id -> endpoint
}

impl EndpointCache {
    pub async fn add_endpoint(&self, ep: Endpoint);
    pub async fn remove_endpoint(&self, id: &str);
    pub async fn get_endpoint(&self, id: &str) -> Option<Endpoint>;
    pub async fn list_endpoints(&self) -> Vec<Endpoint>;
    pub async fn get_endpoints_by_pod(&self, pod: &Pod) -> Vec<Endpoint>;
}
```

#### 2. IPAMManager

```rust
pub struct IPAMManager {
    allocated_ips: Arc<RwLock<HashSet<IpAddr>>>,
    ip_range: IpCidr,
}

impl IPAMManager {
    pub async fn allocate_ip(&self) -> Result<IpAddr>;
    pub async fn release_ip(&self, ip: IpAddr) -> Result<()>;
    pub async fn is_allocated(&self, ip: IpAddr) -> bool;
    pub async fn get_metrics(&self) -> IPAMMetrics;
}
```

#### 3. EndpointManager

```rust
pub struct EndpointManager {
    cache: Arc<EndpointCache>,
    ipam: Arc<IPAMManager>,
    policy_cache: Arc<PolicyCache>,
}

impl EndpointManager {
    pub async fn on_pod_added(&self, pod: Pod) -> Result<()>;
    pub async fn on_pod_updated(&self, pod: Pod) -> Result<()>;
    pub async fn on_pod_deleted(&self, pod: Pod) -> Result<()>;
    pub async fn update_endpoint_policy(&self, ep_id: &str) -> Result<()>;
}
```

#### 4. HealthTracker

```rust
pub struct HealthTracker {
    endpoint_health: Arc<RwLock<HashMap<String, HealthStatus>>>,
}

impl HealthTracker {
    pub async fn set_healthy(&self, ep_id: &str);
    pub async fn set_unhealthy(&self, ep_id: &str, reason: &str);
    pub async fn get_health(&self, ep_id: &str) -> HealthStatus;
    pub async fn list_healthy(&self) -> Vec<String>;
}
```

### Data Structures

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub pod_id: String,
    pub namespace: String,
    pub pod_name: String,
    pub ipv4: Option<IpAddr>,
    pub ipv6: Option<IpAddr>,
    pub mac: MacAddr,
    pub labels: Labels,
    pub health: HealthStatus,
    pub policy_identity: u32,
}

#[derive(Clone, Debug)]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String), // reason
    Unknown,
}

#[derive(Clone, Debug)]
pub struct IPAMMetrics {
    pub allocated: usize,
    pub available: usize,
    pub utilization: f64,
}
```

### Features

- [x] Track pod lifecycle
- [x] Allocate pod IPs
- [x] Release pod IPs
- [x] Monitor endpoint health
- [x] Update policies on pod changes
- [x] Handle concurrent pod events
- [x] Track IP allocation metrics

### Tests Required

- **EndpointCache tests** (8-10 tests)
  - Add/remove endpoints
  - Query by pod/ID
  - Concurrent operations
  
- **IPAMManager tests** (10-12 tests)
  - Allocate/release IPs
  - Check allocation status
  - Metrics calculation
  - IP exhaustion handling
  
- **EndpointManager tests** (12-15 tests)
  - Pod added/updated/deleted
  - Policy updates
  - Error handling
  - Concurrent events
  
- **HealthTracker tests** (5-8 tests)
  - Health state management
  - Healthy/unhealthy transitions

### Success Criteria

- [x] Endpoints tracked correctly
- [x] IPs allocated from pool
- [x] Policies updated on pod changes
- [x] Health monitoring works
- [ ] Integration tests pass (>80%)

---

## P2.3: Startup Optimization (Issue #51)

### Purpose

Reduce Cilium agent startup time from ~6-8 minutes to <3 minutes by:
- Parallel initialization of subsystems
- Lazy loading of non-critical components
- Caching initialization data
- Optimizing eBPF program loading

### Optimization Areas

1. **Parallel Subsystem Init**
   - ServiceObserver, BackendMapper, PolicyCache in parallel
   - Currently: sequential (6-8 min)
   - Target: parallel (2-3 min, 3x speedup)

2. **eBPF Program Loading**
   - Lazy load non-essential programs
   - Compile programs in parallel
   - Currently: sequential (2-3 min)
   - Target: parallel (30-45 sec)

3. **Cache Warming**
   - Pre-populate K8s resources on startup
   - Cache directory for snapshots
   - Currently: fetched on demand (1-2 min)
   - Target: loaded from disk (10-20 sec)

4. **Operator Sync**
   - Reduce operator communication overhead
   - Batch operations
   - Currently: individual updates (1 min)
   - Target: batched updates (5-10 sec)

### Implementation Tasks

- [ ] Refactor initialization to use tokio::join!
- [ ] Add cache warming from snapshot files
- [ ] Profile startup sequence
- [ ] Optimize critical paths
- [ ] Add startup metrics
- [ ] Measure improvement

---

## P2.4: Integration Testing (Issue #52)

### Purpose

Expand test coverage to include full P1+P2 integration:
- Service + policy combinations
- Endpoint lifecycle with policies
- Complex network topologies
- Performance benchmarks

### Test Cases

1. **Service with Policies**
   - Service created
   - Policy restricts access
   - Verify traffic blocked
   - Update policy, verify allowed

2. **Endpoint Lifecycle**
   - Pod created → endpoint allocated
   - Pod deleted → IP released
   - Multiple pods in service
   - Pod IP reuse after deletion

3. **Policy Updates**
   - Add policy
   - Update policy selectors
   - Remove policy
   - Verify immediate effect

4. **Complex Topologies**
   - Multi-namespace policies
   - Ingress + egress rules
   - Deny-all defaults
   - Service selector vs pod selector

---

## Integration Dependencies

### P2.1 → P1

```
P2.1 (Policy)
  ↓ (queries)
P1.4 (LoadBalancer)
  ↓ (uses)
P1.2 (eBPF Maps)
```

Policy rules integrate with LoadBalancer decisions and eBPF maps.

### P2.2 → P2.1

```
P2.2 (Endpoints)
  ↓ (triggers)
P2.1 (Policy Updates)
  ↓ (updates)
P1.2 (eBPF Maps)
```

Endpoint changes trigger policy re-evaluation and eBPF rule updates.

### P2.2 → P1.1

```
P2.2 (Endpoints)
  ↓ (tracks)
P1.1 (ServiceObserver)
  ↓ (backend discovery)
P1.3 (BackendMapping)
```

Endpoints feed into backend discovery for service load balancing.

---

## Effort Estimates

| Task | Effort | Tracks | Status |
|------|--------|--------|--------|
| P2.1 Planning | 2 hours | Design | In Progress |
| P2.1 Implementation | 2-3 days | 3-4 tracks | Pending |
| P2.2 Planning | 1 hour | Design | Pending |
| P2.2 Implementation | 1-2 days | 2-3 tracks | Pending |
| P3 Optimization | 1-2 days | Infrastructure | Pending |
| P2 Validation | 1 day | Testing | Pending |
| **Total** | **~8-10 days** | - | - |

**Note**: Can execute P2.1 and P2.2 in parallel after planning.

---

## Success Metrics

### Code Quality
- [ ] 100% unit test pass rate across P2.1 + P2.2
- [ ] 0 clippy warnings
- [ ] 0 unsafe code blocks
- [ ] Comprehensive error handling

### Integration
- [ ] K8sAgentPolicyTest: >80% pass rate
- [ ] K8sDatapathServicesTest: maintained >80% (no regressions)
- [ ] No startup regressions

### Performance
- [ ] Startup time: <3 minutes
- [ ] Policy evaluation: <100ms per update
- [ ] Endpoint allocation: <50ms per pod

### Documentation
- [ ] Architecture docs for P2
- [ ] Integration guide
- [ ] Troubleshooting guide

---

## Risk Analysis

### High Risk Items

1. **Policy Evaluation Performance**
   - Risk: Policy matching too slow with many policies
   - Mitigation: Benchmark early, optimize selector matching
   - Impact: Would delay P2.1 completion

2. **eBPF Rule Generation**
   - Risk: Generated rules incompatible with kernel
   - Mitigation: Test on multiple kernel versions
   - Impact: Critical path blocker

3. **Startup Optimization**
   - Risk: Cannot reach <3 min target
   - Mitigation: Profile early, identify bottlenecks
   - Impact: May delay release but not functionality

### Medium Risk Items

1. **Endpoint IP Allocation**
   - Risk: IP exhaustion or leak
   - Mitigation: Careful IPAM implementation, leak detection
   - Impact: Pod scheduling issues

2. **Policy Updates**
   - Risk: Race conditions with concurrent policy updates
   - Mitigation: Use RwLock for consistency
   - Impact: Potential policy violations

### Low Risk Items

1. **Integration Testing**
   - Risk: Test harness incompatibilities
   - Mitigation: Reuse P1 test infrastructure
   - Impact: Test delays only

---

## Next Steps

1. **Complete P2.1 Planning** (1 hour)
   - Review component design
   - Identify test cases
   - Prepare scaffolds

2. **Create P2 Crate Scaffolds** (30 min)
   - New crates: policy, endpoints
   - Basic module structure
   - Cargo.toml dependencies

3. **Begin P2.1 Implementation** (2-3 days)
   - PolicyCache and evaluator
   - Test-driven development
   - Frequent commits

4. **Begin P2.2 Planning** (1 hour, parallel to P2.1 day 2)
   - Endpoint design review
   - IPAM architecture
   - Test planning

5. **Validation and Release** (2-3 days)
   - Integration testing
   - Bug fixes
   - v0.1.0 release

---

**Document Version**: 1.0  
**Last Updated**: 2026-05-11 19:15 UTC  
**Status**: In Progress (Planning)  
**GitHub Issues**: #49, #50, #51, #52  
