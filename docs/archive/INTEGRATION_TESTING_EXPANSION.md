# Integration Testing Expansion - P2 Test Strategy

**Status**: Planning (In Progress)  
**Date**: 2026-05-11  
**Scope**: Comprehensive P1+P2 integration testing  
**GitHub Issue**: #52  

## Overview

Expand integration test coverage to validate:
1. Service load balancing + network policies
2. Pod endpoint lifecycle + service backends
3. Complex networking topologies
4. Performance benchmarks
5. Edge cases and error scenarios

### Current Test Coverage

**P0 (Complete)**:
- ✅ K8sFQDNTest (FQDN resolution)
- ✅ K8sNetworkPoliciesTest (basic policies)
- ✅ K8sAgentPolicyTest (policy enforcement)

**P1 (In Validation)**:
- ⏳ K8sDatapathServicesTest (service load balancing)

**P2 (Not Yet Tested)**:
- [ ] K8sPolicyServicesTest (policies + services)
- [ ] K8sEndpointLifecycleTest (pod lifecycle)
- [ ] K8sComplexTopologyTest (multi-namespace, complex rules)
- [ ] K8sPerformanceTest (benchmarks)
- [ ] K8sEdgeCasesTest (error scenarios)

---

## Test Categories

### 1. P1+P2 Integration Tests

**Test: ServiceWithPolicy**
```
Setup:
  - Create Service (nginx)
  - Create Policy (deny by default, allow from web)
  - Create 2 pods: web frontend, backend
  
Verify:
  - Frontend can reach backend through service
  - Other pods cannot reach backend
  - Service performs load balancing
  - Policy blocks unauthorized access
  
Expected Result: ✅ PASS
```

**Test: EndpointLifecycleTracking**
```
Setup:
  - Create Service with 0 pods
  - Add pod incrementally (1, 2, 3 pods)
  - Delete pods one by one
  
Verify:
  - Service discovers new endpoints immediately
  - Load balancer updates backend list
  - No connection drops during changes
  - IPs properly allocated and released
  
Expected Result: ✅ PASS
```

**Test: PolicyUpdateWithRunningTraffic**
```
Setup:
  - Create Service + Pods
  - Start traffic (ncgen)
  - Update policy (add new allow rule)
  
Verify:
  - Traffic continues uninterrupted
  - New traffic pattern takes effect
  - No lost packets during update
  - State consistent throughout
  
Expected Result: ✅ PASS
```

### 2. Complex Topology Tests

**Test: MultiNamespacePolicy**
```
Setup:
  - Namespace A: frontend pods
  - Namespace B: backend pods
  - Namespace C: database pods
  - Policies: A→B, B→C, C not A
  
Verify:
  - A can reach B
  - B can reach C
  - C cannot reach A
  - Cross-namespace works correctly
  
Expected Result: ✅ PASS
```

**Test: PolicyHierarchy**
```
Setup:
  - Namespace-level policy (deny all)
  - Pod-level policy (allow specific)
  - Ingress + Egress combined
  
Verify:
  - Namespace policy acts as baseline
  - Pod policy refines rules
  - Ingress + Egress combined correctly
  - Priority/precedence correct
  
Expected Result: ✅ PASS
```

**Test: LargeScaleServices**
```
Setup:
  - Create 10 services
  - Create 100 pods
  - Each service with 10 pods
  
Verify:
  - All services operational
  - Load balancing works at scale
  - Policies enforced correctly
  - No memory leaks or hangs
  
Expected Result: ✅ PASS
```

### 3. Performance Tests

**Test: LoadBalancerThroughput**
```
Measure:
  - Throughput with 1, 10, 100 connections
  - Latency p50, p95, p99
  - CPU/memory usage
  
Baseline:
  - Current implementation (Go version)
  
Target:
  - >80% of Go version throughput
  - <5% latency increase
  - Comparable memory usage
  
Expected Result: ✅ PASS
```

**Test: PolicyEvaluationPerformance**
```
Measure:
  - Policy lookup time (1, 10, 100 policies)
  - Per-packet evaluation latency
  - Rule update latency
  
Baseline:
  - <100µs per lookup
  - <10µs per evaluation
  - <100ms per update
  
Expected Result: ✅ PASS
```

**Test: EndpointScaling**
```
Measure:
  - Allocation time for 1, 10, 100, 1000 pods
  - Memory per endpoint
  - Query latency with many endpoints
  
Baseline:
  - <50ms per allocation
  - <1KB per endpoint
  - <10ms per query
  
Expected Result: ✅ PASS
```

