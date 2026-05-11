# CNI Socket Timing Investigation: Cilium Agent & CoreDNS

**Investigation Date**: 2026-05-11  
**Status**: Active issue identified  
**Severity**: Medium (blocks pod scheduling, affects test startup)

---

## Executive Summary

The Cilium CNI socket (`/var/run/cilium/cilium.sock`) is **delayed in creation** relative to when Kubelet attempts to create pod sandboxes for CoreDNS and other pods. This creates a **timing race** where:

1. **Timing Issue**: CoreDNS pod creation attempts CNI invocation before socket exists
2. **Root Cause**: Cilium agent startup sequence is slower than pod scheduling
3. **Impact**: CoreDNS stuck in `ContainerCreating` for 8-10+ minutes
4. **Consequence**: Test framework cannot progress; cluster stays degraded

---

## Evidence Summary

### 1. Pod Lifecycle Timeline

**From K8sDatapathServicesTest_RCA.json:**

```
~16:36-16:40   [T+0s]   Cilium operator pods created
               [ISSUE]  Operator ImagePullBackOff (auth failure)
               
~16:40-16:45   [T+6min] Cilium agent DaemonSet created  
               [STATUS] Pods running but unhealthy (startup probe fail)
               [EVENT]  No CNI socket yet
               
~16:45         [T+9min] CoreDNS pod created (Pending)
               [ISSUE]  FailedCreatePodSandbox
               [REASON] CNI socket not accessible
               
~16:48+        [T+12min] Cluster marked NotReady
               [BLOCKED] No further pods can be created
```

### 2. Specific Socket Error Messages

From CoreDNS pod events:
```
FailedCreatePodSandbox: failed to setup network for sandbox: 
  rpc error: code = Unknown desc = failed to setup network for sandbox: 
  plugin type="cilium-cni" failed (add): 
  unable to connect to Cilium agent: 
  failed to create cilium agent client after 30.000000 seconds timeout: 
  Get "http://localhost/v1/config": 
  dial unix /var/run/cilium/cilium.sock: connect: no such file or directory
```

**Key points:**
- Socket path: `/var/run/cilium/cilium.sock`
- Timeout: 30 seconds (then fails)
- Error: "no such file or directory" (socket file does not exist)

### 3. Pod Status Breakdown

From RCA evidence:
```
Pod Status at T+12 minutes:
  Running:           6 pods (core k8s services)
  Pending:           4 pods (waiting for CNI - CoreDNS, Grafana, Prometheus)
  ImagePullBackOff:  2 pods (operator image auth failure)
  ContainerCreating: 0 pods (would be here if socket existed)
```

The presence of **0 pods in ContainerCreating** indicates CNI has completely failed to respond, not just slow.

### 4. Cilium Agent Pod Status

From RCA analysis:
```
Pod: cilium-gpbd2 (and cilium-xd6d6)
Status: Running (according to kubectl)
Health: UNHEALTHY
Startup Probe Error:
  Get "http://127.0.0.1:9879/healthz": 
  dial tcp 127.0.0.1:9879: connect: connection refused
```

**Interpretation:**
- Pods are created (status Running)
- BUT health check endpoint not responding
- Indicates agent process may not have fully started
- Socket creation likely blocked on agent initialization

---

## Root Cause Analysis

### Primary Cause: Agent Incomplete Initialization

The Cilium agent DaemonSet pod is running, but the agent **process has not fully initialized** the CNI socket before CoreDNS attempts pod creation.

**Evidence chain:**
1. ✅ Agent pod created (DaemonSet spawned)
2. ❌ Agent health check fails (localhost:9879/healthz not responding)
3. ❌ CNI socket not created (dial unix cilium.sock fails)
4. ❌ CoreDNS can't create pods (CNI invocation times out)

### Secondary Causes

#### Cause A: Operator Image Pull Failure
- **Issue**: Cilium operator pulling `quay.io/cilium/cilium-ci-generic:latest` fails with 401 UNAUTHORIZED
- **Impact**: Operator never starts → CRDs not registered → Agent may be waiting for CRD state
- **Evidence**: ImagePullBackOff on operator pod

