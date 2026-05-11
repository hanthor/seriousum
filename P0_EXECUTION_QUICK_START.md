# P0 Execution Quick Start

**Session 3 Phase 2**: Run the unified build-and-test pipeline

**New Capability**: One command does everything (build → cluster → load → test)

## Quick Start

### Default (Fastest - FQDN Test, 3 specs)

```bash
just run
```

**What happens**:
1. Builds release binaries
2. Builds container images
3. Creates fresh kind cluster
4. Loads images into cluster
5. Runs K8sFQDNTest (3 specs, ~5 min)

**Estimated time**: 25-30 minutes total
**Success**: Tests execute and report results

### Run Different Test Suites

```bash
# Run services test (50 specs, ~10-15 min)
just run K8sDatapathServicesTest

# Run policies test (50 specs, ~10-15 min)
just run K8sAgentPolicyTest

# Run with custom timeout (e.g., 30 minutes)
just run K8sFQDNTest 30m

# Run policies with 45m timeout
just run K8sAgentPolicyTest 45m
```

## Real-Time Monitoring (Optional)

Open another terminal to watch progress:

```bash
# Watch cluster nodes
watch kubectl cluster-info --kubeconfig ./target/cilium-kind/kind.kubeconfig

# Watch Cilium components
watch -n 2 'export KUBECONFIG=./target/cilium-kind/kind.kubeconfig && kubectl get pods -n kube-system -l k8s-app=cilium,app.kubernetes.io/name=cilium-operator'

# Check CRD creation (should reach 9)
watch -n 5 'export KUBECONFIG=./target/cilium-kind/kind.kubeconfig && kubectl get crd | grep cilium | wc -l'
```

## What the Recipe Does

### Phase 1: Build (2-3 minutes)
```
[1/5] Building release binaries
      └─ cargo build --workspace --release
      └─ Output: /target/release/cilium-agent, etc.

[2/5] Building container images
      └─ Images: cilium-agent, cilium-dbg, operator-generic, hubble, clustermesh-apiserver
      └─ Tagged: localhost:5000/seriousum/*:local
```

### Phase 2: Cluster (2-3 minutes)
```
[3/5] Resetting kind cluster
      └─ Deletes existing cluster (if any)
      └─ Creates fresh cluster with 1 control-plane + 1 worker
      └─ Kubeconfig: ./target/cilium-kind/kind.kubeconfig
```

### Phase 3: Load (2-3 minutes)
```
[4/5] Loading images into cluster
      └─ kind load docker-image for all 5 images
      └─ Makes images available to cluster
```

### Phase 4: Test (5-15 minutes depending on suite)
```
[5/5] Running tests
      └─ Helm installs Cilium with:
         - Upstream operator: quay.io/cilium/cilium-ci:latest
         - Rust agent: localhost:5000/seriousum/cilium-agent:local
      └─ Tests execute:
         - K8sFQDNTest: 3 specs, ~5 min
         - K8sDatapathServicesTest: 50 specs, ~10-15 min
         - K8sAgentPolicyTest: 50 specs, ~10-15 min
```

## Expected Output

### Successful Run (K8sFQDNTest Example)

```
Starting full build and test pipeline for K8sFQDNTest
Suite: K8sFQDNTest
Timeout: 12m

[1/5] Building release binaries...
✓ Binaries built

[2/5] Building container images...
✓ Images built

[3/5] Resetting kind cluster...
✓ Cluster ready

[4/5] Loading images into cluster...
✓ Images loaded

[5/5] Running K8sFQDNTest tests...
==> installing Cilium via Helm
==> waiting for Cilium operator...
==> waiting for pods...
==> running ginkgo tests

Ginkgo Suite: FQDN Test Suite
  K8sFQDNTest
    ✓ Test 1
    ✓ Test 2
    ✓ Test 3

Tests completed: 3 passed
```

## Troubleshooting

### Recipe Fails During Build
- **Issue**: `cargo build` fails
- **Solution**: Run `cargo check` first to see errors
- **Command**: `cd /var/home/james/dev/seriousum && cargo check --workspace --release`

### Recipe Fails During Cluster Creation
- **Issue**: `kind create cluster` fails
- **Solution**: Delete existing cluster first
- **Command**: `kind delete cluster --name kind`

### Recipe Fails During Image Load
- **Issue**: Timeout loading images to kind
- **Solution**: This is normal for first run (images are large). Wait for timeout, then run again - images will be cached.
- **Workaround**: Run with increased timeout: `just run K8sFQDNTest 60m`

### Recipe Fails During Tests
- **Issue**: Tests fail or don't run
- **Solution**: Check logs
  - Operator logs: `kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator --kubeconfig ./target/cilium-kind/kind.kubeconfig`
  - Agent logs: `kubectl logs -n kube-system -l k8s-app=cilium --kubeconfig ./target/cilium-kind/kind.kubeconfig`

## Manual Alternative

If you prefer step-by-step control:

```bash
# Instead of: just run
# Do this:

just build                          # Build binaries
just build-images                   # Build images
just cluster-reset                  # Create cluster
just load-images                    # Load images
just test-fqdn                      # Run tests
```

But `just run` does all of these in one shot.

## P0 Validation Checklist

After running `just run`, verify P0 items are fixed:

- [ ] Build completes successfully
- [ ] Cluster created and Ready
- [ ] Images loaded (no errors)
- [ ] Tests start executing (not stuck)
- [ ] Operator image pulls successfully (check logs)
- [ ] CRDs created (9 total)
- [ ] Agent pods Running
- [ ] CoreDNS pods Running
- [ ] CNI socket created
- [ ] Tests complete with results (pass or fail OK)

## Usage Examples

### Test Everything Quickly
```bash
just run  # Default FQDN (fastest)
```

### Test Services Component
```bash
just run K8sDatapathServicesTest
```

### Extended Test with Longer Timeout
```bash
just run K8sAgentPolicyTest 45m
```

### Run Multiple Suites Sequentially (After first setup)
```bash
just test-sequential
```

## Integration with Documentation

- **Root Causes**: ROOT_CAUSES_AND_FIXES.md
- **P0 Implementation Plan**: P0_IMPLEMENTATION_PLAN.md
- **Diagnostic Tools**: scripts/verify-p0-status.sh
- **Service Implementation**: SERVICE_IMPLEMENTATION_SPEC.md

## Next Steps After P0 Validation

Once P0 is validated:

1. **Review test results**
   - Do tests pass?
   - What are the failures?
   - Are they expected?

2. **Document findings**
   - Log results and observations
   - Compare to expectations
   - Plan P1 fixes

3. **Start P1 implementation** (if needed)
   - Service subsystem components
   - Reference: SERVICE_IMPLEMENTATION_SPEC.md

## Timeline

- **Build**: ~2-3 min
- **Cluster**: ~2-3 min
- **Load**: ~2-3 min
- **Test**:
  - FQDN: ~5 min (3 specs)
  - Services: ~10-15 min (50 specs)
  - Policies: ~10-15 min (50 specs)

**Total**: 12-35 minutes depending on suite

## Summary

**Command**: `just run [suite] [timeout]`

**Default behavior**:
```bash
just run  # Runs K8sFQDNTest with 12m timeout
```

**Full pipeline in one command**:
- Builds everything
- Creates fresh cluster
- Loads images
- Runs tests
- Reports results

**Recommended for P0 validation**: Run this now to test operator and CNI socket fixes.

---

**Status**: Ready to execute  
**Estimated P0 completion time**: 30 minutes for first run
**Expected outcome**: P0 items validated, tests running with real results
