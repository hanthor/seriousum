# Operator Binary Mismatch Fix

## Summary

Fixed a critical deployment issue where the Cilium operator failed to start due to a binary name mismatch between the Dockerfile build configuration and the container's entry point expectation.

---

## Root Cause: Binary Name Mismatch

### The Problem

The Dockerfile was copying the built operator binary to the container with the name `operator`, but the container was configured to execute `cilium-operator-generic` as the entry point.

**Dockerfile (Before):**
```dockerfile
COPY --from=builder /go/bin/operator /usr/local/bin/operator
ENTRYPOINT ["/usr/local/bin/operator"]
```

**Container Startup:**
The container's init process expected to find and execute `/usr/local/bin/cilium-operator-generic`, which did not exist, causing an immediate crash.

### Why This Happened

- The binary build output name (`operator`) did not match the expected container runtime name (`cilium-operator-generic`)
- This mismatch likely resulted from configuration drift between the build system and the deployment expectations
- No validation mechanism caught the binary name during the build process

---

## The Fix Applied

### Dockerfile Changes

**Updated COPY instruction:**
```dockerfile
COPY --from=builder /go/bin/operator /usr/local/bin/cilium-operator-generic
```

**Updated ENTRYPOINT:**
```dockerfile
ENTRYPOINT ["/usr/local/bin/cilium-operator-generic"]
```

### What Changed

1. Binary now copied to the expected filename: `cilium-operator-generic`
2. ENTRYPOINT updated to reference the correctly named binary
3. Container can now find and execute the binary on startup

---

## Cascading Failure: How the Outage Propagated

The operator binary mismatch created a chain reaction of failures throughout the Cilium deployment:

### Stage 1: Operator CrashLoopBackOff
```
Pod: cilium-operator-*
Status: CrashLoopBackOff
Reason: Binary not found - container fails immediately on startup
```
- The operator pod repeatedly crashes and restarts
- Each attempt fails because the expected binary doesn't exist

### Stage 2: CRDs Not Registered
```
Status: CRDs not applied
Reason: Operator cannot run to register Custom Resource Definitions
```
- The operator is responsible for registering Cilium's Custom Resource Definitions (CRDs)
- With the operator crashing, CRDs are never installed into the cluster
- The cluster has no knowledge of Cilium's resources (CiliumNode, CiliumPolicy, etc.)

### Stage 3: Agent Pods Waiting
```
Pod: cilium-*
Status: Pending or CrashLoopBackOff
Reason: CRDs not registered, agent cannot initialize
```
- Cilium agent pods depend on CRDs being available
- Without CRDs, agent pods cannot start their initialization process
- They either hang in Pending state or crash trying to access undefined resources

### Stage 4: cilium.sock Missing
```
Path: /run/cilium/cilium.sock
Status: Not created
Reason: Agent pods never fully initialized
```
- The Cilium agent is responsible for creating the `cilium.sock` Unix socket
- This socket is the control plane for Cilium networking functionality
- Without it, no networking components can communicate with Cilium

### Stage 5: CoreDNS and Other Services Fail
```
Pod: coredns-*
Status: CrashLoopBackOff
Reason: Cannot resolve network - cilium.sock missing
```
- CoreDNS and other services depend on Cilium's networking layer
- Without `cilium.sock`, DNS resolution fails
- Services cannot start or communicate
- Full cluster networking breakdown

### Dependency Chain
```
operator binary missing
    ↓
operator CrashLoopBackOff
    ↓
CRDs not registered
    ↓
agent pods cannot initialize
    ↓
cilium.sock not created
    ↓
coredns fails to start
    ↓
cluster DNS broken
    ↓
pod-to-pod communication fails
```

---

## Post-Fix Validation: What to Watch For

After applying the fix, monitor the following sequence to confirm the operator and networking stack properly recover:

### 1. Operator Pod Starts Successfully ✓
```bash
kubectl get pod -n kube-system -l app.kubernetes.io/name=cilium-operator

# Expected:
# NAME                               READY   STATUS    RESTARTS   AGE
# cilium-operator-generic-abcd1234   1/1     Running   0          2m
```
**What to look for:**
- Status transitions from `CrashLoopBackOff` to `Running`
- READY column shows `1/1`
- RESTARTS stays at `0` or stabilizes

