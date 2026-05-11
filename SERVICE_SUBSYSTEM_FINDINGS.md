# Service Subsystem Initialization - Quick Findings Summary

## Status Code: 🔴 CRITICAL FAILURE
- **Test:** K8sDatapathServicesTest BeforeEach (9 failures)
- **Root Cause:** Multi-layered: CNI socket unavailable → Agent health check fails → Service subsystem never initialized
- **Impact:** Zero service tests can run; cluster bootstrap incomplete

---

## Three-Layer Problem Stack

### Layer 1: Infrastructure 🏗️ (Blocking)
```
Problem:  CNI plugin socket /var/run/cilium/cilium.sock NOT CREATED
Impact:   Kubelet cannot invoke CNI → pods fail to schedule
Status:   All pods stuck in ContainerCreating or Pending
Evidence: "dial unix /var/run/cilium/cilium.sock: connect: no such file or directory"

Why:      Agent pod starts but crashes before creating socket
Why2:     Agent health check endpoint (9879/healthz) not responding
Why3:     Startup probe fails → Kubelet kills pod
```

**Fix Required:** Agent must create CNI socket and health endpoint BEFORE startup probe runs

---

### Layer 2: Agent Initialization 🤖 (Blocking)
```
Problem:  Agent doesn't initialize service subsystems
Status:   Agent binary runs but isn't service-aware
Evidence: No eBPF service programs/maps found, no endpoint tracking

What's Missing:
  ❌ Service observer (watches K8s Service objects)
  ❌ Endpoint lifecycle management (creates/updates pod endpoints)
  ❌ eBPF map population (cilium_lb4_map stays empty)
  ❌ Service eBPF programs (XDP/TC service handler code)
```

**Fix Required:** Implement service initialization in agent startup sequence

---

### Layer 3: Service Datapath 🔀 (Feature)
```
Problem:  Even if agent starts, services don't route traffic
Status:   No load balancing, no service discovery
Evidence: Test failures in BeforeEach when setting up service scenarios

Missing Components:
  ❌ Backend selection algorithm (round-robin, hash, least-conn)
  ❌ Health checking (detect failed backends)
  ❌ Hairpin mode (pod → service it hosts)
  ❌ DSR/IPIP encapsulation (optional modes)
  ❌ Connection tracking (return path steering)
```

**Fix Required:** Implement full service traffic steering with eBPF

---

## What's Initialized vs. Not

