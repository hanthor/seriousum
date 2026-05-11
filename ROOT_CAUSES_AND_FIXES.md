# Root Causes & Fix Roadmap

**Session 3 Phase 1 Findings**: Complete root cause analysis of integration test failures

## Root Cause Hierarchy

### 🔴 P0 CRITICAL - Blocks All Integration Testing

#### 1. **Operator Image Pull Fails (401 UNAUTHORIZED)**
- **Problem**: Chart tries to pull `quay.io/cilium/cilium-ci-generic:latest`
- **Cause**: Non-existent image tag on registry OR missing credentials
- **Impact**: Operator can't initialize → CRDs never created → agent stuck
- **Fix**: 
  - Use local operator image (`localhost:5000/seriousum/operator-generic:local`)
  - OR provide quay.io credentials
  - OR use upstream `quay.io/cilium/cilium-ci:latest` (what we're doing)
- **Estimated Time**: 30 minutes
- **Status**: Partially addressed (using upstream), but local image build should work

#### 2. **Cilium CNI Socket Never Created**
- **Problem**: `/var/run/cilium/cilium.sock` doesn't exist
- **Root Cause**: Agent initialization blocked (operator issue cascades)
- **Impact**: CoreDNS pods stuck in ContainerCreating (timeout after 30s)
- **Dependencies**: Blocked by P0.1 (operator image)
- **Fix**:
  - Fix operator image pull first
  - Verify agent pod reaches full initialization
  - Check `/var/run/cilium/` mount permissions
- **Estimated Time**: 1-2 hours (once operator works)
- **Status**: Diagnosed, awaiting P0.1 fix

### 🟠 P1 HIGH - Blocks Service/Policy Tests

#### 3. **Agent Service Subsystem Not Initialized**
- **Problem**: Service load balancing subsystem only 17% initialized (2/12 components)
- **Root Causes** (in order):
  1. eBPF programs for services not loaded (no BPF maps created)
  2. Service observer not running (doesn't watch endpoints/services)
  3. kube-proxy integration not detected
  4. Endpoint manager not initialized
  5. Service-backend mapping logic missing

- **Missing Components**:
  - BPF maps: `lb4_services`, `lb6_services`, `lb4_backends`, `lb6_backends`, `endpoint_map`, `tunnel_map`
  - eBPF programs: XDP, TC ingress/egress for load balancing
  - Userspace logic: service observer, endpoint sync, map population
  - CNI integration: Not verifying kube-proxy coordination

- **Impact**: 9 K8sDatapathServicesTest scenarios fail in BeforeEach
- **Dependencies**: Needs P0 issues resolved first
- **Fix Strategy** (3-week plan):
  - Week 1: Fix CNI socket + operator CRD sync (P0 items)
  - Week 2: Implement service observer + basic eBPF maps
  - Week 3: Full service load balancing with all backends
- **Estimated Time**: 2-3 weeks
- **Status**: Fully documented in SERVICE_IMPLEMENTATION_SPEC.md

#### 4. **Operator-Agent CRD Synchronization Issues**
- **Problem**: CRDs never created (operator never runs)
- **Root Causes**:
  1. Operator image pull failure (P0.1)
  2. Agent health check times out (port 9879 unreachable)
  3. No explicit CRD wait logic (race conditions possible)
  4. Missing coordination handoff between components

- **Expected CRDs**: 9 total (CiliumNode, CiliumEndpoint, CiliumNetworkPolicy, etc.)
- **Current**: 0 created (operator never reaches initialization)
- **Impact**: Agent waits indefinitely for CRDs, test framework stalls
- **Dependencies**: Blocked by P0.1 (operator image)
- **Fix Strategy**:
  1. Fix operator image pull
  2. Add explicit CRD wait timeouts
  3. Add validation of CRD fields
  4. Add observability for CRD population timing
- **Estimated Time**: 4-6 hours (after operator fix)
- **Status**: Detailed in CRD_SYNC_VERIFICATION_REPORT.md

### 🟡 P2 MEDIUM - Optimization

#### 5. **Slow Startup Sequence (7 minutes)**
- **Problem**: Each test run takes 7 minutes before tests can execute
- **Bottleneck Phases**:
  1. Kind cluster bootstrap: ~2 min
  2. Operator initialization: ~2 min
  3. CRD creation: ~1 min
  4. Agent initialization: ~1.5 min
  5. CNI socket + CoreDNS: ~0.5 min

- **Optimization Opportunities**:
  - Parallelize operator + agent startup (currently sequential)
  - Cache CRD creation (skip if already present)
  - Optimize eBPF program loading
  - Reduce health check retries

- **Target**: < 3 minutes
- **Dependencies**: After P0/P1 fixes working
- **Fix Strategy**: Profile with `scripts/profile-cilium-startup.sh`
- **Estimated Time**: 1-2 weeks optimization work
- **Status**: Profiling script ready in scripts/profile-cilium-startup.sh

## Fix Priority Matrix

```
┌─────────────────────────────┬──────────┬──────────────┐
│ Issue                       │ Priority │ Dependency   │
├─────────────────────────────┼──────────┼──────────────┤
│ Operator image pull         │ P0       │ None         │
│ CNI socket creation         │ P0       │ Needs P0.1   │
│ Agent service subsystem     │ P1       │ Needs P0     │
│ CRD sync coordination       │ P1       │ Needs P0.1   │
│ Startup optimization        │ P2       │ Needs P1     │
└─────────────────────────────┴──────────┴──────────────┘
```

## Recommended Action Sequence

### Immediate (Next 2-4 hours)
1. **Fix Operator Image** (P0.1)
   - Ensure `quay.io/cilium/cilium-ci:latest` is used
   - Verify pull succeeds
   - Check operator pod transitions to Running

2. **Verify CRD Creation** (P0.1 follow-up)
   - Run: `kubectl get crd | grep cilium`
   - Should see 9 CRDs
   - Check operator logs for creation events

3. **Verify CNI Socket** (P0.2 follow-up)
   - Run: `kubectl exec -n kube-system <agent-pod> -- ls /var/run/cilium/cilium.sock`
   - Should show socket file with proper permissions
   - Check CoreDNS pods transition to Running

### Next Day (Full testing cycle)
4. **Run K8sAgentFQDNTest** (3 specs, should be quickest to green)
   - Uses upstream agent + operator (should mostly work)
   - Can validate framework with smaller test set
   - May reveal additional blockers

5. **Debug Service Test Failures** (P1 work)
   - With P0 items working, focus on service subsystem
   - Check which eBPF components are missing
   - Start implementing missing components

### This Week
6. **Implement Service Components** (P1 high-impact)
   - Service observer
   - eBPF maps for load balancing
   - Backend mapping logic

7. **Get First Service Test to Green**
   - Should enable momentum for policy tests

## Diagnostic Commands

### Quick Status Check
```bash
# Operator image & status
kubectl get deployment -n kube-system cilium-operator -o yaml | grep image:

# CRDs present?
kubectl get crd | grep cilium | wc -l

# CNI socket exists?
AGENT=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl exec -n kube-system $AGENT -- test -S /var/run/cilium/cilium.sock && echo "Socket OK" || echo "Socket MISSING"

# CoreDNS status?
kubectl get pods -n kube-system -l k8s-app=kube-dns
```

### Full Investigation
```bash
# Run automated diagnostics
bash scripts/diagnose-cni-socket-timing.sh
bash scripts/profile-cilium-startup.sh

# Check logs
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator --tail=50
kubectl logs -n kube-system -l k8s-app=cilium --tail=50 | grep -i error
```

## Success Criteria

- [ ] Operator image pulls successfully
- [ ] 9 CRDs created in API server
- [ ] CNI socket created at `/var/run/cilium/cilium.sock`
- [ ] CoreDNS pods transition to Running
- [ ] K8sAgentFQDNTest runs (may still fail, but framework executes)
- [ ] K8sDatapathServicesTest runs further (gets past BeforeEach failures)

## Expected Timeline

- **P0 fixes**: This week (estimated 2-4 hours active work)
- **P1 fixes**: Next 2-3 weeks (estimated 40-60 hours implementation)
- **P2 optimizations**: Following week (estimated 10-20 hours)

**Overall**: First test suite potentially green in 1-2 weeks with focused effort

## Reference Documents

| Document | Purpose |
|----------|---------|
| SERVICE_IMPLEMENTATION_SPEC.md | How to implement service subsystem (5 components, Rust code examples) |
| CNI_SOCKET_TIMING_QUICKFIX.md | Step-by-step guide to diagnose & fix socket issue |
| CRD_SYNC_DIAGNOSTIC_CHECKLIST.md | Live cluster verification steps |
| scripts/diagnose-cni-socket-timing.sh | Automated diagnosis tool |
| scripts/profile-cilium-startup.sh | Timeline profiling of all phases |
| scripts/run-cilium-sequential-suites.sh | Efficient multi-suite runner |

---

**Status**: All root causes identified and documented. Ready for implementation phase.

**Recommendation**: Start with P0 operator image fix today. Expect P0 items complete by end of week, P1 service subsystem work can begin in parallel.
