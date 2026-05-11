# CNI Socket Timing - Technical Deep Dive

**For**: Developers debugging Rust agent code  
**Purpose**: Understand socket creation sequence and identify where initialization is blocking

---

## Background: What the Socket Is

### Purpose
The Cilium agent exposes a Unix domain socket at `/var/run/cilium/cilium.sock` that:
1. Receives CNI ADD/DEL requests from kubelet
2. Processes network policy, BPF program loading
3. Returns network interface info for pod sandbox

### Lifecycle
```
Agent starts
  ↓
Read config / CRDs
  ↓
Initialize BPF subsystem
  ↓
Load base BPF programs
  ↓
Create socket (/var/run/cilium/cilium.sock) ← THIS IS BLOCKING
  ↓
Bind to Unix socket
  ↓
Listen for connections
  ↓
Startup probe succeeds (port 9879 responds)
  ↓
CNI requests can now be processed
```

---

## Current Problem: Initialization Blocked

### Where We Get Stuck

```
✅ Agent container started
✅ Agent binary invoked
❌ Config/CRD reading (possibly stalled waiting for operator)
❌ BPF subsystem initialization (possibly OOM or missing kernel features)
❌ Socket creation never happens
❌ Health check port 9879 not bound
```

### Evidence Socket Creation is Blocked
1. Pod status: `Running` (container is alive)
2. Health check: Connection refused on port 9879
3. Socket check: `/var/run/cilium/cilium.sock` does not exist
4. Error log: No obvious crash in pod logs (check `--previous`)

### Hypothesis
One of these is blocking socket creation:
1. Waiting for operator-populated CRDs
2. Waiting for BPF compilation/loading (slow if CPU-constrained)
3. Waiting for node initialization from cilium-node-init DaemonSet
4. Memory exhaustion (OOM) before socket creation reached
5. Permission issue mounting `/var/run/cilium` directory

---

## How to Debug: Investigation Steps

### Step 1: Check Container Status (Is Agent Alive?)

```bash
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')

# Check if container is running
kubectl get pod -n kube-system $AGENT_POD -o jsonpath='{.status.containerStatuses[0]}'

# Look for:
# - ready: false (not ready)
# - started: true (but not ready - startup probe failing)
# - state.running.startedAt: (when it started)
```

### Step 2: Get Agent Logs (What's It Trying To Do?)

```bash
# Get live logs
kubectl logs -n kube-system $AGENT_POD -c cilium-agent -f

# Or get logs since pod started
kubectl logs -n kube-system $AGENT_POD -c cilium-agent --timestamps=true

# Look for:
# - Errors (ERR, ERROR)
# - Initialization messages
# - Socket creation messages
# - CRD/operator messages
# - BPF loading messages

# If pod crashed, get previous logs
kubectl logs -n kube-system $AGENT_POD -c cilium-agent --previous
```

### Step 3: Check Resource Constraints

```bash
# Memory and CPU usage
kubectl top pod -n kube-system $AGENT_POD

# If near limits, pod may be OOM-killed silently
# Check for OOM killer
kubectl describe pod -n kube-system $AGENT_POD | grep -i "oom\|memory"

# Check resource limits in DaemonSet
kubectl get daemonset -n kube-system cilium -o yaml | grep -A 10 "resources:"
```

### Step 4: Verify Mount Points

```bash
# Check if /var/run/cilium is mounted
kubectl exec -n kube-system $AGENT_POD -- mount | grep cilium

# Check directory permissions
kubectl exec -n kube-system $AGENT_POD -- ls -ld /var/run/cilium/

# Should output:
# drwxr-xr-x 2 root root 80 May 11 17:00 /var/run/cilium/

# Try to create a test file to verify write permission
kubectl exec -n kube-system $AGENT_POD -- \
  touch /var/run/cilium/test.txt && echo "✅ Can write" || echo "❌ Permission denied"
```

### Step 5: Check CRD/Operator Status

```bash
# Check if operator is ready
kubectl get deployment -n kube-system cilium-operator

# Check if CRDs are registered
kubectl get crds | grep cilium

# Check if key CRDs have content
kubectl get ciliumconfigs -n kube-system
kubectl get ciliumnode

# If empty or error, operator may not have initialized
```

