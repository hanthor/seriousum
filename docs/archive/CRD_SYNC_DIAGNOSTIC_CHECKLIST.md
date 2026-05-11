# CRD Sync Verification - Quick Diagnostics Checklist

**Usage:** When running Cilium on a live cluster, use these commands to diagnose operator-agent CRD sync issues.

---

## 1. Check Operator Pod Status

```bash
# Get operator pods
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o wide

# Expected:
# NAME                          READY   STATUS    RESTARTS   AGE
# cilium-operator-xxxxx         1/1     Running   0          2m

# If NOT Running:
kubectl describe pod <pod-name> -n kube-system
# Look for: ImagePullBackOff, ImagePullFailed, CrashLoopBackOff, Pending
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== OPERATOR STATUS ==="
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator
echo ""
echo "=== OPERATOR EVENTS ==="
kubectl describe pod -n kube-system -l app.kubernetes.io/name=cilium-operator | grep -A 5 "Events:"
echo ""
echo "=== OPERATOR LOGS (Last 50 lines) ==="
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator --tail=50
```

---

## 2. Check CRD Creation Status

```bash
# List all Cilium-related CRDs
kubectl get crd | grep cilium

# Expected output (should have ~9 CRDs):
# ciliumclusterwidenetworkpolicies.cilium.io
# ciliumendpoints.cilium.io
# ciliumidentities.cilium.io
# ciliumnetworkpolicies.cilium.io
# ciliumnodes.cilium.io
# ... (others)

# If NO CRDs visible:
echo "CRD registration BLOCKED - operator not running successfully"
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== CILIUM CRDS ==="
CRDS=$(kubectl get crd | grep cilium | wc -l)
echo "Found $CRDS Cilium CRDs (expected ~9)"
if [ $CRDS -lt 8 ]; then
  echo "WARNING: CRD count low - operator may not be fully initialized"
  echo "Check operator logs for CRD registration errors"
fi

echo ""
echo "=== DETAILED CRD INFO ==="
kubectl get crd -l app.kubernetes.io/name=cilium -o json | \
  jq '.items[] | {name: .metadata.name, established: .status.conditions[0].status}'
```

---

## 3. Check Agent Pod Status & Health

```bash
# Get agent daemonset pods
kubectl get pods -n kube-system -l k8s-app=cilium -o wide

# Expected:
# NAME                          READY   STATUS    RESTARTS   AGE
# cilium-xxxxx                  1/1     Running   0          2m

# Check health
kubectl exec -it <cilium-pod> -n kube-system -- cilium status

# Expected output:
#   /healthz:           OK
#   Cilium version:     ...
#   Kernel version:     ...
#   Kubernetes version: ...
#   Container runtime:  ...
#   Orchestration platform: ...
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== AGENT POD STATUS ==="
kubectl get ds -n kube-system cilium -o wide

echo ""
echo "=== AGENT POD READY COUNT ==="
READY=$(kubectl get ds -n kube-system cilium -o jsonpath='{.status.numberReady}')
DESIRED=$(kubectl get ds -n kube-system cilium -o jsonpath='{.status.desiredNumberScheduled}')
echo "Ready: $READY / $DESIRED"

if [ "$READY" != "$DESIRED" ]; then
  echo "WARNING: Not all agent pods are ready"
  echo "Checking pod events..."
  kubectl describe pods -n kube-system -l k8s-app=cilium | grep -A 3 "Events:"
fi

echo ""
echo "=== AGENT HEALTH ==="
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
echo "Testing health on pod: $AGENT_POD"
kubectl exec -it $AGENT_POD -n kube-system -- cilium status | head -20
```

---

## 4. Check Operator Logs for CRD Registration

```bash
# Get operator pod name
OPERATOR_POD=$(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o jsonpath='{.items[0].metadata.name}')

# Search for CRD creation logs
kubectl logs $OPERATOR_POD -n kube-system | grep -i 'crd\|register\|create'

# Expected log patterns:
# "Registering custom resource definitions"
# "CRD CiliumNode registered"
# "CRD CiliumEndpoint registered"
# "CRD CiliumNetworkPolicy registered"
# "CRD registration successful"
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== OPERATOR CRD REGISTRATION LOGS ==="
OPERATOR_POD=$(kubectl get pods -n kube-system \
  -l app.kubernetes.io/name=cilium-operator \
  -o jsonpath='{.items[0].metadata.name}')

echo "Operator pod: $OPERATOR_POD"
echo ""
echo "Searching for CRD-related logs..."
kubectl logs $OPERATOR_POD -n kube-system --tail=100 | \
  grep -i 'crd\|register\|create\|schema' | head -20

echo ""
echo "If no results above, operator may not have reached CRD code path"
```

