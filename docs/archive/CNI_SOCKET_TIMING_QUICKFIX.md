# CNI Socket Timing - Quick Action Plan

**Use this when the cluster is running to verify findings and implement fixes.**

---

## Step-by-Step Troubleshooting

### Step 1: Confirm the Issue (30 seconds)

```bash
# Check if CoreDNS is stuck in Pending
kubectl get pods -n kube-system -l k8s-app=kube-dns

# Expected output if issue exists:
# NAME            READY   STATUS    RESTARTS   AGE
# coredns-xxx     0/1     Pending   0          10m
#                 ↑↑↑ 0/1 and Pending = waiting for CNI

# Check the error message
kubectl describe pod -n kube-system <coredns-pod> | grep -A 5 "FailedCreatePodSandbox"

# Expected: "dial unix /var/run/cilium/cilium.sock: connect: no such file or directory"
```

✅ **If you see these outputs, the issue is confirmed.**

---

### Step 2: Check Agent Health (30 seconds)

```bash
# Check agent pod status
kubectl get pods -n kube-system -l k8s-app=cilium -o wide

# Expected output if issue exists:
# NAME         READY   STATUS    RESTARTS
# cilium-xxx   0/1     Running   0
#              ↑↑↑ 0/1 = Not ready (startup probe failing)

# Get the startup probe error
kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].status.containerStatuses[0].state}'

# Expected: Something about health check connection refused
```

✅ **If agent shows 0/1 Ready, agent initialization is incomplete.**

---

### Step 3: Check Operator (30 seconds)

```bash
# Check operator status
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator

# Expected outputs (pick one):
# 1) ImagePullBackOff  = Image auth failure → PRIMARY ISSUE
# 2) Running 0/1       = Running but not ready → SECONDARY ISSUE
# 3) Running 1/1       = Operator OK → Look at agent

# If ImagePullBackOff, check the error
kubectl describe pod -n kube-system <operator-pod> | grep -A 3 "ImagePull"
```

✅ **If you see ImagePullBackOff, start with FIX #1 below.**

---

## Fix #1: Operator Image Issue (Most Common)

### Quick Fix (2 minutes)

```bash
# Option A: Use the working CI image
export CILIUM_OPERATOR_IMAGE="quay.io/cilium/cilium-ci"
export CILIUM_OPERATOR_TAG="latest"

# Re-run test with these set
cd /var/home/james/dev/seriousum
./scripts/run-cilium-kind-test.sh --focus "YourTestPattern"
```

### If Option A Fails: Use Local Image

```bash
# Build locally
cd /var/home/james/dev/seriousum
cargo build --release

# The local seriousum operator image should already be built
export CILIUM_OPERATOR_IMAGE="localhost:5000/seriousum/operator-generic"
export CILIUM_OPERATOR_TAG="local"
export LOAD_INTO_KIND=1

# Load into kind and re-run
./scripts/run-cilium-kind-test.sh --load --focus "YourTestPattern"
```

### If Option B Fails: Add Auth Secret

```bash
# Create secret (requires quay.io credentials)
kubectl create secret docker-registry quay-pull-secret \
  --docker-server=quay.io \
  --docker-username=YOUR_USERNAME \
  --docker-password=YOUR_PASSWORD \
  -n kube-system

# Patch service account to use secret
kubectl patch serviceaccount cilium-operator -n kube-system \
  -p '{"imagePullSecrets": [{"name": "quay-pull-secret"}]}'

# Delete and recreate operator pod
kubectl delete pod -n kube-system -l app.kubernetes.io/name=cilium-operator
```

**Expected result**: Operator pod transitions to Running 1/1 within 1 minute

---

## Fix #2: Agent Initialization Issue

### If Operator is OK but Agent Still 0/1:

```bash
# Get agent pod name
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')

# Check agent logs for crash or errors
kubectl logs -n kube-system $AGENT_POD -c cilium-agent --tail=50

# Look for keywords: error, panic, fail, exit, abort

# If logs show OOM:
kubectl top pod -n kube-system -l k8s-app=cilium

# If OOM present, increase resources in DaemonSet:
kubectl set resources daemonset/cilium -n kube-system \
  -c cilium-agent \
  --limits=memory=1Gi,cpu=500m \
  --requests=memory=512Mi,cpu=250m
```

### Check for BPF Issues

```bash
# Verify BPF subsystem is available
kubectl exec -n kube-system $AGENT_POD -- grep BPF /boot/config-$(uname -r)

# Expected: CONFIG_BPF=y, CONFIG_BPF_SYSCALL=y, CONFIG_NET_CLS_BPF=y

# If missing, may need kernel upgrade or BPF backport
```

### Check Mount Points

```bash
# Verify socket directory is mounted
kubectl exec -n kube-system $AGENT_POD -- ls -ld /var/run/cilium/

# Expected: drwxr-xr-x root root /var/run/cilium/

# Check all cilium mounts
kubectl exec -n kube-system $AGENT_POD -- mount | grep cilium
```

**Expected result**: Agent pod should show 1/1 Ready within 2 minutes

---

## Step 4: Verify Socket Creation

### Once Agent is Ready:

```bash
# Check if socket file now exists
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl exec -n kube-system $AGENT_POD -- ls -la /var/run/cilium/cilium.sock

# Expected output:
# srw-rw-rw- 1 root root /var/run/cilium/cilium.sock
#            ↑ 's' at start means socket (not regular file)
```

✅ **If socket exists, CoreDNS should start within seconds.**

---

## Step 5: Verify CoreDNS Recovery

