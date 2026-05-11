# P0 Critical Fixes Implementation Plan

**Session 3 Phase 2**: Implement critical blockers to enable integration testing

**Timeline**: 2-4 hours active work  
**Estimated Completion**: This session  
**Status**: Ready to implement

## Overview

Two critical blockers prevent all integration testing:

1. **P0.1 - Operator Image Configuration**: Ensure upstream operator image pulls successfully
2. **P0.2 - CNI Socket Creation**: Verify CNI socket is created and CoreDNS becomes Ready

Both issues cascade from operator initialization. Once operator runs successfully:
- CRDs will be created (9 total)
- Agent will be deployed
- CNI socket will be created
- CoreDNS pods will become Ready
- Integration tests can execute

## Pre-Implementation Verification

### Current Configuration Status

Run this command to check current state:
```bash
bash scripts/verify-p0-status.sh
```

Expected output initially:
```
✗ Cluster kind does not exist
```

This is normal - we're starting fresh.

## Step 1: Build Rust Images

**Objective**: Build all Cilium-compatible images locally  
**Time**: ~5-10 minutes  
**Command**:
```bash
just build-images
```

**What happens**:
- Builds Release binaries for agent, daemon, dbg tools
- Creates container images:
  - `localhost:5000/seriousum/cilium-agent:local`
  - `localhost:5000/seriousum/cilium-dbg:local`
  - `localhost:5000/seriousum/operator-generic:local`
  - `localhost:5000/seriousum/hubble:local`
- Images stored in local Docker registry

**Success criteria**:
- All 4 images build successfully
- Images visible in `docker images | grep seriousum`

## Step 2: Create Kind Cluster

**Objective**: Bootstrap a fresh Kubernetes cluster for testing  
**Time**: ~2-3 minutes  
**Command**:
```bash
just cluster-create
```

**What happens**:
- Uses `kind` to create a local Kubernetes cluster
- Bootstraps with 1 control plane + 1 worker
- Creates kubeconfig at `./target/cilium-kind/kind.kubeconfig`
- Sets up local container registry connectivity

**Success criteria**:
- `kind get clusters` shows `kind` cluster
- `kubectl cluster-info` works

## Step 3: Load Images into Kind

**Objective**: Make built images available to the kind cluster  
**Time**: ~2-3 minutes  
**Command**:
```bash
just load-images
```

**What happens**:
- Loads Rust container images into kind cluster
- Makes images available at cluster's container runtime
- Prepares for Cilium installation

**Success criteria**:
- Images appear in `kind load docker-image` without errors
- `kubectl get nodes` shows all nodes Ready

## Step 4: Run FQDN Test (First Integration Test)

**Objective**: Execute smallest test suite to validate P0 fixes  
**Time**: ~7-10 minutes  
**Expected Result**: Either tests pass/fail or clear error messages
**Command**:
```bash
just test-fqdn
```

**What happens**:
1. Helm installs Cilium with:
   - Upstream operator: `quay.io/cilium/cilium-ci:latest`
   - Rust agent: `localhost:5000/seriousum/cilium-agent:local`
2. Operator pod starts and initializes
3. Operator creates 9 Cilium CRDs
4. Agent pods deploy to nodes
5. CNI socket is created at `/var/run/cilium/cilium.sock`
6. CoreDNS pods transition from ContainerCreating to Running
7. Test framework runs 3 FQDN test specs

**Success criteria for P0**:
- ✓ Operator pod transitions to Running (not ImagePullBackOff)
- ✓ 9 CRDs created: `kubectl get crd | grep cilium`
- ✓ Agent pods Running
- ✓ CoreDNS pods Running
- ✓ CNI socket exists: `kubectl exec -n kube-system <agent-pod> -- test -S /var/run/cilium/cilium.sock`

## Verification Commands (During Test Run)

Open another terminal and monitor:

```bash
# Watch cluster readiness
watch kubectl get nodes

# Watch Cilium component status
watch kubectl get pods -n kube-system -l k8s-app=cilium,app.kubernetes.io/name=cilium-operator

# Check CRD creation
kubectl get crd | grep cilium | wc -l  # Should show 9 eventually

# Check operator logs for errors
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator --tail=50

# Check agent logs
POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl logs -n kube-system $POD --tail=50

# Verify CNI socket once agent is ready
kubectl exec -n kube-system $POD -- ls /var/run/cilium/cilium.sock

# Check CoreDNS status
kubectl get pods -n kube-system -l k8s-app=kube-dns
```

## Troubleshooting P0 Issues

### If Operator Pod Stuck in ImagePullBackOff

**Symptom**: Operator pod shows ImagePullBackOff