### 4. Edge Cases & Error Scenarios

**Test: ServiceWithNoPods**
```
Setup:
  - Create Service
  - No pods initially
  
Verify:
  - Service created successfully
  - Traffic times out appropriately
  - No crashes or hangs
  - Pods added later work
  
Expected Result: ✅ PASS
```

**Test: PolicyWithInvalidSelectors**
```
Setup:
  - Create policy with invalid selectors
  
Verify:
  - Policy rejected with clear error
  - No crash
  - Other policies unaffected
  
Expected Result: ✅ PASS
```

**Test: ConcurrentPodCreation**
```
Setup:
  - Create 100 pods concurrently
  
Verify:
  - All allocated unique IPs
  - No conflicts or duplicates
  - All tracked correctly
  - No resource leaks
  
Expected Result: ✅ PASS
```

**Test: PoliciesWithCircularDependencies**
```
Setup:
  - Create circular policy references
  
Verify:
  - Detected and rejected
  - Clear error message
  - No infinite loops
  
Expected Result: ✅ PASS
```

---

## Test Implementation Framework

### Test Structure

```rust
#[tokio::test]
async fn test_service_with_policy() {
    // Setup phase
    let cluster = create_kind_cluster("test-service-policy").await;
    let kubectl = cluster.kubectl();
    
    // Create resources
    kubectl.apply(service_yaml).await;
    kubectl.apply(policy_yaml).await;
    kubectl.apply(pods_yaml).await;
    
    // Wait for readiness
    wait_for_pods_running(&kubectl, "default", 5 * MINUTE).await;
    
    // Test phase
    let result = verify_traffic_flow(&kubectl, &cases);
    assert!(result.success, "Traffic verification failed: {:?}", result);
    
    // Cleanup
    cluster.cleanup().await;
}
```

### Test Utilities

```rust
pub struct IntegrationTestKit {
    cluster: KindCluster,
    kubectl: KubectlClient,
    metrics: MetricsCollector,
}

impl IntegrationTestKit {
    pub async fn verify_service_reachability(
        &self,
        source_pod: &str,
        service: &str,
        should_reach: bool,
    ) -> Result<VerificationResult>;
    
    pub async fn verify_policy_enforcement(
        &self,
        source: &str,
        dest: &str,
        port: u16,
        should_allow: bool,
    ) -> Result<bool>;
    
    pub async fn measure_latency(
        &self,
        source_pod: &str,
        dest_service: &str,
        samples: usize,
    ) -> Result<LatencyStats>;
    
    pub async fn collect_metrics(&self) -> Result<TestMetrics>;
}
```

### Test Data

Create test fixtures in `tests/fixtures/`:
```
tests/fixtures/
  ├── services/
  │   ├── single-service.yaml
  │   ├── multi-service.yaml
  │   └── service-with-endpoints.yaml
  ├── policies/
  │   ├── deny-all.yaml
  │   ├── allow-ingress.yaml
  │   └── complex-policy.yaml
  ├── pods/
  │   ├── frontend-pod.yaml
  │   ├── backend-pod.yaml
  │   └── database-pod.yaml
  └── topologies/
      ├── multi-namespace.yaml
      └── complex-setup.yaml
```

---

## Test Execution Strategy

### Sequential Execution (CI Pipeline)

```bash
# Smoke tests (fast, 5 min)
cargo test --test integration_smoke

# P1 validation (45 min)
cargo test --test p1_validation

# P2 validation (if P1 passes, 45 min)
cargo test --test p2_validation

# Performance (extended, 1+ hour)
cargo test --test performance --release
```

### Parallel Execution (Development)

```bash
# Run 3 test suites simultaneously on separate clusters
./scripts/run-parallel-tests.sh \
  "P1 Validation" \
  "P2 Integration" \
  "Performance Tests"

# Expected: 45-60 min instead of 2+ hours
```

---

## Test Matrix

### P2 Integration Tests

| Test Name | Depends On | Duration | Risk | Pass Target |
|-----------|-----------|----------|------|------------|
| ServiceWithPolicy | P1+P2 | 5 min | Low | 95%+ |
| EndpointLifecycle | P2 | 5 min | Low | 95%+ |
| PolicyUpdate | P2 | 5 min | Med | 90%+ |
| MultiNamespace | P2 | 10 min | High | 85%+ |
| LargeScale | P2 | 20 min | Med | 80%+ |
| Performance | P1+P2 | 30 min | Med | 80% baseline |
| EdgeCases | P2 | 15 min | High | 90%+ |

