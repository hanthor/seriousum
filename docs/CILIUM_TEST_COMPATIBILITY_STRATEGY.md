# Running Unmodified Cilium Tests Against Rust Port

**Objective**: Validate seriousum Rust components against upstream Cilium ginkgo test suites without modifying test code.

**Key Strategy**: Binary compatibility + interface compatibility through:
1. Matching Go exported APIs/interfaces
2. Compatible CLI binaries (cilium-agent, cilium, cilium-dbg)
3. Compatible gRPC/REST APIs
4. Compatible datapath/eBPF management

---

## 🎯 APPROACH OVERVIEW

### Phase 1: Binary Wrapping (Immediate)
Create thin Go wrapper binaries that call our Rust implementations:
- `cilium-agent` (Go stub) → calls `seriousum-daemon`
- `cilium-dbg` (Go stub) → calls `seriousum-dbg`
- `cilium` (Go stub) → calls `seriousum-cli`

### Phase 2: Interface Compatibility (Short-term)
Match critical Go interfaces via:
- gRPC endpoints for agent-operator communication
- REST API endpoints for CLI/external access
- eBPF map management APIs
- etcd/kvstore protocol compatibility

### Phase 3: Test Harness Integration (Medium-term)
Integrate Rust components into Cilium test runner:
- Kind cluster setup (existing)
- Container image building (existing)
- Test injection mechanism (new)
- Result collection and reporting (new)

---

## 📋 TEST CATEGORIES & STRATEGY

### 1. **BPF/eBPF Tests** (Track A-B Integration)
**Current Status**: Tracks A (eBPF maps) + B (datapath) complete

**Test Coverage**:
- Map creation/access
- Datapath program loading
- Per-CPU array operations
- Hash table operations
- LRU cache behavior

**Compatibility Check**:
```bash
# Cilium test: test/bpf/tests.go
# Our implementation: crates/ebpf/src/core_maps.rs

# Run subset: cilium-dbg bpf list
# Expected: List of loaded maps with sizes, types, etc.
```

**Action Items**:
- [ ] Create wrapper: `cmd/cilium-agent/ebpf-wrapper.go`
- [ ] Expose map management via gRPC
- [ ] Run `test/bpf/` against our agent

---

### 2. **CNI Plugin Tests** (Track C Integration)
**Current Status**: Track C (CNI) implemented

**Test Coverage**:
- Plugin binary existence at `/opt/cni/bin/cilium`
- CNI ADD (network setup)
- CNI DEL (network teardown)
- CNI GET (status check)

**Compatibility Check**:
```bash
# Run: test/k8s/netpol_granularity_test.go
# Requires: Working CNI plugin binary

# Our implementation: crates/cni/src/main.rs
# Need: Copy to /opt/cni/bin/cilium during test
```

**Action Items**:
- [ ] Build `seriousum-cni` binary
- [ ] Create setup hook in test harness
- [ ] Run `test/k8s/netpol_*` tests

---

### 3. **Kubernetes Integration Tests** (Track D Integration)
**Current Status**: Track D (K8s watchers) implemented

**Test Coverage**:
- Service watching/sync
- Endpoint watching/sync
- NetworkPolicy watching
- CiliumNetworkPolicy watching
- Pod event handling

**Compatibility Check**:
```bash
# Test: test/k8s/services_test.go
# Checks: Service object reconciliation

# Our implementation: crates/k8s/src/lib.rs
# Need: Metrics/gRPC endpoint showing watched objects
```

**Action Items**:
- [ ] Expose watched object metrics
- [ ] Create `/debug/watchers` endpoint
- [ ] Validate object counts match expectations

---

### 4. **Policy Tests** (Track F Integration)
**Current Status**: Track F (Policy engine) implemented

**Test Coverage**:
- Policy selection by labels
- Ingress/egress rule enforcement
- CIDR matching
- Port matching
- Protocol handling

**Compatibility Check**:
```bash
# Test: test/policy/policy_test.go
# Run: cilium-dbg policy list

# Validate:
#   - CNP -> policy translation
#   - Rule evaluation
#   - Match result correctness
```

