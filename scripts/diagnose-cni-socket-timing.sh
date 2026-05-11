#!/usr/bin/env bash
#
# Diagnose CNI Socket Timing Issues
# ================================
#
# This script performs comprehensive diagnostics on Cilium CNI socket creation
# timing and identifies why CoreDNS pods are stuck in ContainerCreating.
#
# Usage:
#   ./scripts/diagnose-cni-socket-timing.sh [--cluster NAME] [--output FILE]
#
# Requirements:
#   - kubectl configured and connected to cluster
#   - bash 4.0+
#   - Basic utilities: jq, sort, awk, date
#

set -euo pipefail

# Configuration
CLUSTER_NAME="${1:-kind}"
OUTPUT_FILE="${2:-.}/cni-socket-timing-report.txt}"
KUBECONFIG="${KUBECONFIG:-}"
NOW=$(date '+%Y-%m-%d %H:%M:%S')
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors for output (can be disabled with NO_COLOR)
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Utility functions
log_header() {
  local msg="$1"
  echo -e "${BLUE}=== ${msg} ===${NC}"
  echo "=== ${msg} ===" >> "$OUTPUT_FILE"
}

log_info() {
  local msg="$1"
  echo -e "${GREEN}[INFO]${NC} ${msg}"
  echo "[INFO] ${msg}" >> "$OUTPUT_FILE"
}

log_warn() {
  local msg="$1"
  echo -e "${YELLOW}[WARN]${NC} ${msg}"
  echo "[WARN] ${msg}" >> "$OUTPUT_FILE"
}

log_error() {
  local msg="$1"
  echo -e "${RED}[ERROR]${NC} ${msg}"
  echo "[ERROR] ${msg}" >> "$OUTPUT_FILE"
}

log_section() {
  local msg="$1"
  echo "" | tee -a "$OUTPUT_FILE"
  log_header "$msg"
}

# Initialize output file
{
  echo "CNI Socket Timing Diagnostic Report"
  echo "======================================"
  echo "Generated: $NOW"
  echo "Cluster: $CLUSTER_NAME"
  echo ""
} > "$OUTPUT_FILE"

# ============================================================================
# TASK 1: Check Operator Pod Status and Creation Times
# ============================================================================
log_section "TASK 1: Cilium Operator Pod Timeline"

{
  echo "Operator Deployment Status:"
  kubectl get deployment -n kube-system cilium-operator -o wide 2>&1 || echo "N/A"
  echo ""
  echo "Operator Pods:"
  kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o wide 2>&1 || echo "N/A"
  echo ""
  echo "Operator Pod Details (JSON):"
  kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o json 2>&1 | \
    jq -r '.items[] | "\(.metadata.name): created=\(.metadata.creationTimestamp), phase=\(.status.phase), ready=\(.status.conditions[]|select(.type=="Ready").status)"' || echo "N/A"
  echo ""
  echo "Operator Events:"
  kubectl get events -n kube-system --field-selector involvedObject.kind=Pod,involvedObject.name=~cilium-operator --sort-by='.firstTimestamp' 2>&1 | head -20 || echo "N/A"
} >> "$OUTPUT_FILE"

# Check for operator image pull issues
OPERATOR_STATUS=$(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o jsonpath='{.items[0].status.containerStatuses[0].state}' 2>/dev/null || echo "")
if echo "$OPERATOR_STATUS" | grep -q "ImagePull"; then
  log_error "Operator image pull issue detected"
  kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator -o jsonpath='{.items[0].status.containerStatuses[0].state}' >> "$OUTPUT_FILE"
else
  log_info "Operator pod status: OK (not in ImagePull state)"
fi

# ============================================================================
# TASK 2: Check Cilium Agent Pod Status and Health
# ============================================================================
log_section "TASK 2: Cilium Agent Pod Timeline and Health"

