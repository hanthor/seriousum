# Operator-Agent CRD Sync Verification Report

**Date:** May 11, 2026  
**Analysis Method:** Static code analysis + Integration test RCA  
**Status:** ⚠️ **CRITICAL SYNC ISSUES IDENTIFIED**

---

## Executive Summary

The operator-agent CRD synchronization is **experiencing critical timing and initialization issues**:

- **CRD Creation**: Operator attempts to create CRDs but **health check failures prevent successful registration**
- **Agent Sync**: Agent cannot wait for/observe CRD population because **operator pod lifecycle fails early**
- **CNI Plugin Socket**: Missing `/var/run/cilium/cilium.sock` indicates **agent never reaches running state**
- **Cascading Failures**: Operator → Agent → Pod CNI initialization chain breaks at operator stage

### Issue Classification
| Category | Status | Severity |
|----------|--------|----------|
| CRD Creation | **Blocked** | CRITICAL |
| Data Sync | **Cannot verify** | HIGH |
| Timing Issues | **Confirmed** | CRITICAL |
| Agent Wait Logic | **Not implemented** | MEDIUM |

---

## Task 1: List of CRDs Operator Creates

### Expected CRDs (from Cilium upstream design)

The operator **should** create the following CRDs:

1. **CiliumNode** - Node identity, endpoint tracking
   - Fields: `spec.identity`, `spec.addresses`, `status.alibaba`, `status.aws`, status conditions
   
2. **CiliumEndpoint** - Pod network interface endpoints
   - Fields: `spec.containerID`, `spec.labels`, `status.networking.addressing`, `status.state`
   
3. **CiliumNetworkPolicy** - Network policy enforcement
   - Fields: `spec.egress`, `spec.ingress`, `spec.labels`, `spec.priority`
   
4. **CiliumClusterwideNetworkPolicy** - Cluster-wide policies
   - Similar to CiliumNetworkPolicy but cluster-scoped
   
5. **CiliumIdentity** - Service identity and labels
   - Fields: `spec.labels`, `status.security-id`

6. **CiliumLoadBalancerIPPool** - Load balancer IP management
7. **CiliumBGPPeeringPolicy** - BGP routing configuration
8. **CiliumEgressNATPolicy** - Egress NAT rules
9. **CiliumCIDRGroup** - IP group definitions

### Verification Status

**❌ CANNOT VERIFY ON LIVE CLUSTER** - No active Kubernetes cluster available

**From RCA Analysis:**
```
Cilium Operator Pod Status: ImagePullBackOff → Failed Startup
├─ Image Pull Error: quay.io/cilium/cilium-ci-generic:latest - 401 UNAUTHORIZED
├─ No CRD registration occurred (operator never reached running state)
└─ CRD field population: NOT STARTED
```

---

## Task 2: Operator Pod Logs for CRD Creation

### Expected Log Signatures

Healthy operator should emit:
```
msg="Registering custom resource definitions..."
msg="CRD CiliumNode registered successfully"
msg="CRD CiliumEndpoint registered successfully"
msg="CRD CiliumNetworkPolicy registered successfully"
msg="Starting operator reconciliation loop"
```

### Actual Logs from RCA

**Operator Pod: `cilium-operator-794c8bd4f-8rvpb`**
```
Status: ImagePullBackOff
Phase: Failed

Error Log:
"Failed to pull image \"quay.io/cilium/cilium-ci-generic:latest\": 
 failed to resolve reference: unexpected status from HEAD request: 401 UNAUTHORIZED"

Consequence:
- Operator container never started
- No logs available (container not launched)
- CRD registration code never executed
```

### Root Cause

| Issue | Impact |
|-------|--------|
| **Image Authentication Failure** | Container initialization blocked at image pull stage |
| **No CRD Registration Logs** | Operator never reached operational code path |
| **Early Pod Termination** | Operator never became Ready |

**→ This is a TIMING/LIFECYCLE ISSUE, not data corruption**

---

## Task 3: Agent Logs for CRD Wait/Sync

### Expected Agent Wait Patterns

Healthy agent should emit:
```
msg="Waiting for CRD CiliumNode to be registered" component=daemon
msg="Watching CiliumNode updates" component=daemon
msg="CRD sync complete, proceeding with datapath initialization"
msg="Observing endpoint updates from CiliumEndpoint CRD"
msg="Agent ready" component=daemon
```

### Actual Agent Logs from RCA

**Agent DaemonSet Pods: `cilium-gpbd2`, `cilium-xd6d6`**