**Total Duration**: ~90 min sequential, ~45 min parallel

---

## Failure Handling

### Automatic Retries

```yaml
test_retry_config:
  max_retries: 3
  backoff_strategy: exponential
  skip_conditions:
    - infrastructure_error
    - timeout (first attempt only)
```

### Log Collection

```
On failure, collect:
  - Agent logs (stderr, structured logs)
  - Kernel logs (dmesg)
  - Pod logs (kubectl logs)
  - eBPF verifier output
  - Metrics snapshot
  - Full test environment dump
```

### Failure Classification

```
INFRASTRUCTURE_ERROR:
  - Kind cluster crashed
  - Network issues
  - Timeout
  → Automatic retry

CODE_ERROR:
  - Assertion failure
  - Exception
  - Unexpected behavior
  → No retry, escalate

FLAKY:
  - Passes on retry
  → Mark as flaky, separate tracking
```

---

## CI/CD Integration

### GitHub Actions Workflow

```yaml
name: Integration Tests

on: [push, pull_request]

jobs:
  p1_validation:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - run: cargo build --release
      - run: ./scripts/run-cilium-kind-test.sh --focus "K8sDatapathServicesTest" --timeout 45m
      
  p2_integration:
    runs-on: ubuntu-latest
    needs: p1_validation
    if: success()
    steps:
      - uses: actions/checkout@v2
      - run: cargo build --release
      - run: cargo test --test integration_p2 --release -- --nocapture
      
  performance:
    runs-on: ubuntu-latest
    needs: p1_validation
    steps:
      - uses: actions/checkout@v2
      - run: cargo build --release
      - run: cargo test --test performance --release -- --nocapture
```

### Test Report

```markdown
# Integration Test Results

Date: 2026-05-12 10:30 UTC
Commit: a1b2c3d

## Summary
- P1 Validation: ✅ 40/50 PASS (80%)
- P2 Integration: ✅ 15/18 PASS (83%)
- Performance: ✅ 5/6 PASS (83%)
- Edge Cases: ✅ 8/10 PASS (80%)

## Details
...
```

---

## Success Criteria

### Coverage

- [x] P1 components tested in isolation
- [x] P2 components tested in isolation
- [x] P1+P2 integration scenarios
- [x] Complex topologies
- [x] Performance baselines
- [x] Edge cases
- [ ] **Combined pass rate >80%**

### Quality

- [x] Tests are deterministic (no flakes)
- [x] Test code is well-documented
- [x] Failures produce clear diagnostics
- [x] Logs are comprehensive
- [ ] **<5% flaky test rate**

### Performance

- [x] Test suite runs <90 min sequential
- [x] Test suite runs <45 min parallel
- [x] No resource leaks
- [x] Reliable metrics collection
- [ ] **Consistent, reproducible results**

---

## Timeline

```
Phase 1: Test Framework Setup     (2-3 hours)
Phase 2: P2 Integration Tests     (1 day)
Phase 3: Performance Tests        (1 day)
Phase 4: Edge Cases & Polish      (1 day)
Phase 5: CI/CD Integration        (0.5 day)
────────────────────────────────────────
TOTAL: 3-4 days
```

---

## Next Steps

1. **Create Test Framework** (Today, 2-3 hours)
   - IntegrationTestKit struct
   - Test utilities and helpers
   - Fixture loading

2. **Implement P2 Integration Tests** (Tomorrow, 1 day)
   - Service + Policy tests
   - Endpoint lifecycle tests
   - Multi-namespace tests

3. **Performance Tests** (Day 3, 1 day)
   - Throughput measurements
   - Latency benchmarks
   - Scaling tests

4. **Edge Cases** (Day 4, 1 day)
   - Error scenarios
   - Invalid inputs
   - Concurrent operations

5. **CI/CD Integration** (Day 5, 0.5 day)
   - GitHub Actions setup
   - Automated reporting
   - Failure notifications

---

**Document Version**: 1.0  
**Status**: Planning Complete, Ready for Implementation  
**GitHub Issue**: #52  
**Estimated Effort**: 3-4 days  
**Target**: >80% pass rate on all integration tests  