{
  echo "Agent DaemonSet Status:"
  kubectl get daemonset -n kube-system cilium -o wide 2>&1 || echo "N/A"
  echo ""
  echo "Agent Pods Status:"
  kubectl get pods -n kube-system -l k8s-app=cilium -o wide 2>&1 || echo "N/A"
  echo ""
  echo "Agent Pod Timing (Creation vs Ready):"
  kubectl get pods -n kube-system -l k8s-app=cilium -o json 2>&1 | \
    jq -r '.items[] | "\(.metadata.name): created=\(.metadata.creationTimestamp), ready=\(.status.conditions[]|select(.type=="Ready").lastTransitionTime), phase=\(.status.phase)"' || echo "N/A"
  echo ""
  echo "Agent Startup Probe Events:"
  kubectl get events -n kube-system --field-selector involvedObject.kind=Pod,involvedObject.name=~cilium --sort-by='.firstTimestamp' 2>&1 | grep -i "startup\|probe\|health" | head -20 || echo "No startup probe events"
} >> "$OUTPUT_FILE"

# Check agent health status
AGENT_READY=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].status.conditions[?(@.type=="Ready")].status}' 2>/dev/null || echo "Unknown")
if [ "$AGENT_READY" = "True" ]; then
  log_info "Agent pods are Ready"
elif [ "$AGENT_READY" = "False" ]; then
  log_warn "Agent pods are NOT Ready"
  PROBE_STATUS=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].status}' 2>/dev/null)
  echo "Probe details: $PROBE_STATUS" >> "$OUTPUT_FILE"
else
  log_warn "Agent pod status unknown"
fi

# ============================================================================
# TASK 3: Check CoreDNS Pod Status and CNI Events
# ============================================================================
log_section "TASK 3: CoreDNS Pod Status and CNI Socket Access Attempts"

{
  echo "CoreDNS Pods Status:"
  kubectl get pods -n kube-system -l k8s-app=kube-dns -o wide 2>&1 || echo "N/A"
  echo ""
  echo "CoreDNS Pod Events (Detailed):"
  kubectl describe pods -n kube-system -l k8s-app=kube-dns 2>&1 | grep -A 30 "Events:" || echo "N/A"
  echo ""
  echo "CoreDNS CNI Errors:"
  kubectl get events -n kube-system --field-selector involvedObject.kind=Pod,involvedObject.name=~coredns --sort-by='.lastTimestamp' 2>&1 | \
    grep -i "sandbox\|cni\|network\|socket" | head -20 || echo "No CNI errors found"
} >> "$OUTPUT_FILE"

# Check if CoreDNS is stuck in ContainerCreating
COREDNS_STATUS=$(kubectl get pods -n kube-system -l k8s-app=kube-dns -o jsonpath='{.items[0].status.phase}' 2>/dev/null || echo "Unknown")
if [ "$COREDNS_STATUS" = "Pending" ]; then
  log_warn "CoreDNS pods are PENDING (likely waiting for CNI socket)"
elif [ "$COREDNS_STATUS" = "Running" ]; then
  log_info "CoreDNS pods are RUNNING (CNI socket is accessible)"
else
  log_warn "CoreDNS pod status: $COREDNS_STATUS"
fi

# ============================================================================
# TASK 4: Verify Socket Location and Permissions
# ============================================================================
log_section "TASK 4: Socket Location and Accessibility"

{
  echo "Expected Socket Path: /var/run/cilium/cilium.sock"
  echo ""
  echo "Cilium Node Mount Points:"
  for NODE in $(kubectl get nodes -o jsonpath='{.items[].metadata.name}' 2>/dev/null); do
    echo "Node: $NODE"
    NODE_IP=$(kubectl get node "$NODE" -o jsonpath='{.status.addresses[?(@.type=="InternalIP")].address}')
    if [ -n "$NODE_IP" ]; then
      echo "  InternalIP: $NODE_IP"
    fi
  done
  echo ""
  echo "Checking Agent Pod Socket (via exec):"
  AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || echo "")
  if [ -n "$AGENT_POD" ]; then
    echo "Agent pod: $AGENT_POD"
    echo ""
    echo "Socket check:"
    kubectl exec -n kube-system "$AGENT_POD" -c cilium-agent -- \
      ls -la /var/run/cilium/cilium.sock 2>&1 || echo "[MISSING] Socket not found"
    echo ""
    echo "Directory check:"
    kubectl exec -n kube-system "$AGENT_POD" -c cilium-agent -- \
      ls -ld /var/run/cilium/ 2>&1 || echo "[ERROR] Directory inaccessible"
    echo ""
    echo "Mount points:"
    kubectl exec -n kube-system "$AGENT_POD" -c cilium-agent -- \
      mount | grep -E "cilium|run" 2>&1 || echo "[INFO] No cilium mounts"
  else
    echo "[ERROR] No agent pod found for socket verification"
  fi
} >> "$OUTPUT_FILE"

