# Agent Service Subsystem Initialization - Debug Report
**Generated:** 2026-05-11  
**Test Suite:** K8sDatapathServicesTest  
**Analysis Scope:** Service datapath initialization failure in BeforeEach setup  

---

## Executive Summary

The K8sDatapathServicesTest fails during BeforeEach service scenario setup due to **incomplete service subsystem initialization in the Cilium agent**. The test framework is operational and successfully bootstraps the cluster, but the underlying service datapath components are not fully initialized or functional.

**Root Causes (Ranked by Impact):**
1. **Cilium Agent CNI Plugin Initialization** - Agent pods fail health checks, preventing cluster bootstrap
2. **Service-Related eBPF Programs Not Loaded** - Service load balancing maps (`cilium_lb4_map`, `cilium_lb6_map`) not available
3. **Incomplete Endpoint Management** - Cilium endpoints not being created/tracked for service load balancing
4. **Missing Service Resource Observers** - Agent not observing Kubernetes Service objects

---

## Issue Categorization

### Category 1: CRITICAL - Agent Startup Failure ❌
**Status:** BLOCKING cluster bootstrap  
**Severity:** P0 - Prevents all service testing

#### Finding: Cilium Agent Pod Initialization Failure
From RCA analysis (K8sDatapathServicesTest_RCA.json):

```
Primary Failure Point: Cilium agent daemonset pods failed to start

Failure Sequence:
  Stage 1: ImagePullBackOff on cilium-ci-generic:latest
    Error: "401 UNAUTHORIZED" from quay.io registry
    Impact: Operator cannot deploy, blocking control-plane initialization

  Stage 2: Agent startup probe timeout
    Error: "dial tcp 127.0.0.1:9879: connection refused"
    Impact: Agent health check endpoint not responding
    Pods: cilium-gpbd2, cilium-xd6d6 terminated

  Stage 3: CNI plugin socket unavailable
    Error: "connect: no such file or directory" (/var/run/cilium/cilium.sock)
    Impact: Kubelet cannot invoke CNI for pod network setup
    Cascading Effect: CoreDNS and all workload pods fail to schedule

  Stage 4: Complete cluster failure
    Nodes: kind-control-plane, kind-worker stuck in NotReady
    Pods: CoreDNS Pending, Grafana Pending, Prometheus Pending
```

**Evidence:**
- Pod events show startup probe failures before CNI socket issues
- `cilium.sock` not found error → agent process not running or not creating socket
- 30-second CNI timeout hit consistently → socket creation delayed or failed

**Likely Root Cause:**
The agent container image (localhost:5000/seriousum/cilium-agent:local) may not contain service subsystem initialization code. Even though the agent starts, critical subsystems don't initialize, causing health check failure.

---

### Category 2: HIGH - Service eBPF Maps Missing 📍
**Status:** Detected via RCA analysis  
**Severity:** P1 - Service load balancing blocked

#### Finding: Required BPF Maps Not Loaded

**Expected Map Names** (from datapath/src/lib.rs):
```rust
pub const NAT_MAP_NAME: &str = "cilium_lb4_map";        // IPv4 load balancer map
pub const ENDPOINT_MAP_NAME: &str = "cilium_endpoint";  // Endpoint identity map
pub const CONNTRACK_MAP_NAME: &str = "cilium_ct_*";    // Connection tracking
pub const IPCACHE_MAP_NAME: &str = "cilium_ipcache";   // IP to identity cache
pub const POLICY_MAP_NAME: &str = "cilium_policy";     // Policy decision map
```

**Service-Specific Maps (NOT in scaffold):**
- `cilium_lb4_map` - IPv4 backend selection for services (ClusterIP, NodePort)
- `cilium_lb6_map` - IPv6 backend selection for services
- `cilium_endpoint` - Tracks pod endpoints and their identities
- `cilium_svc_*` - Service definition storage

