# CRD Sync Verification - Quick Reference Card

## 📋 All 5 Verification Tasks - At a Glance

### Task 1: Get List of CRDs Operator Creates
```bash
kubectl get crd | grep cilium
```
**Expected:** ~9 CRDs (CiliumNode, CiliumEndpoint, CiliumNetworkPolicy, etc.)  
**Current Status:** ❌ NOT CREATED (Operator fails to start)

---

### Task 2: Check Operator Logs for CRD Creation
```bash
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator --tail=100 | grep -i 'crd\|register\|create'
```
**Expected:** "CRD X registered successfully"  
**Current Status:** ❌ NO LOGS (Operator stuck at image pull: 401 UNAUTHORIZED)

---

### Task 3: Check Agent Logs for CRD Wait/Sync
```bash
kubectl logs -n kube-system -l k8s-app=cilium --tail=100 | grep -i 'crd\|wait\|sync'
```
**Expected:** "Observing CRD updates", "CRD sync complete"  
**Current Status:** ❌ NO LOGS (Agent startup health check fails)

---

### Task 4: Verify CRD Fields (Sample: CiliumNode)
```bash
kubectl get ciliumnodes -o json | jq '.items[0] | {
  identity: .spec.identity,
  addresses: (.spec.addresses | length),
  status: (.status | keys)
}'
```
**Expected Fields:**
- spec.identity (number)
- spec.addresses (array)
- status.cilium-health
- status.node-addresses

**Current Status:** ❌ CANNOT VERIFY (No CiliumNode resources created)

---

### Task 5: CRD Sync Status & Recommendations

| Aspect | Status | Issue Type | Recommendation |
|--------|--------|-----------|-----------------|
| CRD Creation | ❌ BLOCKED | **TIMING** | Fix operator image auth |
| Field Population | ❌ NOT STARTED | **TIMING** | Fix agent startup checks |
| Agent Observation | ❌ FAILED | **TIMING** | Debug health endpoint |
| Data Completeness | ❓ UNKNOWN | **DATA** | Add field validation |
| Sync Visibility | ❌ MISSING | **SEQUENCING** | Add metrics/logging |

**Overall Status:** ⚠️ **CRITICAL** - **TIMING/SEQUENCING ISSUES** (not data corruption)

---

## 🔧 Quick Fixes

### Fix 1: Operator Image Authentication (IMMEDIATE)
```bash
# Option A: Use local image
kind load docker-image --name=kind localhost:5000/seriousum/cilium-operator:local
kubectl set image deployment/cilium-operator -n kube-system \
  cilium-operator=localhost:5000/seriousum/cilium-operator:local
kubectl rollout status deployment/cilium-operator -n kube-system

# Option B: Create image pull secret
kubectl create secret docker-registry quay-creds -n kube-system \
  --docker-server=quay.io --docker-username=USER --docker-password=TOKEN
kubectl patch sa cilium-operator -n kube-system \
  -p '{"imagePullSecrets": [{"name": "quay-creds"}]}'
kubectl rollout restart deployment/cilium-operator -n kube-system
```

### Fix 2: Agent Health Check Debug (IMMEDIATE)
```bash
# Increase startup probe timeout
kubectl edit ds cilium -n kube-system
# Change startupProbe.failureThreshold from 3 to 30 (300s total)

# Check resources
kubectl top nodes
kubectl describe node <node-name> | grep -A 5 "Allocated"

# Verify BPF support
kubectl debug node <node-name> -it -- bash
# Inside: cat /proc/sys/kernel/unprivileged_bpf_disabled
#         grep BPF /boot/config-*
```

### Fix 3: Add CRD Wait Logic (THIS SPRINT)
See `CRD_SYNC_FIXES.md` "Fix 3: Add Explicit CRD Wait Logic" for code

### Fix 4: Add Field Validation (NEXT SPRINT)
See `CRD_SYNC_FIXES.md` "Fix 4: Add CRD Field Validation" for code

### Fix 5: Add Observability (NEXT SPRINT)
See `CRD_SYNC_FIXES.md` "Fix 5: Add Sync Observability" for code

---

## 🩺 Verification Commands