### 2. CRDs Successfully Registered ✓
```bash
kubectl get crd | grep cilium

# Expected:
# ciliumnodes.cilium.io                         2026-05-11T...   True
# ciliumnetworkpolicies.cilium.io               2026-05-11T...   True
# ciliumclusterwidenetworkpolicies.cilium.io   2026-05-11T...   True
```
**What to look for:**
- All Cilium CRDs appear in the output
- Each CRD shows `True` in the ESTABLISHED column
- CRDs are timestamped at or after the operator fix deployment time

### 3. Cilium Agent Pods Initialize ✓
```bash
kubectl get pod -n kube-system -l app=cilium

# Expected:
# cilium-abcd1    1/1     Running   0          3m
# cilium-efgh2    1/1     Running   0          3m
# cilium-ijkl3    1/1     Running   0          2m
```
**What to look for:**
- Agent pods transition from `Pending` or `CrashLoopBackOff` to `Running`
- All pods show `1/1` in READY column
- Startup time stabilizes (no continuous restarts)

### 4. Cilium Socket Created and Accessible ✓
```bash
# On any node:
ls -la /run/cilium/cilium.sock

# Expected:
# srw-rw----  1 root root 0 May 11 14:23 /run/cilium/cilium.sock
```
**What to look for:**
- Socket file exists on all nodes
- Permissions allow pod communication (typically `srw-rw----`)
- No permission denied errors when accessing

### 5. CoreDNS and DNS Resolution Working ✓
```bash
kubectl get pod -n kube-system -l k8s-app=kube-dns

# Expected:
# coredns-abcd1    1/1     Running   0          2m
# coredns-efgh2    1/1     Running   0          2m
```
**Then verify DNS:**
```bash
kubectl run -it --rm debug --image=alpine -- nslookup kubernetes.default
```
**What to look for:**
- CoreDNS pods in `Running` status
- No `CrashLoopBackOff` or restart loops
- DNS queries resolve successfully
- No "connection refused" or "socket not found" errors

### 6. Network Connectivity Verified ✓
```bash
# Test pod-to-pod communication:
kubectl run -it --rm test1 --image=alpine -- sh
kubectl run -it --rm test2 --image=alpine -- sh

# Inside test1:
ping test2
nslookup test2
```
**What to look for:**
- Pods can reach each other by IP and hostname
- No timeout errors
- Response times are normal (< 1 second typically)

---

## Monitoring Checklist

| Component | Before Fix | After Fix | Check By |
|-----------|-----------|-----------|----------|
| Operator Pod | CrashLoopBackOff | Running | `kubectl get pod cilium-operator-*` |
| CRDs | Not registered | All registered | `kubectl get crd \| grep cilium` |
| Agent Pods | Pending/CrashLoopBackOff | Running | `kubectl get pod -l app=cilium` |
| cilium.sock | Missing | Exists on all nodes | `ls /run/cilium/cilium.sock` |
| CoreDNS | CrashLoopBackOff | Running | `kubectl get pod -l k8s-app=kube-dns` |
| Pod-to-Pod DNS | Broken | Working | `nslookup` from pod |

---

## Recovery Timeline

Typical progression after fix deployment:

1. **T+30s**: Operator pod starts running
2. **T+1m**: CRDs appear in cluster
3. **T+1-2m**: Agent pods transition to Running
4. **T+2-3m**: cilium.sock appears on all nodes
5. **T+3-4m**: CoreDNS and services stabilize
6. **T+5m**: Full cluster networking operational

*Times are approximate and may vary based on cluster size and resource availability.*

---

## Prevention

To prevent similar issues in the future:

1. **Binary naming convention**: Define and enforce a standard naming convention for operator binaries
2. **Build validation**: Add a post-build step to verify binary names match expected names
3. **Container image tests**: Test container startup and verify binary execution succeeds
4. **CI/CD gates**: Include image validation in the CI/CD pipeline before pushing to registry
5. **Configuration documentation**: Maintain clear documentation of expected binary paths and names

---

## Related Issues

- Operator binary name mismatch in Dockerfile
- Missing validation for binary names during container build
- Cascading failure propagation through CNI dependency chain

---

## References

- [Cilium Operator Documentation](https://docs.cilium.io/en/latest/operations/concepts/networking/container-network-interface/)
- [Kubernetes CRD Management](https://kubernetes.io/docs/tasks/extend-kubernetes/custom-resources/custom-resource-definitions/)
- [Container Networking Fundamentals](https://www.digitalocean.com/community/tutorials/the-docker-ecosystem-an-overview-of-containerization)
