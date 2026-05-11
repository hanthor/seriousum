# Integration Gates Execution Plan (Task #17)

**Task**: #17 - Run integration gates  
**Status**: READY TO EXECUTE  
**Timeline**: 30+ minutes per test suite  
**Next Steps**: Execute after harness configuration confirmed (#16 complete)

## What Are Integration Gates?

Integration gates are real, end-to-end Cilium test suites that validate:
- Agent initialization and lifecycle
- Network connectivity and datapath
- Service load balancing
- Network policies enforcement
- Observability (Hubble)
- DNS/FQDN resolution
- Cluster mesh features

## Test Suites Recommended for Phase 1

### Priority 1 (Quick Validation)
**K8sFQDNTest** (3 specs, ~5 minutes)
- Fastest validation
- Tests DNS resolution in Kubernetes
- Should mostly work with upstream operator
- Good baseline for framework validation

Command:
```bash
just run K8sFQDNTest
```

### Priority 2 (Core Functionality)
**K8sDatapathServicesTest** (50 specs, ~10-15 minutes)
- Services load balancing
- Endpoint management
- Load balancer logic
- High priority for P1 implementation

Command:
```bash
just run K8sDatapathServicesTest
```

### Priority 3 (Policy Validation)
**K8sAgentPolicyTest** (50 specs, ~10-15 minutes)
- Network policy enforcement
- Egress/ingress rules
- Label-based policy matching

Command:
```bash
just run K8sAgentPolicyTest
```

### Priority 4 (Observability)
**K8sHubbleTest** (~30 specs, ~10 minutes)
- Hubble API functionality
- Flow visibility
- Observability features

Command:
```bash
just run K8sHubbleTest
```

## Execution Strategy

### Sequential Approach (Recommended)

Run suites one at a time on same cluster:
```bash
just run K8sFQDNTest              # ~30 min total (build + test)
# Analyze results
just run K8sDatapathServicesTest  # ~20 min (skip build, reuse cluster)
# Analyze results
just run K8sAgentPolicyTest       # ~20 min
# Analyze results
```

Benefits:
- Lower resource usage
- Faster feedback per suite
- Easier to isolate issues
- Can debug failures immediately

### All-at-Once Approach (Alternative)

Run all suites sequentially on same cluster:
```bash
just test-all-sequential 60m
```

Benefits:
- Single setup, multiple tests
- Complete picture in one go

## Expected Outcomes

### Success (Tests Pass)
- ✅ P0 items working (operator, CNI socket, CRDs)
- ✅ Framework operational
- ✅ Ready to move on to P1 or next test

### Partial Success (Some Pass, Some Fail)
- ✅ Framework operational
- ✅ Identify which components work
- ⚠️ Document failures
- 📋 Plan P1 implementation based on gaps

### Failures (Tests Don't Run)
- 🔧 Likely P0 issues (operator, CNI socket)
- ⚠️ Check diagnostics: `bash scripts/verify-p0-status.sh`
- 📋 Reference troubleshooting guide: P0_IMPLEMENTATION_PLAN.md

## How to Analyze Results

After each test suite runs, capture:

### 1. Test Summary
```bash
# Grep test results from output
grep -i "passed\|failed\|skipped" <test_output>
```

### 2. Error Patterns
```bash
# Look for common errors
grep -i "error\|panic\|timeout" <test_output>
```

### 3. Component Status
```bash
# Check cluster state
export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
kubectl get pods -n kube-system
kubectl get crd | grep cilium | wc -l
```

### 4. Logs
```bash
# Agent logs
kubectl logs -n kube-system -l k8s-app=cilium | tail -100

# Operator logs
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator

# Test framework logs (from ginkgo output)
```

## P1 Implementation Triggers

Based on test results, we'll implement P1 features:

**If K8sDatapathServicesTest fails**:
- Implement: Service subsystem (2-3 weeks)
- Reference: SERVICE_IMPLEMENTATION_SPEC.md
- Components: eBPF maps, service observer, endpoint manager

**If K8sAgentPolicyTest fails**:
- Implement: Policy enforcement (1-2 weeks)
- Depends on: Service subsystem
- Components: Policy controller, eBPF policy rules

**If K8sHubbleTest fails**:
- Implement: Observability (1-2 weeks)
- Lower priority: May work with upstream operator
- Components: Hubble API, flow reporting

## Real-Time Monitoring

While tests run, monitor in another terminal:

```bash
# Watch pods coming up
watch -n 2 'kubectl get pods -n kube-system'

# Watch node status
watch kubectl get nodes

# Monitor resource usage
watch -n 5 'kubectl top nodes; echo "---"; kubectl top pods -n kube-system'

# Monitor Cilium status
kubectl exec -n kube-system <agent-pod> -- cilium status
```

## Documentation to Generate

After running integration gates (#17), create:

### 1. Integration Test Results Report
- Summary of passed/failed tests
- Error categorization
- Component status assessment
- Comparison to expectations

### 2. P1 Implementation Roadmap
- Priority ordered list of what to implement
- Estimated timeline for each component
- Dependencies between components
- Resource requirements

### 3. Diagnostic Data
- Test logs (saved)
- Pod descriptions (saved)
- Error traces (categorized)
- Performance metrics (if available)

## Timeline Estimate

```
Total time to complete #17 integration gates:

Option 1 (Sequential, recommended):
  Build + first test:  30 min
  Second test:         20 min
  Third test:          20 min
  Analysis:           10 min
  ──────────────────
  Total:              80 min (1.5 hours)

Option 2 (All at once):
  Build + all tests:  60 min
  Analysis:           10 min
  ──────────────────
  Total:              70 min (1 hour)

Option 3 (Quick validation):
  Build + FQDN:       30 min
  Analysis:            5 min
  ──────────────────
  Total:              35 min (quick check)
```

## Next: Task #18 (Fix Integration Blockers)

Once #17 is complete, #18 will:
1. Categorize failures
2. Prioritize implementations
3. Create detailed implementation tasks
4. Execute P1 service subsystem work

The severity of #18 depends on #17 results:
- If most tests pass: Focus on quick wins
- If many tests fail: Plan major P1 implementation (2-3 weeks)
- If tests don't run: Debug P0 issues first

## Commands Reference

```bash
# Quick validation (fastest)
just run K8sFQDNTest

# Full services validation
just run K8sDatapathServicesTest

# Full policies validation
just run K8sAgentPolicyTest

# All in one
just test-all-sequential 90m

# Check P0 status anytime
bash scripts/verify-p0-status.sh

# View current cluster status
export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
kubectl get pods -n kube-system
kubectl get crd | grep cilium

# View agent logs
kubectl logs -n kube-system -l k8s-app=cilium --tail=100
```

## Success Criteria for #17

✅ Integration gates executed without hanging  
✅ Real pass/fail results obtained  
✅ No infrastructure errors blocking tests  
✅ Clear error messages for failures  
✅ Results analyzed and documented  
✅ Path to #18 (blockers) identified  

---

**Status**: Ready to execute immediately  

**Next**: Run `just run K8sFQDNTest` to start gathering integration test data

**Follow-up**: Document results and plan #18 implementation based on findings