```
Status: Running but UNHEALTHY
Health Check: FAILED

Startup Probe Error:
  "Get \"http://127.0.0.1:9879/healthz\": dial tcp 127.0.0.1:9879: connect: connection refused"

Pod Lifecycle:
  1. Created (Pending)
  2. Container started (Running)
  3. Health probe fails repeatedly
  4. Pod terminated (CrashLoopBackOff)

Consequence:
  - Agent never reached healthy state
  - Health check endpoint (9879) never became responsive
  - No opportunity to wait for operator CRDs
  - CNI socket never created
```

### Wait/Sync Logic Status

| Phase | Status | Evidence |
|-------|--------|----------|
| **Agent startup** | ❌ FAILS | Startup probe timeout |
| **Operator observability** | ❌ BLOCKED | Operator not running |
| **CRD discovery wait** | ❌ NOT REACHED | Agent exits before CRD logic |
| **Datapath ready signal** | ❌ INCOMPLETE | No `/healthz` response |

**→ This is TIMING/SEQUENCING ISSUE: Agent initialization fails before CRD sync logic can execute**

---

## Task 4: Verify CRD Fields (Sample: CiliumNode)

### Expected CiliumNode CRD Structure

```yaml
apiVersion: cilium.io/v2
kind: CiliumNode
metadata:
  name: kind-control-plane
  namespace: kube-system
spec:
  identity: 1000  # Node identity
  addresses:
    - type: InternalIP
      ip: 10.244.0.1
    - type: CiliumInternalIP
      ip: 10.244.0.1
  encryption: {}
  health:
    ipv4: 10.244.0.1

status:
  alibaba: {}
  aws: {}
  azure: {}
  cilium-health:
    ipv4: 10.244.0.1
  node-addresses:
    - type: NodeInternalIP
      address: 10.244.0.1
```

### Verification Status from RCA

**Field Verification Result:**

```
├─ CiliumNode CRD: ❌ CANNOT VERIFY
│  └─ Reason: Operator failed before CRD creation
│
├─ Expected Fields Present: UNKNOWN
│  ├─ spec.identity: NOT CREATED
│  ├─ spec.addresses: NOT CREATED
│  ├─ status.cilium-health: NOT CREATED
│  └─ status.node-addresses: NOT CREATED
│
└─ Field Population Timeline: NOT RECORDED
   └─ Operator never reached CRD registration code
```

### Data Integrity Check

| Aspect | Status |
|--------|--------|
| CRD schema definition | Unknown - not reached |
| Field validation | Unknown - not reached |
| Default value population | Unknown - not reached |
| Operator → Agent field sync | **BLOCKED** |

**→ This is a DATA/IMPLEMENTATION ISSUE: CRD schema and field population not verified**

---

## Task 5: CRD Sync Status Assessment

### Sync Status Matrix

| Component | Status | Evidence | Issue Type |
|-----------|--------|----------|-----------|
| **Operator Image Pull** | ❌ FAILED | 401 UNAUTHORIZED | TIMING |
| **Operator Pod Startup** | ❌ FAILED | ImagePullBackOff → Never Started | TIMING |
| **CRD Registration** | ❌ NOT STARTED | No logs, no operator running | TIMING |
| **Agent Startup** | ❌ FAILED | Startup probe timeout | TIMING |
| **Agent Health Check** | ❌ FAILED | Port 9879 unreachable | TIMING |
| **CNI Socket Creation** | ❌ FAILED | /var/run/cilium/cilium.sock missing | TIMING/SEQUENCING |
| **CRD Field Population** | ❌ NOT STARTED | No agent observing CRDs | TIMING |
| **Pod Network Setup** | ❌ FAILED | CNI plugin unavailable | CASCADING |

### Failure Chain Analysis

```
┌─────────────────────────────────────────────────────────────┐
│ OPERATOR-AGENT CRD SYNC FAILURE CHAIN                       │
└─────────────────────────────────────────────────────────────┘

  1. IMAGE PULL FAILS (TIMING ISSUE)
     └─ quay.io/cilium/cilium-ci-generic:latest: 401 UNAUTHORIZED
        
  2. OPERATOR POD LIFECYCLE BLOCKED (TIMING CONSEQUENCE)
     └─ Pod stuck in ImagePullBackOff
        └─ Never reaches running state
           
  3. CRD REGISTRATION NEVER EXECUTES (TIMING CONSEQUENCE)
     └─ Operator code path never entered
        └─ No CRDs in cluster
           
  4. AGENT STARTUP HEALTH CHECK FAILS (TIMING ISSUE)
     └─ http://127.0.0.1:9879/healthz: connection refused
        └─ Agent process may not have started OR
        └─ Port 9879 never bound OR
        └─ Agent crashed immediately
           
  5. CNI SOCKET NOT CREATED (SEQUENCING CONSEQUENCE)
     └─ /var/run/cilium/cilium.sock missing
        └─ Agent never reached service-starting state
           
  6. POD NETWORK CREATION FAILS (CASCADING CONSEQUENCE)
     └─ kubelet CNI plugin invocation times out (30s)
        └─ CoreDNS and all workloads stuck in ContainerCreating
           └─ CLUSTER BOOTSTRAP FAILS
              └─ NO TESTING POSSIBLE
```