# ============================================================================
# TASK 5: Agent Initialization Logs
# ============================================================================
log_section "TASK 5: Agent Pod Logs and Socket Creation Traces"

{
  AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || echo "")
  if [ -n "$AGENT_POD" ]; then
    echo "Latest Agent Pod: $AGENT_POD"
    echo ""
    echo "=== Agent Logs (Last 100 lines) ==="
    kubectl logs -n kube-system "$AGENT_POD" -c cilium-agent --tail=100 2>&1 | head -100
    echo ""
    echo "=== Agent Previous Logs (if exists) ==="
    kubectl logs -n kube-system "$AGENT_POD" -c cilium-agent --previous 2>&1 | tail -50 || echo "[N/A] No previous logs"
    echo ""
    echo "=== Searching for socket-related messages ==="
    kubectl logs -n kube-system "$AGENT_POD" -c cilium-agent 2>&1 | \
      grep -i "socket\|listen\|/var/run\|healthz" | head -30 || echo "[INFO] No socket-related messages found"
  else
    echo "[ERROR] No agent pod found for log inspection"
  fi
} >> "$OUTPUT_FILE"

# ============================================================================
# TASK 6: CNI Configuration Verification
# ============================================================================
log_section "TASK 6: CNI Configuration and Binary Status"

{
  echo "Cilium CNI Configuration File:"
  echo "Expected: /etc/cni/net.d/05-cilium.conflist"
  echo ""
  
  AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || echo "")
  if [ -n "$AGENT_POD" ]; then
    echo "CNI Config from Agent Pod:"
    kubectl exec -n kube-system "$AGENT_POD" -c cilium-agent -- \
      cat /etc/cni/net.d/05-cilium.conflist 2>&1 | jq . 2>&1 || echo "[N/A] Config not readable"
    echo ""
    echo "CNI Binary Check:"
    kubectl exec -n kube-system "$AGENT_POD" -c cilium-agent -- \
      ls -la /usr/bin/cilium-cni 2>&1 || echo "[MISSING] CNI binary not found"
  fi
} >> "$OUTPUT_FILE"

# ============================================================================
# TASK 7: Cluster and Node Status
# ============================================================================
log_section "TASK 7: Cluster and Node Status"

{
  echo "Node Status:"
  kubectl get nodes -o wide 2>&1
  echo ""
  echo "Node Readiness Issues:"
  kubectl describe nodes 2>&1 | grep -A 5 "Not Ready" | head -30 || echo "[OK] All nodes ready"
  echo ""
  echo "System Resource Usage:"
  kubectl top nodes 2>&1 || echo "[N/A] Metrics not available"
  echo ""
  echo "Kube-System Pods Overview:"
  kubectl get pods -n kube-system -o wide 2>&1 | head -20
} >> "$OUTPUT_FILE"

# ============================================================================
# TASK 8: Timeline and Correlation
# ============================================================================
log_section "TASK 8: Event Timeline Correlation"

{
  echo "Sorted Event Timeline (Last 50 events):"
  kubectl get events --all-namespaces --sort-by='.lastTimestamp' 2>&1 | tail -50
  echo ""
  echo "Socket-related Events:"
  kubectl get events --all-namespaces 2>&1 | grep -i "socket\|cilium\|cni" | tail -20 || echo "[INFO] No socket events in recent history"
} >> "$OUTPUT_FILE"