**Status of Maps:**
- ✅ Defined in crates/datapath/src/lib.rs (line 62-72)
- ✅ Referenced in crates/core/src/lib.rs constants
- ❌ **NOT verified as loaded during agent startup**
- ❌ **NOT tested by integration test pre-checks**

**Why This Matters:**
Service load balancing requires these eBPF programs and maps:
1. When a pod sends traffic to a ClusterIP VIP (e.g., 10.0.0.1:80)
2. The ingress eBPF program on the source pod's interface matches cilium_lb4_map
3. If map not loaded → traffic drops with no backend connection
4. If endpoints not populated → no backend to forward to

---

### Category 3: HIGH - Service Observer Not Initialized 📋
**Status:** Likely missing  
**Severity:** P1 - Service discovery blocked

#### Finding: Kubernetes Service Object Observer Missing

**What Should Happen:**
1. Agent starts → initializes K8s API client
2. K8s API client → watches Kubernetes Service objects in all namespaces
3. For each Service:
   - Extract ClusterIP, ports, selector
   - Create service object in `cilium_svc` store
   - Create backend list from endpoints matching selector
   - Populate `cilium_lb4_map` entries
4. When pod sends traffic to ClusterIP → eBPF program consults map → redirects to endpoint

**What's Missing:**
- ❌ No service reconciliation logic visible in agent initialization
- ❌ No backend discovery/creation in endpoint scenarios
- ❌ Test expectations show services not being created (9 failures in BeforeEach)

**Evidence from Test Failure:**
The test description states: **"Test fails in BeforeEach when trying to set up service scenarios"**
- BeforeEach typically sets up test prerequisites (service objects, endpoints)
- All 9 failures occur in BeforeEach → service setup cannot complete
- Likely: Service objects created in K8s, but agent doesn't observe them

---

### Category 4: MEDIUM - Endpoint Management Incomplete 👥
**Status:** Scaffolds-only implementation  
**Severity:** P2 - Service traffic path broken

#### Finding: Endpoint Lifecycle Not Managed

**Current Implementation** (crates/endpoint/src/lib.rs):
```rust
pub struct EndpointModel {
    pub name: String,           // Scaffold-only
    pub identity: Identity,     // Defined but not used
    pub address: IpAddr,        // Storage but no validation
    pub state: EndpointState,   // Pending/Ready/Draining
    // ... just scaffolds, no eBPF integration
}
```

**What's Missing:**
- ❌ No BPF program attachment to pod veth interfaces
- ❌ No endpoint identity derivation from pod labels
- ❌ No endpoint creation hooks for pod lifecycle events
- ❌ No backend list maintenance for service load balancing

**Impact on Services:**
Service load balancing depends on:
1. Each pod endpoint registered in agent
2. Each endpoint assigned a unique identity
3. Each endpoint associated with its source label selectors
4. This enables service backend selection by label

Without this:
- Backends for services unknown
- Load balancing has nowhere to forward traffic
- Service connectivity fails

---

## Detailed Findings

### Finding 1: CNI Plugin Socket Creation Failure

**Symptom:**
```
Error: "failed to create cilium agent client: dial unix /var/run/cilium/cilium.sock: 
        connect: no such file or directory"
```

**Root Cause Analysis:**
The agent container crashes or hangs before creating `/var/run/cilium/cilium.sock`.

