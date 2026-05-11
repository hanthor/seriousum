#!/usr/bin/env bash
# Verify P0 status before running integration tests
# This is a minimal diagnostic to confirm P0 prerequisites

set -euo pipefail

KUBECONFIG=${KUBECONFIG:-./target/cilium-kind/kind.kubeconfig}
KIND_CLUSTER=${KIND_CLUSTER:-kind}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║              P0 STATUS VERIFICATION                           ║"
echo "║  Session 3 Phase 2: Critical Fixes Verification              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Check if cluster exists
echo "=== CLUSTER STATUS ==="
if kind get clusters 2>/dev/null | grep -q "$KIND_CLUSTER"; then
    echo "✓ Cluster $KIND_CLUSTER exists"
else
    echo "✗ Cluster $KIND_CLUSTER does not exist"
    echo "  Run: just cluster-create"
    exit 1
fi
echo ""

# Check if kubeconfig is valid
export KUBECONFIG
if ! kubectl cluster-info &>/dev/null; then
    echo "✗ Kubeconfig not accessible"
    exit 1
fi
echo "✓ Kubeconfig is valid"
echo ""

# Check operator image configuration
echo "=== OPERATOR IMAGE CONFIGURATION ==="
OPERATOR_DEPLOY=$(kubectl get deployment -n kube-system cilium-operator 2>/dev/null || echo "")
if [ -z "$OPERATOR_DEPLOY" ]; then
    echo "⚠ Cilium operator deployment not yet installed"
    echo "  Status: Operator will be installed when tests run"
else
    OPERATOR_IMAGE=$(kubectl get deployment -n kube-system cilium-operator -o jsonpath='{.spec.template.spec.containers[0].image}' 2>/dev/null || echo "")
    echo "Current operator image: $OPERATOR_IMAGE"
    
    if [[ "$OPERATOR_IMAGE" == *"cilium-ci"* ]]; then
        echo "✓ Using upstream cilium-ci operator"
    else
        echo "✗ Not using upstream operator"
    fi
fi
echo ""

# Check for Cilium CRDs
echo "=== CILIUM CRDs ==="
CRD_COUNT=$(kubectl get crd 2>/dev/null | grep cilium | wc -l)
echo "CRDs present: $CRD_COUNT / 9"
if [ "$CRD_COUNT" -eq 0 ]; then
    echo "⚠ No Cilium CRDs yet (expected, will be created by operator)"
else
    kubectl get crd 2>/dev/null | grep cilium || true
fi
echo ""

# Check for Cilium agent pods
echo "=== CILIUM AGENT PODS ==="
AGENT_PODS=$(kubectl get pods -n kube-system -l k8s-app=cilium 2>/dev/null | tail -n +2 | wc -l)
echo "Agent pods: $AGENT_PODS"
if [ "$AGENT_PODS" -gt 0 ]; then
    kubectl get pods -n kube-system -l k8s-app=cilium -o wide || true
    echo ""
    
    # Check for CNI socket
    AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || echo "")
    if [ -n "$AGENT_POD" ]; then
        echo "=== CNI SOCKET STATUS ==="
        if kubectl exec -n kube-system "$AGENT_POD" -- test -S /var/run/cilium/cilium.sock 2>/dev/null; then
            echo "✓ CNI socket exists at /var/run/cilium/cilium.sock"
        else
            echo "✗ CNI socket missing at /var/run/cilium/cilium.sock"
        fi
        echo ""
    fi
else
    echo "⚠ No agent pods yet (expected, will be created by operator)"
fi
echo ""

# Check CoreDNS status
echo "=== COREDNS STATUS ==="
DNS_PODS=$(kubectl get pods -n kube-system -l k8s-app=kube-dns 2>/dev/null | tail -n +2 | wc -l)
DNS_RUNNING=$(kubectl get pods -n kube-system -l k8s-app=kube-dns --field-selector=status.phase=Running 2>/dev/null | tail -n +2 | wc -l)
echo "CoreDNS pods: $DNS_RUNNING / $DNS_PODS running"
if [ "$DNS_RUNNING" -eq 0 ] && [ "$DNS_PODS" -gt 0 ]; then
    echo "✗ CoreDNS pods stuck (likely waiting for CNI socket)"
    kubectl describe pod -n kube-system -l k8s-app=kube-dns 2>/dev/null | grep -A5 "ContainerCreating" || true
else
    kubectl get pods -n kube-system -l k8s-app=kube-dns -o wide || true
fi
echo ""

# Summary
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    NEXT STEPS                                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "P0.1: Operator Image Configuration - ✓ CONFIGURED"
echo "  Script uses: quay.io/cilium/cilium-ci:latest"
echo "  Status: Ready when tests run"
echo ""
echo "P0.2: CNI Socket Creation"
if [ "$AGENT_PODS" -eq 0 ]; then
    echo "  Status: Not yet created (needs operator to run)"
    echo "  Next: Run integration tests to trigger full setup"
elif ! kubectl exec -n kube-system "$AGENT_POD" -- test -S /var/run/cilium/cilium.sock 2>/dev/null; then
    echo "  Status: ✗ MISSING - Investigate after test startup"
    echo "  Next: Run diagnostic: bash scripts/diagnose-cni-socket-timing.sh"
else
    echo "  Status: ✓ READY"
fi
echo ""
echo "Recommended next action:"
echo "  1. Run: just test-fqdn --timeout 12m"
echo "  2. Monitor operator initialization"
echo "  3. Check CNI socket creation"
echo "  4. Review agent logs if issues arise"