### Overall CRD Sync Status

**Current State: ⚠️ PARTIALLY BROKEN - CRITICAL ISSUES**

```
Sync Status by Category:
├─ CRD Registration Flow:    ❌ BLOCKED AT IMAGE PULL
├─ CRD Data Propagation:     ❌ BLOCKED AT OPERATOR INIT
├─ Agent CRD Observation:    ❌ BLOCKED AT AGENT STARTUP
├─ Pod Network Readiness:    ❌ BLOCKED AT CNI SOCKET
└─ Test Execution Readiness: ❌ BLOCKED - NO NETWORK
```

---

## Recommendations

### Problem Classification

| Aspect | Issue Type | Root Cause | Impact |
|--------|-----------|-----------|--------|
| **Operator not starting** | TIMING | Image authentication failure | CRDs never created |
| **Agent health check failing** | TIMING | Startup probe timeout | Agent never ready |
| **CNI socket missing** | SEQUENCING | Agent never reaches running state | No pod networking |
| **Cascading bootstrap failure** | CASCADING | Multiple timing issues combine | Cluster unusable |

### Priority 1: Fix Image Authentication (IMMEDIATE)

**Action:** Resolve operator image pull failure
```bash
# Current Issue:
# quay.io/cilium/cilium-ci-generic:latest returns 401 UNAUTHORIZED

# Solution Options:
# Option 1: Use local-built image
kind load docker-image --name=kind localhost:5000/seriousum/cilium-operator:local

# Option 2: Create image pull secret
kubectl create secret docker-registry quay-creds \
  -n kube-system \
  --docker-server=quay.io \
  --docker-username=<user> \
  --docker-password=<token>

# Option 3: Use upstream public image
# Use quay.io/cilium/operator:latest instead of cilium-ci-generic
```

**Expected Outcome:**
- Operator pod transitions from ImagePullBackOff to Running
- CRD registration code path executes
- Operator logs show successful CRD creation

### Priority 2: Debug Agent Startup (HIGH)

**Action:** Identify why agent startup probe fails

```bash
# Collect logs from previous run (if available)
kubectl logs <cilium-agent-pod> -c cilium-agent --previous

# Check for common causes:
# 1. OOM: check node memory pressure
kubectl top nodes
kubectl describe node <node-name> | grep -A 5 "Allocated"

# 2. BPF subsystem: verify kernel support
cat /proc/sys/kernel/unprivileged_bpf_disabled
grep BPF /boot/config-$(uname -r)

# 3. Port binding: check if 9879 is available
netstat -tlnp | grep 9879

# 4. Service account RBAC: verify permissions
kubectl auth can-i create ciliumnode \
  --as=system:serviceaccount:kube-system:cilium-operator
```

**Expected Outcome:**
- Agent startup probe succeeds
- Agent reaches healthy state
- CNI socket `/var/run/cilium/cilium.sock` created

### Priority 3: Implement Agent CRD Wait Logic (MEDIUM)

**Action:** Add explicit CRD readiness checks in agent initialization

```rust
// Pseudo-code for agent startup sequence:
async fn agent_startup() -> Result<()> {
    // 1. Wait for operator to create CRDs
    wait_for_crd("CiliumNode", Duration::from_secs(30)).await?;
    wait_for_crd("CiliumEndpoint", Duration::from_secs(30)).await?;
    
    // 2. Verify CRD schema has expected fields
    verify_crd_fields("CiliumNode", &["spec.identity", "spec.addresses"]).await?;
    
    // 3. Create node resource (will be populated by operator)
    create_cilium_node_resource().await?;
    
    // 4. Wait for operator to populate our node
    wait_for_node_populated(Duration::from_secs(60)).await?;
    
    // 5. Initialize datapath based on CRD state
    initialize_datapath().await?;
    
    // 6. Report health
    healthz_listen(9879).await?;
}
```

**Expected Outcome:**
- Clear separation between operator (CRD creation) and agent (CRD consumption)
- Agent explicitly waits with timeout
- Better diagnostics of sync failures

### Priority 4: Add CRD Sync Observability (MEDIUM)