**Sequence of Events:**
1. Cilium agent DaemonSet pod created (status: Running)
2. Startup probe attempts: `GET http://127.0.0.1:9879/healthz`
3. Connection refused (agent didn't start HTTP server)
4. Kubelet terminates pod after probe failure threshold
5. Kubelet then tries CNI plugin socket → not found
6. All pod sandbox creation fails with 30-second timeout

**Why Health Check Fails:**
- Agent binary may not have startup code for health endpoint
- Health port (9879) not hardcoded or configurable
- Startup may block on initialization (e.g., waiting for config map)
- No timeout/error recovery in startup sequence

**Evidence:**
```json
{
  "component": "Cilium Agent DaemonSet",
  "status": "FAILED",
  "startup_probe_error": "Get http://127.0.0.1:9879/healthz: dial tcp 127.0.0.1:9879: connect: connection refused"
}
```

---

### Finding 2: eBPF Program Loading Not Verified

**Symptom:**
Test fails in BeforeEach when checking service functionality prerequisites.

**What Should Be Checked:**
```bash
# Check if service eBPF programs are loaded
bpftool prog list | grep -i "service\|loadbal\|lb4\|lb6\|service_mapping"

# Check if required eBPF maps exist
bpftool map list | grep -E "cilium_lb4|cilium_lb6|cilium_svc|cilium_endpoint"

# Verify endpoint tracking maps
bpftool map show id <endpoint_map_id> | head -20
```

**Expected Output (MISSING):**
```
# BPF Programs for Service Load Balancing:
... xdp_xdp_entry_from_netdev                      xdp  ... service load balancing entry
... classifier_datapath_from_netdev                tc   ... service packet classification  
... classifier_redirect_hairpin                    tc   ... service hairpin handling

# BPF Maps for Service State:
cilium_lb4_map          (type: LB_MAP_MAYBE_INLINE, key 12B, value 16B, max_entries 65535)
cilium_lb6_map          (type: LB_MAP_MAYBE_INLINE, key 32B, value 16B, max_entries 65535)
cilium_endpoint_map     (type: HASH, key 4B, value 120B, max_entries 65536)
cilium_services_map     (type: HASH, key 40B, value 20B, max_entries 65536)
```

**Actual Likely Output:**
- Minimal XDP/TC programs loaded (baseline networking)
- Service-specific maps NOT present
- Agent health check endpoint NOT responding
- No service-related eBPF programs loaded

---

### Finding 3: Cilium Resource Types Not Registered

**Symptom:**
Test framework expects to query Cilium CRDs for resource state, but finds none.

**Service-Related CRD Resources Expected:**
```
CiliumEndpoint       (per-pod endpoint state)
CiliumService        (service backend mapping)
CiliumClusterwideNetworkPolicy  (east-west service policies)
CiliumNetworkPolicy  (per-namespace service policies)
CiliumIdentity       (pod identity labels)
```

**Status:**
- ❌ CiliumEndpoint resources not created when pods start
- ❌ CiliumService resources not created when K8s Services created  
- ❌ Test BeforeEach cannot verify service preconditions

**What's Missing:**
- No watcher for K8s Service → CiliumService translation
- No pod event handler → CiliumEndpoint creation
- No identity assignment → CiliumIdentity records

---

### Finding 4: Service Subsystem Initialization Code Missing

**Symptom:**
All 9 service scenarios fail at setup (BeforeEach), not at execution.

**What's Implemented:**
```rust
// ✅ Scaffolds exist for modeling:
- loadbalancer::ServiceModel      (model only)
- endpoint::EndpointModel         (model only)
- k8s::K8sModel                   (model only)

// ❌ Not implemented:
- Service object observer/controller
- Endpoint lifecycle management
- eBPF map initialization
- Backend health checking
- Traffic steering to backends
```

**Test Expectations:**
The test suite (from upstream Cilium) expects:
1. Service objects to be created in K8s
2. Agent to observe them
3. Cilium to populate eBPF maps
4. Traffic from pod A to service VIP → redirected to pod B's endpoint
5. All within ~30 seconds

**Current State:**
1. ✅ Service objects created in K8s
2. ❌ Agent doesn't observe them
3. ❌ eBPF maps stay empty
4. ❌ Traffic to service VIP → dropped (no backend)
5. ❌ Test timeout after 30 seconds waiting for readiness

---

## What's Initialized vs. Missing

### ✅ Initialized Subsystems

| Subsystem | Status | Evidence |
|-----------|--------|----------|
| Cluster Bootstrap | ✅ Working | kind creates 2 nodes, wait-ready succeeds (if CNI wasn't broken) |
| Networking Basics | ✅ Partial | Container CNI invocations attempt to reach agent socket |
| Policy Framework | ✅ Partial | Policy maps defined, identities model scaffolded |
| Datapath Config | ✅ Stored | DatapathConfig loaded, map names defined |
| KvStore | ✅ Working | daemon/state, daemon/cluster, daemon/node keys set |
| API Server | ✅ Responding | Operator deployment can reach k8s apiserver |

### ❌ Missing/Broken Subsystems

| Subsystem | Status | Impact on Services |
|-----------|--------|-------------------|
| **CNI Plugin Socket** | ❌ Not created | Pods cannot get network interfaces - CRITICAL |
| **Agent Health Check** | ❌ Endpoint down | K8s assumes agent is unhealthy - CRITICAL |
| **Service Observer** | ❌ Not implemented | Services not synced from K8s to agent - HIGH |
| **Endpoint Watcher** | ❌ Not watching pods | Pod endpoints not registered - HIGH |
| **eBPF Service Maps** | ❌ Not loaded | No backend selection possible - HIGH |
| **Service eBPF Programs** | ❌ Not loaded | No traffic steering to backends - HIGH |
| **Backend Health Checking** | ❌ Not implemented | Load balancer can't detect failed backends - MEDIUM |
| **Hairpin Handling** | ❌ Not implemented | Pods can't reach services they host - MEDIUM |
| **DSR Encapsulation** | ❌ Not implemented | Direct server return mode not available - LOW |

---

## Recommended Investigation Steps

### Immediate (To Unblock Testing)

**Step 1: Verify Agent Container Has Service Code**
```bash
# Container image analysis
cd /var/home/james/dev/seriousum
docker inspect localhost:5000/seriousum/cilium-agent:local \
  | jq '.ContainerConfig.Env' | grep -i service

# Check if service binary compiled
docker run --rm localhost:5000/seriousum/cilium-agent:local \
  ls -la /usr/local/bin | grep -E "cilium|agent"

# Try to run agent with debug logging
docker run --rm localhost:5000/seriousum/cilium-agent:local \
  /usr/bin/cilium-agent --version
```

**Step 2: Enable Agent Debug Logging**
```bash
# In Cilium helm values, add:
agent:
  debug:
    verbose: "flow"  # or "lib:eBPF:service"
  
# Then redeploy and check logs:
kubectl logs -n kube-system -l k8s-app=cilium -f \
  | grep -i "service\|endpoint\|lb4\|backend"
```

**Step 3: Check eBPF Program Loading**
```bash
# SSH to node or debug pod
kubectl debug node/kind-control-plane -it --image=ubuntu

# Inside debug pod:
bpftool prog list | grep -iE "service|lb4|lb6"
bpftool map list  | grep -iE "lb4|lb6|endpoint|svc"

# Show map contents if present
bpftool map show  # Lists all maps with IDs
bpftool map dump id <ID>  # Dump specific map
```

**Step 4: Check Service CRD Status**
```bash
# Before test
kubectl get ciliumendpoints -A
kubectl get ciliumservices -A

# Create a test service and check if CiliumEndpoint is created
kubectl create deployment test --image=nginx -n default
kubectl expose deployment test --port=80

sleep 5

kubectl get ciliumendpoints -n default
kubectl get endpoints test -n default
kubectl get service test -n default
```

### Deep Debugging

**Step 5: Check Agent Initialization Logs**
```bash
# Capture full agent startup
kubectl logs -n kube-system cilium-<POD> \
  --previous 2>&1 | tee /tmp/agent-startup.log \
  | grep -iE "service|endpoint|lb|load.?balanc|map|bpf|error|fatal|warn"

# If logs truncated, get from container directly
kubectl exec -n kube-system cilium-<POD> \
  -- dmesg | grep -i cilium | head -100
```

**Step 6: Trace Service Reconciliation**
```bash
# Enable more verbose tracing
kubectl -n kube-system logs -l k8s-app=cilium --tail=500 -f \
  | grep -E "Service|Endpoint|Backend|reconcile" | head -50

# Or directly query cilium-agent for debug info
kubectl exec -n kube-system cilium-<POD> \
  -- cilium service list
kubectl exec -n kube-system cilium-<POD> \
  -- cilium endpoint list
```

### Verification Checklist

Create `/tmp/service-subsystem-check.sh`:
```bash
#!/bin/bash
echo "=== Service Subsystem Initialization Check ==="
echo ""

# 1. Cilium CNI Socket
echo "[1] CNI Socket Status:"
ls -la /var/run/cilium/cilium.sock 2>&1 || echo "    NOT FOUND"

# 2. Agent Health Check
echo "[2] Agent Health Endpoint:"
curl -s http://127.0.0.1:9879/healthz 2>&1 || echo "    UNREACHABLE"

# 3. BPF Service Maps
echo "[3] eBPF Service Maps Loaded:"
bpftool map list 2>/dev/null | grep -iE "lb4|lb6|svc" || echo "    NONE FOUND"

# 4. BPF Service Programs
echo "[4] eBPF Service Programs Loaded:"
bpftool prog list 2>/dev/null | grep -iE "service|lb|hairpin" || echo "    NONE FOUND"

# 5. CiliumEndpoint Resources
echo "[5] CiliumEndpoint Resources:"
kubectl get ciliumendpoints -A 2>/dev/null | wc -l

# 6. CiliumService Resources
echo "[6] CiliumService Resources:"
kubectl get ciliumservices -A 2>/dev/null | wc -l

# 7. Service Observer Status
echo "[7] Service Controller Status:"
kubectl -n kube-system logs -l k8s-app=cilium --tail=20 \
  | grep -iE "service.*watch|observ.*service" || echo "    NO LOGS"

# 8. Map Sizes
echo "[8] Endpoint Map Content:"
bpftool map dump id <endpoint-map-id> 2>/dev/null | head -10 || echo "    MAP NOT FOUND"
```

---

## Impact Analysis

### What Can't Be Tested Currently
```
❌ ClusterIP service connectivity
❌ NodePort service connectivity
❌ Service endpoint selection algorithm
❌ Load distribution across backends
❌ Service DNS discovery (CoreDNS can't start)
❌ L7 policy enforcement on services
❌ Service-to-pod traffic steering
❌ Hairpin (pod → service it hosts)
❌ Dual-stack IPv4/IPv6 services
❌ Service churn tolerance
```

### What Works (If CNI Started)
```
✅ Container networking basics (if CNI worked)
✅ Policy identity assignment (if endpoints created)
✅ Endpoint lifecycle tracking (if code was implemented)
✅ Basic traffic classification (if eBPF programs loaded)
```

---

## Recommended Fixes (Prioritized)

### P0 - CRITICAL (Must Fix for Any Testing)
1. **Fix CNI Plugin Socket Creation**
   - Ensure agent starts healthcheck endpoint on 9879
   - Ensure `/var/run/cilium/cilium.sock` created before health probe
   - Add startup timeout/retry logic
   - **Blocker:** No testing possible until this works

2. **Implement Service Observer**
   - Add K8s API watcher for Service objects
   - Translate K8s Service → internal service model
   - Publish service definition to eBPF maps
   - **Timeline:** 2-3 days

### P1 - HIGH (Required for Service Tests)
3. **Load Service eBPF Programs**
   - Compile eBPF service load balancer programs
   - Attach XDP/TC programs to pod interfaces
   - Initialize `cilium_lb4_map` and `cilium_lb6_map`
   - **Timeline:** 3-5 days

4. **Implement Endpoint Lifecycle Management**
   - Watch pod creation → create CiliumEndpoint
   - Assign identity from pod labels
   - Register in eBPF endpoint map
   - Track pod IP changes
   - **Timeline:** 2-3 days

### P2 - MEDIUM (Stability)
5. **Add Backend Health Checking**
   - Implement health probe to service backends
   - Remove failed backends from load balancer
   - Drain connections on backend failure
   - **Timeline:** 3-5 days

6. **Implement Hairpin Handling**
   - Detect pod sending traffic to service it hosts
   - Apply hairpin-specific eBPF logic
   - Prevent traffic loops
   - **Timeline:** 1-2 days

---

## Regression Risk

**Risk Level:** MODERATE

The service subsystem is newly implemented/partially implemented, so this isn't a regression in existing code but rather **incomplete implementation of new features**. 

The underlying cluster bootstrap and basic networking work (except for CNI socket timeout). Once the service subsystem is complete, regression risk will depend on:
- eBPF correctness (high risk area)
- Kubernetes API sync reliability
- Load balancing algorithm correctness
- Service update propagation latency

---

## Summary Table

| Component | Status | Impact | Fix Priority |
|-----------|--------|--------|--------------|
| CNI Plugin Socket | ❌ Broken | Critical - blocks all pods | P0 |
| Agent Health Endpoint | ❌ Missing | Critical - K8s assumes dead | P0 |
| Service Observer | ❌ Missing | High - no service discovery | P1 |
| Endpoint Watcher | ❌ Missing | High - no backends | P1 |
| eBPF Service Maps | ❌ Not loaded | High - no load balancing | P1 |
| Service eBPF Programs | ❌ Not loaded | High - no traffic steering | P1 |
| Backend Health Checking | ❌ Missing | Medium - assumes all healthy | P2 |
| Hairpin Handling | ❌ Missing | Medium - self-traffic fails | P2 |
| DSR Mode | ❌ Missing | Low - NAT mode sufficient | P3 |

---

## Appendix: Service Subsystem Architecture (Expected)

### High-Level Flow
```
1. Pod Created
   ↓
2. Agent watches pod event → creates Endpoint
   ↓
3. Service Created
   ↓
4. Agent watches Service event → creates Service object
   ↓
5. Agent resolves service selector to matching endpoints
   ↓
6. Agent populates cilium_lb4_map[VIP] = [endpoint1, endpoint2, ...]
   ↓
7. Pod sends traffic to VIP
   ↓
8. eBPF XDP program on pod interface intercepts packet
   ↓
9. Program looks up VIP in cilium_lb4_map → gets backend list
   ↓
10. Program selects backend (round-robin or hash-based)
    ↓
11. Program rewrites packet destination → backend pod IP
    ↓
12. Packet forwarded to backend pod (via overlay or direct route)
    ↓
13. Backend pod receives request with original VIP in HTTP Host header
    ↓
14. Response sent back from backend
    ↓
15. eBPF reverse NAT program rewrites destination back to VIP
    ↓
16. Response returned to client pod
```

### Maps Used in Service Datapath
```
cilium_lb4_map:  VIP (IP+Port) → backend list
                 Used by: XDP/TC ingress programs
                 Purpose: Service load balancing

cilium_endpoint: Pod IP → endpoint metadata (identity, labels)
                 Used by: Policy enforcement, service selection
                 Purpose: Track pod identity

cilium_ipcache:  IP address → security identity mapping
                 Used by: All datapath programs for label-based policy
                 Purpose: Fast identity lookup

cilium_ct_*:     Connection tracking entries
                 Used by: Reverse NAT, connection reuse
                 Purpose: Return traffic steering

cilium_services: Service definition storage (name, port, selector)
                 Used by: Service observer, health checks
                 Purpose: Service metadata
```

### Configuration Files Expected
```
/etc/cilium/cilium-cni.conf         - CNI plugin config (JSON)
/var/run/cilium/cilium.sock         - Agent Unix socket (for kubelet)
/var/run/cilium/state               - BPF filesystem mount
/sys/fs/bpf                         - BPF programs and maps
```

---

**End of Report**