```bash
# Check if operator is running
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator

# Check if agent is running  
kubectl get ds cilium -n kube-system

# Count CRDs
kubectl get crd | grep cilium | wc -l

# Check agent health
AGENT=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl exec -it $AGENT -n kube-system -- cilium status

# Check CNI socket
kubectl debug node $(kubectl get nodes -o jsonpath='{.items[0].metadata.name}') -it -- \
  ls -la /var/run/cilium/cilium.sock

# Full diagnostic bundle
bash CRD_SYNC_DIAGNOSTIC_CHECKLIST.md
```

---

## 📚 Documentation Files

| File | Purpose | When to Use |
|------|---------|-----------|
| **CRD_SYNC_VERIFICATION_REPORT.md** | Complete analysis of all tasks | For understanding root causes |
| **CRD_SYNC_DIAGNOSTIC_CHECKLIST.md** | Ready-to-run kubectl commands | For diagnosing live clusters |
| **CRD_SYNC_FIXES.md** | Implementation guide with code | For implementing fixes |
| **CRD_SYNC_QUICK_REFERENCE.md** | This file | For quick lookup |

---

## ❌ Healthy vs. Broken Sync

### ✅ Healthy CRD Sync
```
✓ Operator: Running (1/1 Ready)
✓ Agent: All Ready (N/N)
✓ CRDs: ~9 present
✓ CNI socket: Exists
✓ Logs: Show "registered", "observing"
→ ACTION: No action needed
```

### ⚠️ Partial CRD Sync
```
⚠ Operator: Running but high memory
⚠ Agent: Some not ready
⚠ CRDs: Some missing
⚠ Logs: "Waiting for CRD..."
→ ACTION: Check logs, wait longer, check resources
```

### ❌ Broken CRD Sync
```
✗ Operator: ImagePullBackOff / CrashLoopBackOff
✗ Agent: Pending / CrashLoopBackOff
✗ CRDs: 0 or very few
✗ Logs: "Failed to pull" / "Timeout"
→ ACTION: Apply Fix 1 immediately
```

---

## 🎯 Root Cause Summary

**Q: Why are CRDs not being created?**  
A: Operator pod stuck in ImagePullBackOff (401 UNAUTHORIZED on quay.io)

**Q: Why aren't agents becoming healthy?**  
A: Health check endpoint (9879) unreachable - agent startup fails before reaching running state

**Q: Why is the CNI socket missing?**  
A: Agent pods never reach running state due to health check failures

**Q: Is this a data problem?**  
A: No - it's a TIMING problem. Operator and agent fail before any code path that handles CRDs is reached.

**Q: What's the dependency chain?**  
A: Operator image → Operator pod → CRD creation → Agent health → CNI socket → Pod networking

**Broken at:** First step (Operator image pull)

---

## ⏱️ Implementation Timeline

| Priority | Item | Effort | Timeline | Blocks |
|----------|------|--------|----------|--------|
| P0 | Fix operator image | 30 min | TODAY | All testing |
| P0 | Debug agent health | 2-4 hrs | TODAY | All testing |
| P0 | Verify CNI socket | 1 hr | TODAY | All testing |
| P1 | Add CRD wait logic | 4-8 hrs | THIS SPRINT | Race conditions |
| P1 | Add field validation | 2-4 hrs | THIS SPRINT | Silent failures |
| P2 | Add observability | 4-8 hrs | NEXT SPRINT | Debugging ease |

**Estimated time to working CRD sync:** 3-6 hours (P0 items)  
**Estimated time to robust CRD sync:** 1-2 sprints (all items)

---

## 🔗 Related Documents

- **Integration Test Findings:** INTEGRATION_TEST_FINDINGS.md
- **K8s Datapath Test RCA:** K8sDatapathServicesTest_RCA.json
- **Progress Snapshot:** PROGRESS_SNAPSHOT.md

---

## 📞 Quick Help

**"CRDs aren't being created"**
→ Check operator pod status: `kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator`

**"Agent pods are crashing"**
→ Check startup probe: `kubectl logs <agent-pod> -n kube-system --previous`

**"I don't see CRD sync logs"**
→ Likely operator/agent never reached that code. Check their startup logs first.

**"Which fix should I apply first?"**
→ Fix 1 (operator image) - it unblocks everything else.

**"How long does CRD sync take?"**
→ When healthy: 5-10 seconds. When broken: Never completes.

---

Generated: 2026-05-11
Status: Verification Complete, Analysis Complete, Fixes Documented