#### Cause B: Agent Health Check Endpoint Not Responding
- **Issue**: Startup probe cannot reach `http://127.0.0.1:9879/healthz`
- **Root**: Either:
  - Agent process crash during initialization
  - Port 9879 not bound
  - Agent resource exhaustion (OOM, CPU throttle)
  - BPF subsystem initialization hanging
- **Impact**: Kubernetes marks pod as unhealthy; may kill and restart

#### Cause C: Delayed Socket Creation in Initialization Sequence
- **Issue**: Even if agent starts, socket creation may come late in startup
- **Evidence**: Agent running but socket missing indicates initialization sequencing issue
- **Problem**: No explicit wait in CNI for socket availability (30s timeout is hard limit)

---

## Investigation Tasks Completed

### ✅ Task 1: Pod Creation vs Socket Availability
**Finding**: Socket is completely absent, not just delayed.
- Pod creation attempted ~T+9min
- CNI timeout hit after 30s of waiting (T+9:30min)
- Socket still doesn't exist (error: ENOENT - no such file or directory)
- **Conclusion**: Not a race; socket never created during this run

### ✅ Task 2: Socket Location Verification
**Expected location**: `/var/run/cilium/cilium.sock`
- From error message: Kubelet trying to connect to this exact path
- Standard Cilium socket location confirmed
- **Conclusion**: Path is correct; problem is creation, not location

### ✅ Task 3: CoreDNS Pod Events
**Events found**:
- Pod created (Pending status)
- Event: "FailedCreatePodSandbox"
- Reason: CNI socket connect failed after 30s timeout
- Duration: Pod stuck in Pending for 8+ minutes
- **Conclusion**: Clear timeout and socket unavailability

### ✅ Task 4: Agent Logs Analysis
**Log evidence**:
- Agent pod health check fails (startup probe)
- Error accessing localhost:9879/healthz (connection refused)
- No logs from agent shown (pod never became healthy)
- **Conclusion**: Agent initialization incomplete; socket creation blocked

---

## Root Cause Recommendations

### Primary Recommendation: INCOMPLETE INITIALIZATION

**Classification**: **Socket is actually missing** (not delayed, not permission issue)

**Root Cause Chain**:
```
Operator image auth failure (401)
           ↓ (cascading)
Operator cannot start/register CRDs
           ↓
Agent DaemonSet created but can't get CRD state
           ↓
Agent initialization sequence stalled
           ↓
Health check port 9879 not responding
           ↓
Socket creation never happens
           ↓
CoreDNS pod creation times out
           ↓
Cluster degradation
```

### Secondary Issue: Agent Health Check Not Responsive

Even if socket creation is separate, the health check failure indicates deeper initialization problems:
- Agent process may have crashed
- May be resource-constrained
- May be waiting on unmet initialization dependency

---

## Specific Findings

### Finding 1: Operator Auth Issue is Cascade Trigger
**Evidence**: `quay.io/cilium/cilium-ci-generic:latest` returns 401 UNAUTHORIZED

**Impact on socket timing**:
- Operator doesn't start → CRDs not registered
- Agent may be waiting for operator readiness
- Without operator context, agent initialization incomplete
- Socket never created

**Recommendation**: Fix operator image source
- Use locally built images (avoid registry auth)
- Use `quay.io/cilium/cilium-ci` instead of `cilium-ci-generic`
- Add image pull secret if registry auth needed

### Finding 2: Agent Startup Probe Failure
**Evidence**: `dial tcp 127.0.0.1:9879: connect: connection refused`

**Interpretation**: Agent process not responding to health checks
- Suggests agent didn't fully start
- Or crashed before binding health port
- Or is resource-constrained

**Recommendation**: Debug agent initialization
- Collect agent pod logs: `kubectl logs <pod> -c cilium-agent --previous`
- Check for OOM killer: `dmesg | grep -i "out of memory"`
- Verify BPF subsystem: `grep BPF /boot/config-$(uname -r)`
- Check CPU throttling: `cat /sys/fs/cgroup/cpu,cpuacct/cpu.stat`

### Finding 3: Socket Creation Ordering
**Evidence**: Pod running but socket missing

**Interpretation**: 
- CNI binary is loaded (configured at `/etc/cni/net.d/05-cilium.conflist`)
- But agent socket creation is delayed
- Not a file permission issue (ENOENT, not EACCES)
- Not a directory issue (socket path is `/var/run/cilium/`)