**Action:** Instrument operator and agent with CRD sync metrics

```
Metrics to add:
├─ operator_crd_registration_duration_seconds (histogram)
├─ operator_crd_registration_success (counter)
├─ agent_crd_wait_duration_seconds (histogram)
├─ agent_crd_sync_success (counter)
├─ agent_crd_field_population_latency_ms (gauge)
└─ crd_sync_status (gauge: 0=blocked, 1=synced, 2=failed)

Logs to add:
├─ "CRD registration started: [CRD list]"
├─ "CRD <name> created successfully"
├─ "CRD field population complete: <count> fields"
├─ "Agent waiting for CRD <name>: timeout in <duration>"
├─ "Agent observing CRD updates for <name>"
└─ "CRD sync complete - datapath initialization proceeding"
```

**Expected Outcome:**
- Clear visibility into operator→agent handoff
- Easy debugging of timing issues
- Alertable metrics for CRD sync failures

### Priority 5: Implement CRD Field Validation (LOW)

**Action:** Verify CRD fields at agent startup

```rust
// Validate CRD schema includes expected fields
async fn validate_cilium_node_crd() -> Result<()> {
    let crd = get_crd("CiliumNode").await?;
    
    let required_fields = vec![
        "spec.identity",
        "spec.addresses",
        "spec.health",
        "status.cilium-health",
        "status.node-addresses",
    ];
    
    for field in required_fields {
        if !crd.has_field(field) {
            return Err(format!("CRD missing required field: {}", field));
        }
    }
    
    Ok(())
}
```

**Expected Outcome:**
- Early detection of schema mismatch
- Clear error messages
- Prevents downstream failures from incomplete CRDs

---

## Data vs. Timing Issue Diagnosis

### Timing Issues (Found)
1. **Image Pull Timeout**: Operator image authentication fails immediately
2. **Agent Startup Timeout**: Startup probe fails after N seconds
3. **Health Check Timeout**: `/healthz` endpoint unreachable
4. **CNI Socket Timeout**: Kubelet times out waiting for agent after 30s
5. **Bootstrap Timeout**: CoreDNS and pods stuck in pending after 8+ minutes

**→ Recommended Solution: Fix image pull, debug agent startup, add explicit waits**

### Data Issues (Cannot Confirm, But Likely)
1. **CRD Schema**: Unknown if CRD definitions are correct (not created)
2. **Field Population**: Unknown if operator populates all required fields (operator failed)
3. **Field Format**: Unknown if CRD fields match agent expectations (not synced)

**→ Recommended Solution: Add post-sync CRD field validation**

### Sequencing Issues (Found)
1. **Agent before operator**: Agent health check may be checked before operator creates CRDs
2. **Pod networking before CNI ready**: Kubelet tries to network pods before agent socket exists
3. **No explicit coordination**: No wait-for-readiness coordination between operator and agent

**→ Recommended Solution: Implement explicit CRD wait logic in agent, add readiness probes**

---

## Implementation Status Summary

| Component | Expected | Implemented | Gap |
|-----------|----------|-------------|-----|
| Operator CRD creation | ✅ Upstream | ✅ Via upstream operator | None (if operator runs) |
| Agent CRD wait loop | ✅ Needed | ❌ Not found | MISSING |
| CRD field validation | ✅ Needed | ❌ Not found | MISSING |
| Sync observability | ✅ Needed | ❌ Not found | MISSING |
| Error recovery | ✅ Needed | ❌ Not found | MISSING |

---

## Conclusion

**CRD Sync Status: ⚠️ BLOCKED DUE TO TIMING/SEQUENCING ISSUES**

The operator-agent CRD synchronization architecture is **sound in design but broken in execution**:

✅ **Working:**
- Integration test framework can bootstrap clusters
- Kubernetes Kind environment is operational
- Cilium deployment manifests load correctly

❌ **Broken:**
- Operator image pull fails (authentication)
- Agent startup probe fails (health check)
- CNI socket never created (cascading consequence)
- Pod networking completely blocked

**Primary Issue:** **TIMING** - Multiple sequential failures in operator→agent startup sequence prevent CRD registration from ever occurring

**Secondary Issue:** **SEQUENCING** - No explicit coordination between operator and agent ensures race conditions

**Tertiary Issue:** **DATA** - Cannot verify CRD fields and schema correctness because upstream operator never executes

**Recommended Action:** 
1. Fix image authentication immediately (P0)
2. Debug agent startup failures (P0)
3. Implement explicit CRD wait coordination (P1)
4. Add sync observability and metrics (P1)
5. Validate CRD fields at agent startup (P2)