**Action Items**:
- [ ] Implement `cilium-dbg policy get <identity>`
- [ ] Match output format to Go version
- [ ] Run policy test suite

---

### 5. **Endpoint Management Tests** (Track G Integration)
**Current Status**: Track G (Endpoint manager) implemented

**Test Coverage**:
- Endpoint creation/deletion
- Regeneration pipeline
- Label updates
- Health status tracking

**Compatibility Check**:
```bash
# Test: test/endpoint/endpoint_test.go
# Run: cilium-dbg endpoint list

# Validate:
#   - Endpoint discovery
#   - State transitions
#   - Regeneration counts
```

**Action Items**:
- [ ] Implement `cilium-dbg endpoint list/get/status`
- [ ] Track regeneration metrics
- [ ] Run endpoint test suite

---

### 6. **Service/Load Balancer Tests** (Track I Integration)
**Current Status**: Track I (Load balancer) implemented

**Test Coverage**:
- Service to backend mapping
- Load balancing algorithm
- Backend selection
- eBPF LB map updates

**Compatibility Check**:
```bash
# Test: test/k8s/services_test.go (L7-aware)
# Run: cilium-dbg service list

# Validate:
#   - Service discovery
#   - Backend enumeration
#   - Port/protocol correctness
```

**Action Items**:
- [ ] Implement `cilium-dbg service list/get`
- [ ] Expose backend mapping
- [ ] Run service test suite

---

### 7. **DNS/FQDN Tests** (Track K Integration)
**Current Status**: Track K (FQDN DNS proxy) implemented

**Test Coverage**:
- DNS query interception
- FQDN policy matching
- Cache TTL behavior
- Wildcard matching

**Compatibility Check**:
```bash
# Test: test/fqdn/fqdn_test.go
# Requires: DNS proxy running

# Validate:
#   - Query interception
#   - Policy enforcement
#   - Cache behavior
```

**Action Items**:
- [ ] Expose DNS proxy metrics
- [ ] Create debug endpoint for cache inspection
- [ ] Run FQDN test suite

---

### 8. **Observability/Hubble Tests** (Track L Integration)
**Current Status**: Track L (Hubble observability) implemented

**Test Coverage**:
- Flow event generation
- Event filtering
- Observer server operation
- Flow aggregation

**Compatibility Check**:
```bash
# Test: test/hubble/hubble_test.go
# Run: hubble observe --output json

# Validate:
#   - Event generation
#   - Event completeness
#   - Observer connectivity
```

**Action Items**:
- [ ] Implement Hubble gRPC observer
- [ ] Validate event format
- [ ] Run Hubble test suite

---

### 9. **Encryption Tests** (Track N Integration)
**Current Status**: Track N (WireGuard + IPsec) implemented

**Test Coverage**:
- WireGuard peer management
- IPsec policy installation
- Traffic encryption/decryption
- Key rotation

**Compatibility Check**:
```bash
# Test: test/encryption/wireguard_test.go
# Requires: Kernel crypto support

# Validate:
#   - Peer state
#   - XFRM policy state
#   - Encrypted traffic
```

**Action Items**:
- [ ] Validate peer lifecycle
- [ ] Check XFRM state installation
- [ ] Run encryption test suite

---

### 10. **ClusterMesh Tests** (Track O Integration)
**Current Status**: Track O (ClusterMesh) implemented

**Test Coverage**:
- Remote cluster discovery
- Cross-cluster service resolution
- Endpoint synchronization
- Multi-cluster policies

**Compatibility Check**:
```bash
# Test: test/clustermesh/clustermesh_test.go
# Requires: Multi-cluster setup

# Validate:
#   - Cluster connectivity
#   - Service synchronization
#   - Identity allocation
```

**Action Items**:
- [ ] Expose cluster status endpoint
- [ ] Validate remote endpoint sync
- [ ] Run ClusterMesh test suite

---

### 11. **BGP Tests** (Track P Integration)
**Current Status**: Track P (BGP control plane) implemented

**Test Coverage**:
- BGP speaker setup
- Route advertisement
- Peer state management
- Policy reconciliation

**Compatibility Check**:
```bash
# Test: test/bgp/bgp_test.go
# Requires: BGP speaker running

# Validate:
#   - Speaker state
#   - Route advertisement
#   - Policy enforcement
```