```bash
# Watch CoreDNS pod status
kubectl get pods -n kube-system -l k8s-app=kube-dns -w

# Expected progression:
# coredns-xxx   0/1   Pending        ContainerCreating (10s)
# coredns-xxx   0/1   ContainerCreating (20s)
# coredns-xxx   1/1   Running        (30s)

# Once Running, check logs
kubectl logs -n kube-system <coredns-pod> | head -20
```

✅ **If CoreDNS reaches Running, socket issue is RESOLVED.**

---

## Verification: All Systems Ready

Once everything is working, you should see:

```bash
$ kubectl get pods -n kube-system
NAME                              READY   STATUS    
cilium-operator-xxx               1/1     Running
cilium-xxxx                       1/1     Running
cilium-yyyy                       1/1     Running
coredns-xxx                       1/1     Running
coredns-yyy                       1/1     Running
etcd-kind-control-plane           1/1     Running
kube-apiserver-kind-control-pl    1/1     Running
kube-controller-manager-kind-co   1/1     Running
kube-proxy-xxx                    1/1     Running
kube-proxy-yyy                    1/1     Running
kube-scheduler-kind-control-pla   1/1     Running

$ kubectl get nodes
NAME                 STATUS   ROLES
kind-control-plane   Ready    control-plane,master
kind-worker          Ready    <none>

$ kubectl exec -n kube-system <agent-pod> -- ls -la /var/run/cilium/cilium.sock
srw-rw-rw- 1 root root /var/run/cilium/cilium.sock
```

✅ All green = Ready to run tests!

---

## Quick Diagnostics Command

```bash
# One-liner to check all three critical components
echo "=== OPERATOR ===" && \
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator --no-headers && \
echo "=== AGENT ===" && \
kubectl get pods -n kube-system -l k8s-app=cilium --no-headers && \
echo "=== COREDNS ===" && \
kubectl get pods -n kube-system -l k8s-app=kube-dns --no-headers && \
echo "=== NODES ===" && \
kubectl get nodes --no-headers
```

Expected:
```
=== OPERATOR ===
cilium-operator-xxx   1/1     Running
=== AGENT ===
cilium-xxxx           1/1     Running
cilium-yyyy           1/1     Running
=== COREDNS ===
coredns-xxx           1/1     Running
coredns-yyy           1/1     Running
=== NODES ===
kind-control-plane    Ready
kind-worker           Ready
```

---

## Run Full Diagnostics

When something is still wrong after attempting fixes:

```bash
cd /var/home/james/dev/seriousum
bash scripts/diagnose-cni-socket-timing.sh

# This generates detailed report: cni-socket-timing-report.txt
# Review this report for specific errors and logs
cat cni-socket-timing-report.txt | less
```

---

## Timeline Targets

| Phase | Current | Target | OK? |
|-------|---------|--------|-----|
| Cluster bootstrap | ~2 min | <2 min | ✅ |
| Operator ready | +5-10 min | +1 min | ❌ SLOW |
| Agent ready | +10-15 min | +2 min | ❌ SLOW |
| Socket created | +15 min | +2.5 min | ❌ SLOW |
| CoreDNS ready | +20 min | +3 min | ❌ SLOW |
| **Total to test-ready** | **~25-30 min** | **<5 min** | ❌ 5-6x SLOWER |

Once fixes are applied, expect:
- Operator: <1 min to Ready
- Agent: <2 min to Ready
- Socket: Created immediately after agent Ready
- CoreDNS: <30s after socket exists
- **Total**: 3-5 minutes (5-6x improvement)

---

## If Still Not Working

1. **Collect full diagnostics**:
   ```bash
   bash scripts/diagnose-cni-socket-timing.sh
   ```

2. **Save the report**:
   ```bash
   cp cni-socket-timing-report.txt ~/cilium-debug-$(date +%Y%m%d-%H%M%S).txt
   ```

3. **Check detailed logs**:
   ```bash
   AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
   kubectl logs -n kube-system $AGENT_POD -c cilium-agent > ~/agent-logs.txt
   kubectl logs -n kube-system $AGENT_POD -c cilium-agent --previous > ~/agent-logs-previous.txt
   ```

4. **Check operator logs**:
   ```bash
   OP_POD=$(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o jsonpath='{.items[0].metadata.name}')
   kubectl logs -n kube-system $OP_POD > ~/operator-logs.txt
   ```

5. **Review**: The logs will show exactly where initialization is stalling.

---

## Summary

| If You See | Problem | Fix |
|------------|---------|-----|
| coredns Pending + agent 0/1 + operator good | Agent init failure | Fix #2 or check BPF |
| coredns Pending + agent 0/1 + operator ImagePullBackOff | Operator image auth | Fix #1 |
| coredns Running + agent 1/1 + socket exists | ✅ All good! Run tests |
| Something else | Unknown | Run full diagnostics |

---

## Next: Run Tests

Once all pods are Ready and socket exists:

```bash
cd /var/home/james/dev/seriousum

# Run a quick test to verify system is ready
./scripts/run-cilium-kind-test.sh \
  --focus "Cilium" \
  --no-bootstrap-cluster \
  --skip-build \
  --skip-dropin \
  --test-timeout 10m

# Expected: Tests run successfully for 7-10 minutes
# (Previously timing out or failing in BeforeEach)
```

---

## Contact for Deeper Issues

If after trying these steps the socket is still missing and diagnostics aren't clear:

1. Review full diagnostic report: `cni-socket-timing-report.txt`
2. Review agent logs for specific error messages
3. Check if this is a Rust agent-specific issue vs upstream cilium compatibility
4. May need to debug agent code directly (initialization sequence)

See main investigation document: `CNI_SOCKET_TIMING_INVESTIGATION.md`