**Diagnosis**:
```bash
kubectl describe pod -n kube-system -l app.kubernetes.io/name=cilium-operator
```

**Solutions**:
1. Check image pull permissions:
   ```bash
   docker pull quay.io/cilium/cilium-ci:latest
   ```

2. If pull fails, check network:
   ```bash
   kubectl run -it --image=alpine test -- sh
   # Inside container: wget https://quay.io
   ```

3. If that works, image may not exist with that exact tag:
   ```bash
   docker pull quay.io/cilium/cilium-ci:v1.15
   # Or use a specific build
   ```

### If CNI Socket Not Created

**Symptom**: Agent pod Running but socket missing at `/var/run/cilium/cilium.sock`

**Diagnosis**:
```bash
bash scripts/diagnose-cni-socket-timing.sh
```

**Root causes** (in order of likelihood):
1. Operator image not running → CRDs not created
2. Agent pod still initializing (check logs)
3. Mount permissions issue in agent pod
4. eBPF program loading failure

**Solutions**:
1. Verify operator running: `kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator`
2. Check agent logs: `kubectl logs -n kube-system -l k8s-app=cilium | grep -i error`
3. Check if systemd-resolved interfering: `systemctl status systemd-resolved`

### If CoreDNS Pods Stuck in ContainerCreating

**Symptom**: CoreDNS pods never transition to Running

**Diagnosis**:
```bash
kubectl describe pod -n kube-system -l k8s-app=kube-dns
```

**Expected events**:
- Pod created
- Container creating (waiting for CNI)
- Once CNI socket exists: IP assigned, container ready
- Pod Running

**If stuck**: Check agent pod CNI initialization

## Expected Timeline

### Quick Test Baseline (30 minutes)

```
0m   - Start
2m   - Build images (~2-3m)
5m   - Create cluster (~2-3m)
8m   - Load images (~2-3m)
10m  - Run test command
18m  - Operator initializes (~5-8m)
22m  - CRDs created, agent deploys
24m  - CNI socket ready
26m  - CoreDNS ready
30m  - Tests execute or report results
```

### Full Test Run (7-10 minutes after setup)

Once setup is complete, test runs take ~7-10 minutes:
- Setup time: ~2-5 min (operator, CRDs, agent)
- Test execution: ~3-5 min (3 FQDN specs)

## Phase 2 Success Criteria

After running these steps, verify:

✅ **P0.1 - Operator Image**: 
- Upstream `quay.io/cilium/cilium-ci:latest` pulls successfully
- Operator pod Running
- No ImagePullBackOff errors

✅ **P0.2 - CNI Socket**:
- `/var/run/cilium/cilium.sock` exists and accessible
- CoreDNS pods transition to Running
- Agent pods fully initialized

✅ **P0 Cascade Effects**:
- 9 Cilium CRDs created
- Agent successfully deployed
- Network connectivity established

✅ **Test Framework Operational**:
- Tests execute (pass or fail with clear errors)
- Framework reports real test results (not infrastructure errors)
- Logs available for debugging

## If All P0 Checks Pass

Next steps (Session 3 Phase 2 continuation):

1. **Review test results** from K8sFQDNTest
   - Note any failures and their nature
   - Compare against expectations

2. **Run additional test suites** to establish baseline
   - K8sDatapathServicesTest (50 specs)
   - K8sAgentPolicyTest (50 specs)

3. **Document blockers** found
   - Create issue tracking for functional gaps
   - Prioritize by impact

4. **Begin P1 implementation** (service subsystem)
   - Reference SERVICE_IMPLEMENTATION_SPEC.md
   - Implement service observer and eBPF maps

## Tools & References

**Diagnostic tools**:
- `scripts/verify-p0-status.sh` - Quick P0 status check
- `scripts/diagnose-cni-socket-timing.sh` - Deep CNI investigation
- `scripts/profile-cilium-startup.sh` - Timeline profiling

**Documentation**:
- ROOT_CAUSES_AND_FIXES.md - Root cause hierarchy
- CNI_SOCKET_TIMING_QUICKFIX.md - CNI troubleshooting
- SERVICE_IMPLEMENTATION_SPEC.md - Next phase specs

**Recipes**:
- `just test-fqdn` - Run FQDN test
- `just test-services` - Run services test
- `just test-sequential` - Run multiple suites

## Summary

P0 fixes are **configuration/operational** issues, not code issues:

1. **Build**: Already configured, uses upstream operator
2. **Deploy**: Helm chart has correct image settings
3. **Verify**: New verification script added

Expected outcome: P0 items fully resolved in this session, first test suite running with real results.

---

**Status**: Ready for implementation  
**Time estimate**: 2-4 hours  
**Next milestone**: K8sFQDNTest baseline results