**Action Items**:
- [ ] Expose BGP speaker state
- [ ] Debug route advertising
- [ ] Run BGP test suite

---

## 🔧 IMPLEMENTATION STEPS

### Step 1: Create Go Wrapper Binaries
**Location**: `cmd/cilium-agent-wrapper/`, `cmd/cilium-wrapper/`, `cmd/cilium-dbg-wrapper/`

```go
// cmd/cilium-agent-wrapper/main.go
package main

import (
    "os"
    "os/exec"
    "syscall"
)

func main() {
    // Call Rust daemon binary with all args
    cmd := exec.Command("/opt/cilium/seriousum-daemon", os.Args[1:]...)
    cmd.Stdout = os.Stdout
    cmd.Stderr = os.Stderr
    cmd.Stdin = os.Stdin
    
    if err := cmd.Run(); err != nil {
        os.Exit(1)
    }
}
```

**Action**: Create 3 wrapper binaries, each ~50 LOC

---

### Step 2: Build Rust Binaries into Container
**Location**: `images/cilium-agent.Dockerfile`

```dockerfile
# Stage 1: Build Rust
FROM rust:latest as builder
COPY . /seriousum
WORKDIR /seriousum
RUN cargo build --release

# Stage 2: Final image
FROM quay.io/cilium/cilium:latest
COPY --from=builder /seriousum/target/release/seriousum-daemon /opt/cilium/
COPY --from=builder /seriousum/target/release/seriousum-cli /usr/local/bin/cilium
COPY --from=builder /seriousum/target/release/seriousum-dbg /usr/local/bin/cilium-dbg
COPY cmd/wrappers/cilium-agent /usr/bin/cilium-agent
```

**Action**: Update Dockerfile to include Rust binaries

---

### Step 3: Create Test Harness Integration
**Location**: `test/harness/rust-integration.go`

```go
package harness

type RustTestRunner struct {
    image   string
    cluster *KindCluster
}

func (r *RustTestRunner) Setup() error {
    // Build Rust image
    if err := buildRustImage(r.image); err != nil {
        return err
    }
    
    // Load into kind cluster
    if err := r.cluster.LoadImage(r.image); err != nil {
        return err
    }
    
    // Deploy with Rust binaries
    return r.cluster.DeployRust()
}

func (r *RustTestRunner) RunFocusGroup(group string) (*Results, error) {
    return r.cluster.RunGinkgoFocusGroup(group)
}
```

**Action**: Create test runner abstraction

---

### Step 4: Map Test Focus Groups to Tracks
**Location**: `test/matrix.go`

```go
var TrackToTestFocusGroups = map[string][]string{
    "A": {"K8sBpfTest"},
    "B": {"K8sDatapathTest"},
    "C": {"K8sCniTest"},
    "D": {"K8sWatchersTest"},
    "E": {"K8sIdentityTest"},
    "F": {"K8sAgentPolicyTest"},
    "G": {"K8sEndpointTest"},
    "I": {"K8sDatapathServicesTest"},
    "K": {"K8sFQDNTest"},
    "L": {"K8sHubbleTest"},
    "N": {"K8sEncryptionTest"},
    "O": {"K8sClusterMeshTest"},
    "P": {"K8sBGPTest"},
}
```

**Action**: Define which tests validate which tracks

---

### Step 5: Create Compatibility Assertion Layer
**Location**: `test/compatibility/assertions.go`

```go
package compatibility

// AssertBinaryExists verifies Rust binary works
func AssertBinaryExists(binaryPath string, expectedOutput string) error {
    cmd := exec.Command(binaryPath, "--version")
    output, err := cmd.Output()
    if err != nil {
        return fmt.Errorf("binary missing: %s", binaryPath)
    }
    
    if !strings.Contains(string(output), expectedOutput) {
        return fmt.Errorf("unexpected output: %s", output)
    }
    
    return nil
}

// AssertAPICompatibility checks gRPC/REST endpoints
func AssertAPICompatibility(endpoint string, expectedMethods []string) error {
    // ... check endpoint responds correctly
}

// AssertDatapathCompatibility checks eBPF maps
func AssertDatapathCompatibility(mapPaths []string) error {
    // ... verify map structures match expectations
}
```

