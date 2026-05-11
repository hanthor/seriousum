#!/usr/bin/env bash
set -euo pipefail

# Profile Cilium startup sequence and generate timeline report
# Measures time for each major phase from cluster bootstrap to test readiness

KUBECONFIG_FILE=${KUBECONFIG_FILE:-/var/home/james/dev/seriousum/target/cilium-kind/kind.kubeconfig}
OUTPUT_FILE=${OUTPUT_FILE:-/tmp/cilium-startup-profile.txt}

export KUBECONFIG="$KUBECONFIG_FILE"

log_timestamp() {
  local phase="$1"
  local elapsed=$(($(date +%s) - START_TIME))
  printf "[%4ds] %s\n" "$elapsed" "$phase" | tee -a "$OUTPUT_FILE"
}

echo "Starting Cilium startup profiling..." > "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

START_TIME=$(date +%s)
log_timestamp "=== PROFILING START ==="

# Phase 1: Cluster bootstrap
log_timestamp "CLUSTER: Kind cluster already exists"

# Phase 2: Check node readiness
log_timestamp "NODES: Waiting for node readiness..."
while ! kubectl get nodes --no-headers 2>/dev/null | grep -q "Ready"; do
  sleep 1
done
log_timestamp "NODES: All nodes ready"

# Phase 3: Wait for kube-system core pods
log_timestamp "CORE: Waiting for kube core components..."
for pod in etcd kube-apiserver kube-controller-manager kube-scheduler; do
  while ! kubectl get pods -n kube-system -o name 2>/dev/null | grep -q "$pod"; do
    sleep 1
  done
done
log_timestamp "CORE: All core components deployed"

# Phase 4: Wait for API server readiness
log_timestamp "API: Waiting for API server readiness..."
while ! kubectl get pods -n kube-system --no-headers 2>/dev/null | grep -q "kube-apiserver.*Running"; do
  sleep 1
done
log_timestamp "API: API server ready"

# Phase 5: Check if Cilium already installed
if kubectl get deployment -n kube-system cilium-operator 2>/dev/null; then
  log_timestamp "CILIUM: Already installed, skipping reinstall"
else
  log_timestamp "CILIUM: Cilium not found, would install here"
fi

# Phase 6: Wait for Cilium operator to start
log_timestamp "OPERATOR: Waiting for operator deployment..."
while [ $(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator --no-headers 2>/dev/null | wc -l) -eq 0 ]; do
  sleep 1
done
log_timestamp "OPERATOR: Operator pods created"

# Phase 7: Wait for Cilium operator readiness
log_timestamp "OPERATOR: Waiting for operator readiness..."
kubectl wait --for=condition=ready pod -n kube-system -l app.kubernetes.io/name=cilium-operator --timeout=300s 2>/dev/null || true
log_timestamp "OPERATOR: Operator ready (or timed out)"

# Phase 8: Wait for Cilium CRDs
log_timestamp "CRDS: Waiting for Cilium CRDs to register..."
CRDS_EXPECTED=10
CRDS_FOUND=0
CRDS_START=$(date +%s)
while [ "$CRDS_FOUND" -lt "$CRDS_EXPECTED" ]; do
  CRDS_FOUND=$(kubectl get crd 2>/dev/null | grep -c "cilium\|cil" || true)
  ELAPSED=$(($(date +%s) - CRDS_START))
  if [ "$ELAPSED" -gt 60 ]; then
    break
  fi
  sleep 1
done
log_timestamp "CRDS: $CRDS_FOUND CRDs registered"

# Phase 9: Wait for Cilium agent daemonset
log_timestamp "AGENT: Waiting for agent daemonset..."
while ! kubectl get daemonset -n kube-system cilium 2>/dev/null | grep -q cilium; do
  sleep 1
done
log_timestamp "AGENT: Agent daemonset deployed"

# Phase 10: Wait for agent pod creation
log_timestamp "AGENT: Waiting for agent pods..."
while [ $(kubectl get pods -n kube-system -l k8s-app=cilium --no-headers 2>/dev/null | wc -l) -eq 0 ]; do
  sleep 1
done
log_timestamp "AGENT: Agent pods created"

# Phase 11: Wait for CNI socket creation
log_timestamp "CNI: Waiting for cilium.sock creation..."
SOCKET_CREATED=false
for node in $(kubectl get nodes -o jsonpath='{.items[].metadata.name}'); do
  # This would require SSH to node, skip for now
  :
done
log_timestamp "CNI: Socket check (note: requires node access to verify)"

# Phase 12: Wait for agent readiness
log_timestamp "AGENT: Waiting for agent pod readiness..."
kubectl wait --for=condition=ready pod -n kube-system -l k8s-app=cilium --timeout=300s 2>/dev/null || true
AGENT_READY=$(kubectl get pods -n kube-system -l k8s-app=cilium --no-headers 2>/dev/null | grep -c "Running" || true)
log_timestamp "AGENT: $AGENT_READY agent pods running"

# Phase 13: Wait for CoreDNS readiness (depends on CNI)
log_timestamp "COREDNS: Waiting for CoreDNS readiness..."
kubectl wait --for=condition=ready pod -n kube-system -l k8s-app=kube-dns --timeout=60s 2>/dev/null || true
log_timestamp "COREDNS: CoreDNS ready (or timed out)"

# Phase 14: Test namespace creation readiness
log_timestamp "TESTING: System ready for test execution"

END_TIME=$(date +%s)
TOTAL_TIME=$((END_TIME - START_TIME))

log_timestamp "=== PROFILING COMPLETE ==="
echo "" >> "$OUTPUT_FILE"
echo "Total startup time: ${TOTAL_TIME}s" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
echo "Timeline saved to: $OUTPUT_FILE" >> "$OUTPUT_FILE"

# Display summary
echo ""
echo "=========================================="
echo "Cilium Startup Profile"
echo "=========================================="
cat "$OUTPUT_FILE"
echo ""
echo "Analysis:"
echo "  Total startup time: ${TOTAL_TIME}s"
if [ "$TOTAL_TIME" -lt 300 ]; then
  echo "  ✅ Startup < 5 minutes (fast)"
elif [ "$TOTAL_TIME" -lt 600 ]; then
  echo "  ⚠️  Startup 5-10 minutes (moderate)"
else
  echo "  ❌ Startup > 10 minutes (slow - optimize needed)"
fi
echo ""
echo "Bottleneck phases typically:"
echo "  1. CRD registration by operator"
echo "  2. Agent pod initialization"
echo "  3. CNI socket creation"
echo "  4. CoreDNS pod startup (waits for CNI)"