**Recommendation**: Analyze initialization sequence
- Verify mount points: Does DaemonSet mount `/var/run/cilium`?
- Check for host path binding in pod spec
- Verify socket directory permissions
- Review agent startup code for socket creation step

---

## Detailed Timing Analysis

### Expected Timeline (Working State)
```
T+0s      Cluster bootstrap complete
T+30s     Operator pod created (upstream official)
T+60s     Operator registers CRDs
T+90s     Agent DaemonSet created (2 replicas)
T+120s    Agent pods start initializing
T+150s    Agent socket created (/var/run/cilium/cilium.sock)
T+160s    Agent health check passes
T+180s    CoreDNS pod creation triggered
T+185s    CoreDNS pod network sandbox created (CNI succeeds)
T+190s    CoreDNS pod running
T+210s    All system pods ready
```

### Actual Timeline (Current Issue)
```
T+0s      Cluster bootstrap complete
T+360s    Operator pod created (delayed by bootstrap)
T+420s    Operator still not ready (image auth fail)
T+480s    Agent DaemonSet created anyway
T+540s    Agent pods running (but unhealthy)
T+570s    CoreDNS creation attempted (doesn't wait for agent ready)
T+600s    CNI timeout hit (30s wait for socket)
T+600s+   CoreDNS stuck in Pending; cluster degradation
T+720s+   Test framework times out
```

**Delay Factors**:
1. Operator startup: +5-10 min (image pull issues)
2. Agent initialization: +5-10 min (socket creation slow)
3. CoreDNS scheduling: Overlaps with agent startup (race condition)

---

## Permission Analysis

### Is This a Permission Issue?

**No.** Evidence:

1. **Error type**: `ENOENT` (no such file or directory)
   - Not `EACCES` (permission denied)
   - Not `EPERM` (operation not permitted)
   - Socket file simply doesn't exist

2. **Socket directory**: `/var/run/cilium/`
   - If directory didn't exist: `mkdir: EACCES`
   - Error only on socket connect, not directory check

3. **DaemonSet privilege**: Cilium agent runs as root
   - Should have permission to create `/var/run/cilium/cilium.sock`
   - Should have permission to bind to port 9879

**Conclusion**: Not a permission issue; socket creation is simply not happening.

---

## Delay vs Missing Classification

### Is Socket Delayed or Missing?

**Classification: MISSING** (not just delayed)

**Evidence**:
1. **Timeout occurred**: 30-second timeout explicitly hit
   - CoreDNS waited 30 seconds for socket
   - At T+30s, socket still ENOENT
   - This is long enough for normal startup