### ✅ Working Components
- Cluster bootstrap (kind creates nodes)
- Container runtime (docker/containerd)
- Kubernetes API server
- Upstream Cilium operator (starts but can't manage agent)
- KvStore daemon (state stored but unused)

### ❌ Broken/Missing Components

| Component | What It Should Do | Status |
|-----------|------------------|--------|
| CNI Socket | Allow Kubelet to call CNI | ❌ NOT CREATED |
| Agent Health Check | Signal readiness to K8s | ❌ NOT RESPONDING |
| Service Observer | Watch K8s Services | ❌ NOT IMPLEMENTED |
| Endpoint Manager | Track pod identities | ❌ NOT IMPLEMENTED |
| Service Maps | Store VIP→backend mappings | ❌ NOT LOADED |
| Service Programs | Route traffic to backends | ❌ NOT LOADED |
| Backend Selector | Choose backend per flow | ❌ NOT IMPLEMENTED |

---

## Evidence from RCA Analysis

### Pod Events Timeline
```
T+0:00  Cilium operator deployment attempted
        Status: ImagePullBackOff (401 UNAUTHORIZED from quay.io)
        → Falls back to upstream operator (works)

T+4m:   Cilium agent DaemonSet pods created
        Status: Running (initially)

T+4m:   Startup probe begins: GET http://127.0.0.1:9879/healthz
        Response: Connection refused
        Reason: Agent didn't bind health endpoint

T+8m:   Startup probe failure threshold reached
        Kubelet kills both cilium-gpbd2 and cilium-xd6d6 pods

T+8m:   CoreDNS pod attempts to schedule
        Kubelet calls CNI to create network sandbox
        CNI socket unreachable (agent down)
        Error: Timeout after 30 seconds

T+10m:  Cluster marked NotReady
        All node status checks fail
        Test cannot proceed
```

### Service Test Failures
```
K8sDatapathServicesTest Results:
  Passed:  0
  Failed:  9 (all in BeforeEach)
  Skipped: 41

BeforeEach Failure Reasons (Inferred):
  - Service objects can be created in K8s ✅
  - But agent doesn't observe them ❌
  - CiliumEndpoint resources not created ❌
  - Service load balancing maps not populated ❌
  - Test preconditions never satisfied ❌
  - All 50 scenarios skipped or failed ❌
```

---

## Required eBPF Maps (Currently Missing)

### Service Load Balancing
```
cilium_lb4_map:
  Type:     Hash map (BPF_MAP_TYPE_HASH)
  Key Size: 12 bytes (IP + port + protocol)
  Value:    16 bytes (backend selection info)
  Entries:  65536
  Purpose:  IPv4 service → backend redirect

cilium_lb6_map:
  Type:     Hash map (BPF_MAP_TYPE_HASH)
  Key Size: 32 bytes (IPv6 + port + protocol)
  Value:    16 bytes (backend selection info)
  Entries:  65536
  Purpose:  IPv6 service → backend redirect
```

### Endpoint Tracking
```
cilium_endpoint:
  Type:     Hash map (BPF_MAP_TYPE_HASH)
  Key Size: 4 bytes (endpoint ID or IP)
  Value:    120 bytes (metadata, labels, state)
  Entries:  65536
  Purpose:  Pod identity and metadata lookup

cilium_ipcache:
  Type:     Hash map (BPF_MAP_TYPE_HASH)
  Key Size: Variable (IP address)
  Value:    8 bytes (security identity)
  Entries:  512000
  Purpose:  Fast IP → identity mapping
```

### State Tracking
```
cilium_ct_*:
  Type:     Hash map (BPF_MAP_TYPE_HASH)
  Purpose:  Connection tracking for return traffic
  
cilium_svc_*:
  Type:     Hash map (BPF_MAP_TYPE_HASH)
  Purpose:  Service definition cache
```

---

## eBPF Programs Missing

### XDP (Express Data Path) Layer
```
Location:  Ingress on pod veth interface
Purpose:   Early packet interception for service traffic
Missing:   xdp_entry (intercepts packets to service VIPs)
           xdp_lb_map_lookup (consults cilium_lb4_map)
           xdp_backend_select (chooses backend)
```

### TC (Traffic Control) Layer
```
Location:  Classically after XDP, on pod veth
Purpose:   Secondary filtering, policy enforcement
Missing:   classifier_entry (packet classification)
           classifier_service (service handling)
           classifier_backend (backend forwarding)
           classifier_hairpin (hairpin mode)
```

### Kernel Hook Points
```
Needed Hooks:
  ❌ XDP_INGRESS       (earliest, highest performance)
  ❌ TC_INGRESS        (fallback if no XDP)
  ❌ TC_EGRESS         (return traffic steering)
  ❌ kprobes/uprobes   (connection tracking)
```

---

## Service Subsystem Initialization Checklist

### What Should Happen on Agent Start
- [ ] 1. Parse configuration → service subsystem enabled
- [ ] 2. Mount BPF filesystem → /sys/fs/bpf
- [ ] 3. Compile eBPF programs (service_lb4.o, service_lb6.o)
- [ ] 4. Load programs into kernel (bpf() syscall)
- [ ] 5. Create BPF maps (cilium_lb4_map, etc.)
- [ ] 6. Create CNI socket → /var/run/cilium/cilium.sock
- [ ] 7. Bind health endpoint → 0.0.0.0:9879
- [ ] 8. Start K8s API watcher → observe Service objects
- [ ] 9. Start pod event watcher → create CiliumEndpoint per pod
- [ ] 10. Reconcile services → populate lb4_map with backends
- [ ] 11. Return "Ready" from health check
- [ ] 12. Accept CNI requests on socket

### What's Actually Happening
- [x] 1. Parse configuration (basic config exists)
- [x] 2. Mount BPF filesystem (probably done)
- [ ] 3. Compile eBPF programs ❌
- [ ] 4. Load programs into kernel ❌
- [ ] 5. Create BPF maps ❌
- [ ] 6. Create CNI socket ❌
- [ ] 7. Bind health endpoint ❌
- [ ] 8. Start K8s API watcher ❌
- [ ] 9. Start pod event watcher ❌
- [ ] 10. Reconcile services ❌
- [ ] 11. Return "Ready" from health check ❌
- [ ] 12. Accept CNI requests on socket ❌

**Progress: 2/12 = 17% of service subsystem initialized**

---

## Diagnostic Commands (When Cluster Running)

```bash
# Check if service maps exist
bpftool map list | grep -iE "lb4|lb6|svc"

# Check if service programs loaded
bpftool prog list | grep -iE "service|lb|hairpin"

# Check CiliumEndpoint resources
kubectl get ciliumendpoints -A

# Check service observer logs
kubectl logs -n kube-system -l k8s-app=cilium \
  | grep -iE "service|observer|watch|reconcil"

# Check agent status
kubectl exec -n kube-system cilium-<POD> -- cilium status

# Verify CNI socket exists
ls -la /var/run/cilium/cilium.sock

# Test CNI socket responsiveness
echo "" | nc -U /var/run/cilium/cilium.sock

# Check health endpoint
curl -v http://127.0.0.1:9879/healthz

# List all endpoints
kubectl exec -n kube-system cilium-<POD> -- cilium endpoint list

# List all services
kubectl exec -n kube-system cilium-<POD> -- cilium service list
```

---

## Next Actions (Priority Order)

### Immediate (P0 - Unblock Testing)
1. ✅ **Analyze** - Understand current state (THIS REPORT DONE)
2. ⚠️  **Fix CNI Socket** - Must create socket + health endpoint
3. ⚠️  **Verify Agent Starts** - Agent pod should survive health check

### Short Term (P1 - Enable Service Tests)
4. ⚠️  **Implement Service Observer** - Watch K8s Service objects
5. ⚠️  **Implement Endpoint Manager** - Track pod identities
6. ⚠️  **Load Service eBPF Programs** - Compile and attach
7. ⚠️  **Populate Service Maps** - VIP → backend lookup tables

### Medium Term (P2 - Full Service Support)
8. ⚠️  **Add Backend Selection Algorithm** - Round-robin/hash
9. ⚠️  **Add Health Checking** - Detect failed backends
10. ⚠️ **Add Hairpin Support** - Pod → service it hosts
11. ⚠️ **Add Connection Tracking** - Return traffic steering

---

## References

- **Full Report:** SERVICE_SUBSYSTEM_DEBUG_REPORT.md (21KB, detailed)
- **RCA Analysis:** K8sDatapathServicesTest_RCA.json (in repo)
- **Source Code:** 
  - crates/datapath/src/lib.rs (map definitions)
  - crates/endpoint/src/lib.rs (endpoint model)
  - crates/loadbalancer/src/lib.rs (service model)
- **Cilium Upstream:** /var/home/james/dev/cilium/test/... (test suite)

---

**Report Generated:** 2026-05-11 | **Status:** CRITICAL (0% service tests passing)