# ============================================================================
# ANALYSIS SECTION
# ============================================================================
log_section "ANALYSIS SUMMARY"

{
  echo ""
  echo "Key Findings:"
  echo "============="
  echo ""
  
  # Check operator
  OP_STATUS=$(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator --no-headers 2>/dev/null | awk '{print $3}' | head -1)
  echo "[1] Operator Status: $OP_STATUS"
  
  # Check agent
  AG_STATUS=$(kubectl get pods -n kube-system -l k8s-app=cilium --no-headers 2>/dev/null | awk '{print $3}' | head -1)
  AG_READY=$(kubectl get pods -n kube-system -l k8s-app=cilium --no-headers 2>/dev/null | awk '{print $2}' | head -1)
  echo "[2] Agent Status: $AG_STATUS (Ready: $AG_READY)"
  
  # Check CoreDNS
  DNS_STATUS=$(kubectl get pods -n kube-system -l k8s-app=kube-dns --no-headers 2>/dev/null | awk '{print $3}' | head -1)
  echo "[3] CoreDNS Status: $DNS_STATUS"
  
  # Check socket
  AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || echo "")
  if [ -n "$AGENT_POD" ]; then
    SOCKET_EXISTS=$(kubectl exec -n kube-system "$AGENT_POD" -c cilium-agent -- test -S /var/run/cilium/cilium.sock 2>/dev/null && echo "YES" || echo "NO")
    echo "[4] Socket Exists: $SOCKET_EXISTS"
  else
    echo "[4] Socket Exists: UNKNOWN (no agent pod)"
  fi
  
  # Nodes ready
  NODES_READY=$(kubectl get nodes --no-headers 2>/dev/null | grep -c "Ready" || echo "0")
  NODES_TOTAL=$(kubectl get nodes --no-headers 2>/dev/null | wc -l)
  echo "[5] Nodes Ready: $NODES_READY/$NODES_TOTAL"
  
  echo ""
  echo "Root Cause Hypothesis:"
  echo "======================"
  if [ "$OP_STATUS" != "Running" ] && [ "$OP_STATUS" != "" ]; then
    echo "→ Operator not running ($OP_STATUS): May prevent agent initialization"
  fi
  
  if echo "$AG_READY" | grep -q "0/"; then
    echo "→ Agent pods not ready: Startup probe likely failing"
  fi
  
  if [ "$DNS_STATUS" = "Pending" ]; then
    echo "→ CoreDNS stuck in Pending: CNI socket unavailable"
  fi
  
  echo ""
  echo "Recommendations:"
  echo "================"
  echo "1. Fix operator image/auth issues (if present)"
  echo "2. Debug agent startup probe failures"
  echo "3. Verify socket is created after agent startup"
  echo "4. Increase CNI socket timeout if needed"
  echo "5. Check node resources (CPU, memory, disk)"
} >> "$OUTPUT_FILE"

# ============================================================================
# SUMMARY
# ============================================================================
echo ""
log_section "Diagnostic Complete"
echo "Report saved to: $OUTPUT_FILE"
log_info "Total entries in report: $(wc -l < "$OUTPUT_FILE")"

# Display key metrics
echo ""
echo "Quick Status:"
OP_COUNT=$(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator --no-headers 2>/dev/null | wc -l)
AG_COUNT=$(kubectl get pods -n kube-system -l k8s-app=cilium --no-headers 2>/dev/null | wc -l)
DNS_COUNT=$(kubectl get pods -n kube-system -l k8s-app=kube-dns --no-headers 2>/dev/null | wc -l)
NODES=$(kubectl get nodes --no-headers 2>/dev/null | wc -l)

echo "  Operator Pods: $OP_COUNT"
echo "  Agent Pods: $AG_COUNT"
echo "  DNS Pods: $DNS_COUNT"
echo "  Nodes: $NODES"

exit 0