### Step 6: Check BPF Subsystem

```bash
# Verify kernel BPF support
kubectl exec -n kube-system $AGENT_POD -- \
  grep BPF /boot/config-$(uname -r)

# Should show:
# CONFIG_BPF=y
# CONFIG_BPF_SYSCALL=y
# CONFIG_NET_CLS_BPF=y
# CONFIG_NET_ACT_BPF=y

# If not present, kernel doesn't support BPF (won't work)

# Check BPF filesystem
kubectl exec -n kube-system $AGENT_POD -- mount | grep bpffs

# Should show:
# none on /sys/fs/bpf type bpf (rw,nosuid,nodev,noexec,relatime)
```

---

## Analysis: Why Socket Creation Blocks

### Scenario A: Operator Not Ready (Most Likely)

**Symptom**: Agent logs show waiting for operator

**Evidence**:
```bash
# Check operator readiness
kubectl get pod -n kube-system -l app.kubernetes.io/name=cilium-operator

# If not Running/Ready, agent is likely waiting
kubectl logs -n kube-system <operator-pod> | head -50
```

**Fix**: 
- Fix operator image (see Quick Fix guide)
- Agent can proceed with partial CRD state

**Agent Code**: Check if initialization explicitly waits for specific CRDs

### Scenario B: BPF Compilation Slow (Possible)

**Symptom**: Agent logs show BPF loading/compiling, but then hangs

**Evidence**:
```bash
# Check for BPF-related log entries
kubectl logs -n kube-system $AGENT_POD -c cilium-agent | \
  grep -i "bpf\|compile\|load\|verif"

# If stuck on BPF, logs will show no progress for minutes
```

**Fix**:
- May need to profile BPF loading
- Possibly pre-compile or cache BPF programs
- Or disable certain BPF features for testing

**Agent Code**: Check BPF initialization loop; may need timeout or streaming progress

### Scenario C: Memory Exhaustion (Less Likely But Possible)

**Symptom**: Pod status shows Running but process not responding

**Evidence**:
```bash
# Check for OOM
kubectl describe pod -n kube-system $AGENT_POD | grep -i oom

# Check memory pressure
kubectl top pod -n kube-system $AGENT_POD

# If near limit, process may have been OOM-killed
```

**Fix**:
- Increase resource requests in DaemonSet
- Profile agent memory usage
- May indicate memory leak in initialization

**Agent Code**: Check for large allocations before socket creation

### Scenario D: Mount Permission Issue (Possible)

**Symptom**: Agent logs may show permission denied on `/var/run/cilium`

**Evidence**:
```bash
# Try to write to socket directory
kubectl exec -n kube-system $AGENT_POD -- \
  touch /var/run/cilium/test.txt 2>&1

# If permission denied, mount is wrong
```

**Fix**:
- Check DaemonSet mount specification
- Verify host path exists on nodes
- May need to run container as root (already done)

**Agent Code**: Check if socket creation has explicit error handling

---

## Socket Creation Code Pattern (What We're Looking For)

In the Rust agent code, socket creation typically looks like:

```rust
// This is what should be happening (in agent startup)

// 1. Create socket directory
std::fs::create_dir_all("/var/run/cilium")?;

// 2. Remove old socket if exists
let _ = std::fs::remove_file("/var/run/cilium/cilium.sock");

// 3. Create Unix domain socket
let listener = UnixListener::bind("/var/run/cilium/cilium.sock")?;

// 4. Set permissions
std::fs::set_permissions("/var/run/cilium/cilium.sock", 
  std::fs::Permissions::from_mode(0o666))?;

// 5. Start accepting connections
while let Ok((connection, _)) = listener.accept() {
  // Handle CNI requests
}
```

**If this is in agent code but socket not created**, the issue is earlier in the initialization sequence.

**If this is not in agent code**, it needs to be added.

---

## Recommended Debugging Additions

### Add Startup Progress Logging