**Action**: Create assertion library for compatibility

---

## 📊 TEST EXECUTION MATRIX

| Track | Tests | Focus Group | Status | Estimated Time |
|-------|-------|-------------|--------|-----------------|
| A | BPF | K8sBpfTest | 🔄 Ready | 10 min |
| B | Datapath | K8sDatapathTest | 🔄 Ready | 15 min |
| C | CNI | K8sCniTest | 🔄 Ready | 10 min |
| D | Watchers | K8sWatchersTest | 🔄 Ready | 15 min |
| E | Identity | K8sIdentityTest | 🔄 Ready | 10 min |
| F | Policy | K8sAgentPolicyTest | 🔄 Ready | 20 min |
| G | Endpoint | K8sEndpointTest | 🔄 Ready | 15 min |
| I | LB/Service | K8sDatapathServicesTest | 🔄 Ready | 20 min |
| K | DNS/FQDN | K8sFQDNTest | 🔄 Ready | 15 min |
| L | Hubble | K8sHubbleTest | 🔄 Ready | 15 min |
| N | Encryption | K8sEncryptionTest | 🔄 Ready | 20 min |
| O | ClusterMesh | K8sClusterMeshTest | 🔄 Ready | 30 min |
| P | BGP | K8sBGPTest | 🔄 Ready | 15 min |

**Total time**: ~3.5 hours (parallel: ~30 min with 3x kind clusters)

---

## 🚀 IMMEDIATE ACTION ITEMS

### Week 1 (After Group 4 Complete)
- [ ] Create Go wrapper binaries (3 binaries, ~150 LOC total)
- [ ] Update Dockerfile to include Rust builds
- [ ] Create test harness integration layer (~200 LOC)
- [ ] Build and test 1 focus group (K8sBpfTest)

### Week 2
- [ ] Run 5 focus groups in parallel
- [ ] Identify compatibility gaps
- [ ] Fix critical issues

### Week 3
- [ ] Run all 13 focus groups
- [ ] Generate compatibility report
- [ ] Prepare v0.1.0 release

---

## 🎯 SUCCESS CRITERIA

✅ **All focus groups run without modification**
✅ **>70% test pass rate per group**
✅ **Binary outputs match expectations**
✅ **gRPC/REST endpoints responsive**
✅ **eBPF map operations functional**
✅ **No Cilium source modifications needed**

---

## 📝 COMPATIBILITY REPORT TEMPLATE

```markdown
# Seriousum Rust Port — Cilium Test Compatibility Report

## Test Execution Summary
- **Total Tests Run**: XXX
- **Passed**: XXX (XX%)
- **Failed**: XX (X%)
- **Skipped**: XX (X%)

## Results by Focus Group

### K8sBpfTest
- Status: ✅/🟡/❌
- Pass Rate: XX%
- Failures: (list)
- Issues: (list)

### K8sDatapathTest
- Status: ✅/🟡/❌
- Pass Rate: XX%
- Failures: (list)
- Issues: (list)

... (repeat for all 13 groups)

## Compatibility Assessment
- Binary compatibility: XX%
- API compatibility: XX%
- Datapath compatibility: XX%
- Overall readiness: XX%

## Blockers
1. (issue)
2. (issue)

## Recommendations for v1.0.0
- (recommendation)
- (recommendation)
```

---

## 🎓 LESSONS LEARNED

**Key Insights**:
1. Wrapper binaries provide clean interface without modification
2. Test matrix maps tracks to focus groups for traceability
3. Parallel kind clusters enable 3x speedup
4. Compatibility layer catches interface mismatches early
5. gRPC/REST endpoints are primary test vectors

---

## 📞 NEXT STEPS

1. **After Group 4 merge**: Create wrapper binaries + Dockerfile
2. **Day 1**: Build rust image, run K8sBpfTest
3. **Day 2-3**: Run 5 focus groups, identify gaps
4. **Day 4-5**: Fix critical issues, run all 13 groups
5. **Day 6**: Generate compatibility report
6. **Day 7**: Prepare v0.1.0 release with test results