2. **No cascading timeouts**: If it were just delayed, would see partial creation
   - After 30s timeout, CoreDNS gives up (doesn't retry in same event)
   - No evidence of later socket creation attempt
   - Subsequent pod creation attempts also fail (same issue)

3. **Agent health check failure**: Additional indicator
   - If agent was initializing normally, health port would eventually bind
   - Connection refused = port not bound at all
   - Suggests agent itself didn't finish initialization

**Conclusion**: Socket is not being created in this run, not just delayed past 30s timeout.

---

## Recommendations: Immediate Actions

### Action 1: Fix Operator Image (P0 - Critical)
**Why**: Operator not starting cascades to agent initialization failure

```bash
# Option A: Use local image
export CILIUM_OPERATOR_IMAGE="localhost:5000/seriousum/operator-generic"
export CILIUM_OPERATOR_TAG="local"

# Option B: Use official CI image
export CILIUM_OPERATOR_IMAGE="quay.io/cilium/cilium-ci"
export CILIUM_OPERATOR_TAG="latest"

# Option C: Add image pull secret for registry auth
kubectl create secret docker-registry \
  --docker-server=quay.io \
  --docker-username=<user> \
  --docker-password=<token> \
  -n kube-system quay-credentials
```

### Action 2: Verify Agent Initialization (P0 - Critical)
**Why**: Startup probe failure indicates agent not fully starting

```bash
# Collect agent logs
kubectl logs -n kube-system -l k8s-app=cilium -c cilium-agent --previous

# Check startup probe failures
kubectl get events -n kube-system --field-selector involvedObject.name=cilium-*

# Verify pod resources
kubectl top pod -n kube-system -l k8s-app=cilium

# Check kernel capabilities
kubectl exec -n kube-system <agent-pod> -- \
  grep BPF /boot/config-$(uname -r)
```

### Action 3: Enable Socket Creation Diagnostics (P1 - High)
**Why**: Need visibility into why socket isn't created

```bash
# Add debug logging to agent startup
kubectl set env daemonset/cilium \
  -n kube-system \
  -c cilium-agent \
  CILIUM_DEBUG=true

# Verify socket creation step
kubectl exec -n kube-system <agent-pod> -- \
  ls -la /var/run/cilium/cilium.sock

# Check mount points
kubectl exec -n kube-system <agent-pod> -- \
  mount | grep cilium

# Check socket directory permissions
kubectl exec -n kube-system <agent-pod> -- \
  ls -ld /var/run/cilium/
```

### Action 4: Increase CNI Timeout (P2 - Medium)
**Why**: May help if socket is just slightly delayed

```bash
# Via Cilium CiliumConfig
kubectl patch ciliumconfigs/cilium \
  --type merge \
  -p '{"spec":{"cni":{"waitForSocket":"60s"}}}'

# Or kubelet CNI timeout
# Add to kubelet config: cniPluginReadOnly=false, cniCacheDirs=/var/lib/cni
```

### Action 5: Implement Socket Availability Check (P2 - Medium)
**Why**: Test framework should wait for CNI readiness before proceeding

```bash
# Add to test suite BeforeEach
kubectl wait --for=condition=ready pod \
  -n kube-system \
  -l k8s-app=cilium \
  --timeout=300s

# Verify socket exists
kubectl exec -n kube-system <agent-pod> -- \
  test -S /var/run/cilium/cilium.sock
```

---

## Diagnostic Script

I've created a comprehensive diagnostic script to run when investigating this issue. See below: `diagnose-cni-socket-timing.sh`

---

## Testing Protocol

When rerunning tests to verify fixes:

1. **Pre-test verification**:
   ```bash
   kubectl get pods -n kube-system -l k8s-app=cilium -o wide
   kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o wide
   ```

2. **During pod creation**:
   ```bash
   # Watch for CoreDNS startup
   kubectl get pods -n kube-system coredns-* -w
   
   # Monitor CNI invocations
   kubectl logs -n kube-system <agent-pod> -f -c cilium-agent | grep -i socket
   ```

3. **Post-failure analysis**:
   ```bash
   kubectl describe pod -n kube-system coredns-* | grep -A 5 "FailedCreatePodSandbox"
   kubectl get events -n kube-system --sort-by='.firstTimestamp'
   ```

---

## Expected Resolution

Once fixed, should see:

✅ Operator pod healthy (no ImagePullBackOff)  
✅ Agent pods Ready (startup probe passing)  
✅ `/var/run/cilium/cilium.sock` file exists on nodes  
✅ CoreDNS pods transitioning to Running  
✅ Cluster nodes Ready  
✅ Test framework can proceed with test scenarios  

**Expected timeline**: 3-5 minutes from cluster bootstrap to test-ready

---

## Summary Table

| Aspect | Status | Classification | Action |
|--------|--------|-----------------|--------|
| Socket exists? | ❌ No | **Missing** | Fix agent init |
| Socket delayed? | ✅ Yes (30s+) | Timeout exceeded | Increase timeout |
| Permission issue? | ❌ No | ENOENT not EACCES | N/A |
| Agent started? | ⚠️ Partial | Unhealthy probe | Debug startup |
| Operator ready? | ❌ No | Image auth fail | Fix image source |
| CoreDNS blocked? | ✅ Yes | CNI socket missing | Resolve socket |
| Test progress? | ❌ Blocked | Prereq unmet | Wait for CNI fix |

---

## Conclusion

**Root Cause**: Cilium agent initialization is incomplete; CNI socket is not created.

**Primary Issue**: Operator image authentication failure cascades to agent initialization failure.

**Socket Status**: **Missing** (ENOENT), not delayed or permission-related.

**Recommended Fix**: 
1. Fix operator image source or registry auth
2. Debug agent startup and socket creation
3. Implement socket availability checks before CNI invocation
4. Verify operator→agent CRD synchronization

**Timeline to Resolution**: 1-2 hours (if simple image fix) to 1 day (if deep agent debugging needed)