```rust
// Before initializing each major subsystem
eprintln!("[STARTUP] Starting config initialization...");
// ... config code ...
eprintln!("[STARTUP] ✅ Config ready");

eprintln!("[STARTUP] Starting BPF subsystem...");
// ... BPF code ...
eprintln!("[STARTUP] ✅ BPF ready");

eprintln!("[STARTUP] Creating socket...");
let listener = UnixListener::bind(SOCKET_PATH)?;
eprintln!("[STARTUP] ✅ Socket created: {}", SOCKET_PATH);
```

### Add Health Check Endpoint Earlier

```rust
// Bind health check port BEFORE full initialization
// This helps diagnose where startup is stalled

let health_listener = TcpListener::bind("127.0.0.1:9879")?;

// Spawn thread to handle health checks
// (even if initialization not complete yet)
spawn_health_thread(health_listener);

// Then proceed with full initialization
// Health check can now report "initializing" instead of refusing
```

### Add Timeout to Blocking Operations

```rust
// Wrap potentially blocking operations in timeout
let operator_ready = tokio::time::timeout(
    Duration::from_secs(30),
    wait_for_operator_crd()
).await?;

// If timeout, log and continue with defaults
// (don't block forever)
```

---

## Testing the Fix

Once code changes are made:

```bash
# Rebuild agent
cargo build --release -p seriousum-cilium-agent

# Load into kind cluster
docker build -t localhost:5000/seriousum/cilium-agent:test .
kind load docker-image --name kind localhost:5000/seriousum/cilium-agent:test

# Run test
export CILIUM_IMAGE="localhost:5000/seriousum/cilium-agent"
export CILIUM_TAG="test"
./scripts/run-cilium-kind-test.sh --focus "YourPattern"

# Watch for socket creation
kubectl logs -n kube-system -l k8s-app=cilium -f -c cilium-agent | \
  grep -i "socket\|startup\|ready"
```

---

## Expected Behavior After Fix

### In Agent Logs:
```
[STARTUP] Starting agent initialization
[STARTUP] Loading config from operator
[STARTUP] ✅ Config ready
[STARTUP] Initializing BPF subsystem
[STARTUP] ✅ BPF ready
[STARTUP] Creating CNI socket
[STARTUP] ✅ Socket created: /var/run/cilium/cilium.sock
[STARTUP] Starting health check endpoint
[STARTUP] ✅ Health check ready on 127.0.0.1:9879
[STARTUP] Agent fully initialized, ready for CNI requests
```

### In Kubernetes:
```bash
$ kubectl get pods -n kube-system -l k8s-app=cilium
NAME         READY   STATUS    
cilium-xxx   1/1     Running   ← Changed from 0/1

$ kubectl get pods -n kube-system -l k8s-app=kube-dns
NAME         READY   STATUS    
coredns-xxx  1/1     Running   ← Changed from 0/1 Pending

$ kubectl exec -n kube-system <pod> -- ls -l /var/run/cilium/cilium.sock
srw-rw-rw- 1 root root /var/run/cilium/cilium.sock ← Now exists
```

---

## Debugging Checklist

Use this checklist when socket is still missing after basic fixes:

- [ ] Agent container is running (`kubectl get pods` shows Running)
- [ ] Agent logs exist and don't show crash (`kubectl logs ...`)
- [ ] Operator is Running and Ready (not ImagePullBackOff)
- [ ] CRDs exist (`kubectl get crds | grep cilium`)
- [ ] BPF support enabled (`grep BPF /boot/config-$(uname -r)`)
- [ ] `/var/run/cilium` directory exists and writable
- [ ] Agent logs show initialization messages
- [ ] Socket creation attempted in logs (if added)
- [ ] No OOM or permission errors in logs
- [ ] Health check port not bound (means stuck before that)

---

## Next Steps for Code Fix

Based on this analysis:

1. **Add startup progress logging** - Identify exactly where initialization stalls
2. **Add timeout to blocking operations** - Prevent infinite waits
3. **Move health check initialization earlier** - Better diagnostics
4. **Add explicit socket creation error handling** - Show why socket fails
5. **Test with profiling** - Measure time spent in each phase

Once these are in, re-run investigation to get clear diagnostics of where the real bottleneck is.