---

## 5. Check Agent Logs for CRD Waiting/Sync

```bash
# Get first agent pod
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')

# Search for CRD wait/sync logs
kubectl logs $AGENT_POD -n kube-system | grep -i 'crd\|wait\|sync\|ready'

# Expected log patterns:
# "Waiting for CRD CiliumNode"
# "Observing CiliumEndpoint updates"
# "CRD sync complete"
# "Datapath initialization complete"
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== AGENT CRD SYNC LOGS ==="
AGENT_POD=$(kubectl get pods -n kube-system \
  -l k8s-app=cilium \
  -o jsonpath='{.items[0].metadata.name}')

echo "Agent pod: $AGENT_POD"
echo ""
echo "Searching for CRD sync logs..."
kubectl logs $AGENT_POD -n kube-system --tail=100 | \
  grep -i 'crd\|wait\|sync\|ready\|observ' | head -20

echo ""
echo "If no results above, agent may not have reached CRD sync code path"
```

---

## 6. Verify CRD Field Presence (Sample: CiliumNode)

```bash
# Get a CiliumNode resource
kubectl get ciliumnodes -o json | jq '.items[0]'

# Expected structure:
# {
#   "apiVersion": "cilium.io/v2",
#   "kind": "CiliumNode",
#   "metadata": {...},
#   "spec": {
#     "identity": <number>,
#     "addresses": [...],
#     "health": {...}
#   },
#   "status": {
#     "alibaba": {...},
#     "aws": {...},
#     "cilium-health": {...},
#     "node-addresses": [...]
#   }
# }
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== CILIUMNODE CRD SAMPLE ==="
NODE_COUNT=$(kubectl get ciliumnodes -o jsonpath='{.items|length}')
echo "Total CiliumNode resources: $NODE_COUNT"

if [ $NODE_COUNT -gt 0 ]; then
  echo ""
  echo "First node details:"
  kubectl get ciliumnodes -o json | jq '.items[0] | {
    name: .metadata.name,
    identity: .spec.identity,
    addresses: (.spec.addresses | length),
    status_fields: (.status | keys)
  }'
else
  echo "WARNING: No CiliumNode resources found"
  echo "Operator may not have synced with agent"
fi

echo ""
echo "=== CILIUMNODE FIELD VALIDATION ==="
kubectl get ciliumnodes -o json | jq '.items[0] | 
  if (.spec.identity and .spec.addresses and .status."cilium-health") 
  then "✓ All required fields present" 
  else "✗ Missing required fields" 
  end'
```

---

## 7. Check CNI Plugin Availability

```bash
# Check if cilium CNI binary exists on nodes
kubectl get nodes -o jsonpath='{.items[0].metadata.name}' | \
  xargs -I {} kubectl debug node {} -it -- \
  ls -la /opt/cni/bin/cilium

# Expected: cilium binary should exist and be executable

# Check CNI socket availability
kubectl get nodes -o jsonpath='{.items[0].metadata.name}' | \
  xargs -I {} kubectl debug node {} -it -- \
  ls -la /var/run/cilium/cilium.sock

# Expected: Socket should exist if agent is running
```

**Diagnostic Command:**
```bash
#!/bin/bash
echo "=== CNI PLUGIN VERIFICATION ==="
NODE=$(kubectl get nodes -o jsonpath='{.items[0].metadata.name}')
echo "Checking node: $NODE"

echo ""
echo "CNI Binary:"
kubectl debug node $NODE -it -- \
  bash -c 'if [ -f /opt/cni/bin/cilium ]; then 
    echo "✓ Binary exists"; stat /opt/cni/bin/cilium 
  else 
    echo "✗ Binary missing"; ls /opt/cni/bin/ 
  fi'

echo ""
echo "CNI Socket:"
kubectl debug node $NODE -it -- \
  bash -c 'if [ -S /var/run/cilium/cilium.sock ]; then 
    echo "✓ Socket exists"; stat /var/run/cilium/cilium.sock 
  else 
    echo "✗ Socket missing"; ls -la /var/run/cilium/ 
  fi'
```

---

## 8. Full Diagnostic Bundle

```bash
#!/bin/bash
# Run all diagnostics and save to file

echo "=== CILIUM OPERATOR-AGENT CRD SYNC VERIFICATION ===" | tee cilium-diagnostics.log
echo "Timestamp: $(date -Iseconds)" | tee -a cilium-diagnostics.log
echo "" | tee -a cilium-diagnostics.log

echo "1. OPERATOR STATUS" | tee -a cilium-diagnostics.log
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o wide | tee -a cilium-diagnostics.log

echo "" | tee -a cilium-diagnostics.log
echo "2. AGENT STATUS" | tee -a cilium-diagnostics.log
kubectl get ds -n kube-system cilium -o wide | tee -a cilium-diagnostics.log

echo "" | tee -a cilium-diagnostics.log
echo "3. CRD LIST" | tee -a cilium-diagnostics.log
kubectl get crd | grep cilium | tee -a cilium-diagnostics.log

echo "" | tee -a cilium-diagnostics.log
echo "4. OPERATOR LOGS" | tee -a cilium-diagnostics.log
OPERATOR_POD=$(kubectl get pods -n kube-system \
  -l app.kubernetes.io/name=cilium-operator \
  -o jsonpath='{.items[0].metadata.name}')
kubectl logs $OPERATOR_POD -n kube-system --tail=100 | \
  grep -i 'crd\|register\|error' | tee -a cilium-diagnostics.log

echo "" | tee -a cilium-diagnostics.log
echo "5. AGENT LOGS" | tee -a cilium-diagnostics.log
AGENT_POD=$(kubectl get pods -n kube-system \
  -l k8s-app=cilium \
  -o jsonpath='{.items[0].metadata.name}')
kubectl logs $AGENT_POD -n kube-system --tail=100 | \
  grep -i 'crd\|wait\|sync\|error' | tee -a cilium-diagnostics.log

echo "" | tee -a cilium-diagnostics.log
echo "6. CILIUMNODE SAMPLE" | tee -a cilium-diagnostics.log
kubectl get ciliumnodes -o json | \
  jq '.items[0] | {name: .metadata.name, identity: .spec.identity}' | \
  tee -a cilium-diagnostics.log

echo "" | tee -a cilium-diagnostics.log
echo "Diagnostics saved to: cilium-diagnostics.log"
```

---

## Interpretation Guide

### ✅ Healthy CRD Sync

```
✓ Operator pod: Running (1/1)
✓ Agent pods: All Ready (X/X)
✓ CRD count: ~9 CRDs present
✓ Operator logs: "CRD registered successfully"
✓ Agent logs: "Observing CRD updates"
✓ CiliumNode: Present with spec.identity and status fields
✓ CNI socket: Present (/var/run/cilium/cilium.sock)
```

**Action:** No action needed - sync is working

### ⚠️ Partial CRD Sync

```
⚠ Operator pod: Running but high memory usage
⚠ Agent pods: Some not ready
⚠ CRD count: Some CRDs missing (< 8)
⚠ Operator logs: "CRD registration in progress..."
⚠ Agent logs: "Waiting for CRD..."
⚠ CiliumNode: Present but incomplete fields
```

**Action:**
1. Wait longer (CRD sync may still be in progress)
2. Check operator/agent logs for specific errors
3. Check node resources (memory, CPU, disk)

### ❌ Broken CRD Sync

```
✗ Operator pod: ImagePullBackOff / CrashLoopBackOff
✗ Agent pods: Pending / CrashLoopBackOff
✗ CRD count: 0 or very few
✗ Operator logs: "Failed to pull image" / "CRD registration failed"
✗ Agent logs: No logs or "Timeout waiting for CRD"
✗ CiliumNode: Missing or all fields empty
✗ CNI socket: Missing
```

**Actions (Priority Order):**
1. Fix operator image pull (authentication or availability)
2. Check operator startup logs for errors
3. Verify operator pod RBAC permissions
4. Check agent startup health checks
5. Verify CNI binary and socket availability

---

## Report Output

Save the full diagnostic output:

```bash
{
  "status": "healthy|partial|broken",
  "timestamp": "2026-05-11T...",
  "operator": {
    "ready": true/false,
    "pod_status": "...",
    "crd_registered": number
  },
  "agent": {
    "ready": true/false,
    "pod_ready_count": number,
    "pod_desired_count": number,
    "health_endpoint": "responding|not_responding|unknown"
  },
  "crd": {
    "total_count": number,
    "sample_node": {
      "name": "...",
      "identity": number,
      "fields_populated": true/false
    }
  },
  "recommendations": [
    "..."
  ]
}
```

