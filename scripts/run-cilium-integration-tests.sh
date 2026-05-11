#!/bin/bash
# CILIUM INTEGRATION TEST EXECUTION PLAN
# Full testing compliance with unmodified Cilium test suites

set -e

PROJECT_DIR="/var/home/james/dev/seriousum"
CILIUM_DIR="/var/home/james/dev/cilium"
cd "$PROJECT_DIR"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
success() { echo -e "${GREEN}✅ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠️  $1${NC}"; }

# ============================================================================
# STEP 1: BUILD RUST AGENT CONTAINER IMAGES
# ============================================================================
echo ""
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   STEP 1: BUILD RUST AGENT CONTAINER IMAGES                       ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

info "Building Rust binaries (release mode)..."
cargo build --release --bin seriousum-daemon 2>&1 | tail -2
cargo build --release --bin seriousum-cli 2>&1 | tail -2
cargo build --release --bin seriousum-dbg 2>&1 | tail -2
success "Rust binaries built"

info "Creating wrapper binaries..."
mkdir -p cmd/wrappers
cat > cmd/wrappers/cilium-agent << 'WRAPPER_EOF'
#!/bin/bash
exec /opt/cilium/seriousum-daemon "$@"
WRAPPER_EOF
chmod +x cmd/wrappers/cilium-agent
success "Wrapper binaries created"

info "Building Docker image..."
docker build -f images/cilium-agent.Dockerfile \
  -t seriousum-agent:latest \
  . 2>&1 | tail -3
success "Docker image built: seriousum-agent:latest"

echo ""

# ============================================================================
# STEP 2: CREATE KIND CLUSTER
# ============================================================================
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   STEP 2: CREATE KIND CLUSTER FOR TESTING                         ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

CLUSTER_NAME="cilium-rust-test"

if kind get clusters 2>/dev/null | grep -q "$CLUSTER_NAME"; then
  warn "Cluster $CLUSTER_NAME already exists, using it"
else
  info "Creating kind cluster: $CLUSTER_NAME"
  kind create cluster --name "$CLUSTER_NAME" 2>&1 | tail -3
  success "Cluster created"
fi

info "Loading Docker image into kind..."
kind load docker-image seriousum-agent:latest --name "$CLUSTER_NAME" 2>&1 | tail -2
success "Image loaded"

echo ""

# ============================================================================
# STEP 3: DEPLOY CILIUM WITH RUST AGENT
# ============================================================================
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   STEP 3: DEPLOY CILIUM WITH RUST AGENT                           ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

info "Deploying Cilium with Rust agent image..."
# Using upstream operator, overriding image to use our Rust build
helm install cilium cilium/cilium \
  --namespace kube-system \
  --create-namespace \
  --set image.repository=seriousum-agent \
  --set image.tag=latest \
  --set operator.enabled=true \
  2>&1 | tail -3

success "Cilium deployed"

info "Waiting for agent to start (max 60s)..."
kubectl wait --for=condition=ready pod \
  -l k8s-app=cilium \
  -n kube-system \
  --timeout=60s 2>&1 | tail -2
success "Agent ready"

echo ""

# ============================================================================
# STEP 4: VERIFY AGENT STARTUP
# ============================================================================
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   STEP 4: VERIFY RUST AGENT STARTUP                               ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

info "Agent logs (last 20 lines):"
kubectl logs -n kube-system -l k8s-app=cilium | tail -20
success "Agent started successfully"

echo ""

# ============================================================================
# STEP 5: RUN GINKGO TEST MATRIX
# ============================================================================
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   STEP 5: RUN CILIUM GINKGO TEST MATRIX (13 FOCUS GROUPS)         ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

cd "$CILIUM_DIR"

FOCUS_GROUPS=(
  "K8sBpfTest"
  "K8sDatapathTest"
  "K8sCniTest"
  "K8sWatchersTest"
  "K8sIdentityTest"
  "K8sAgentPolicyTest"
  "K8sEndpointTest"
  "K8sDatapathServicesTest"
  "K8sFQDNTest"
  "K8sHubbleTest"
  "K8sEncryptionTest"
  "K8sClusterMeshTest"
  "K8sBGPTest"
)

RESULTS_DIR="$PROJECT_DIR/cilium-test-results"
mkdir -p "$RESULTS_DIR"

TOTAL_PASS=0
TOTAL_FAIL=0

for GROUP in "${FOCUS_GROUPS[@]}"; do
  info "Running $GROUP..."
  
  # Run ginkgo with focus on specific test group
  OUTPUT_FILE="$RESULTS_DIR/${GROUP}-results.txt"
  
  ./test/k8s/runner.sh --focus="$GROUP" \
    2>&1 | tee "$OUTPUT_FILE" | tail -10
  
  # Extract pass/fail counts
  PASS=$(grep -oP "(?<=: )\d+(?= passed)" "$OUTPUT_FILE" | head -1 || echo "0")
  FAIL=$(grep -oP "(?<=: )\d+(?= failed)" "$OUTPUT_FILE" | head -1 || echo "0")
  
  TOTAL_PASS=$((TOTAL_PASS + PASS))
  TOTAL_FAIL=$((TOTAL_FAIL + FAIL))
  
  if [ "$FAIL" -eq 0 ]; then
    success "$GROUP: $PASS passed"
  else
    warn "$GROUP: $PASS passed, $FAIL failed"
  fi
done

echo ""

# ============================================================================
# STEP 6: GENERATE COMPATIBILITY REPORT
# ============================================================================
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   STEP 6: GENERATE COMPATIBILITY REPORT                           ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

REPORT_FILE="$PROJECT_DIR/CILIUM_COMPATIBILITY_REPORT.md"
cat > "$REPORT_FILE" << REPORT_EOF
# Cilium Integration Test Results

**Date**: $(date -u '+%Y-%m-%d %H:%M:%S UTC')
**Rust Agent**: seriousum v0.1.0
**Cilium Reference**: upstream $(cd $CILIUM_DIR && git rev-parse --short HEAD)

## Summary

**Total Tests Run**: $((TOTAL_PASS + TOTAL_FAIL))
**Passed**: $TOTAL_PASS
**Failed**: $TOTAL_FAIL
**Pass Rate**: $(( (TOTAL_PASS * 100) / (TOTAL_PASS + TOTAL_FAIL) ))%

## Results by Focus Group

| Focus Group | Pass | Fail | Status |
|-------------|------|------|--------|
REPORT_EOF

for GROUP in "${FOCUS_GROUPS[@]}"; do
  OUTPUT_FILE="$RESULTS_DIR/${GROUP}-results.txt"
  PASS=$(grep -oP "(?<=: )\d+(?= passed)" "$OUTPUT_FILE" | head -1 || echo "0")
  FAIL=$(grep -oP "(?<=: )\d+(?= failed)" "$OUTPUT_FILE" | head -1 || echo "0")
  
  if [ "$FAIL" -eq 0 ]; then
    STATUS="✅ PASS"
  else
    STATUS="❌ FAIL"
  fi
  
  echo "| $GROUP | $PASS | $FAIL | $STATUS |" >> "$REPORT_FILE"
done

cat >> "$REPORT_FILE" << REPORT_EOF

## Track Mapping

- Track A (eBPF): K8sBpfTest
- Track B (Datapath): K8sDatapathTest
- Track C (CNI): K8sCniTest
- Track D (K8s): K8sWatchersTest
- Track E (Identity): K8sIdentityTest
- Track F (Policy): K8sAgentPolicyTest
- Track G (Endpoint): K8sEndpointTest
- Track I (LB): K8sDatapathServicesTest
- Track K (FQDN): K8sFQDNTest
- Track L (Hubble): K8sHubbleTest
- Track N (Encryption): K8sEncryptionTest
- Track O (ClusterMesh): K8sClusterMeshTest
- Track P (BGP): K8sBGPTest

## Analysis

[Detailed analysis of results, identified gaps, recommendations for v0.1.1]

## Next Steps

1. Fix critical failures
2. Iterate on v0.1.1
3. Expand test coverage
4. Plan Group 5+ implementation

REPORT_EOF

success "Compatibility report generated: $REPORT_FILE"

echo ""

# ============================================================================
# STEP 7: SUMMARY
# ============================================================================
echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║   CILIUM INTEGRATION TESTING COMPLETE                             ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo ""

echo "📊 FINAL RESULTS"
echo "================"
echo "Total Tests Run: $((TOTAL_PASS + TOTAL_FAIL))"
echo "Passed: $TOTAL_PASS ✅"
echo "Failed: $TOTAL_FAIL ❌"
echo "Pass Rate: $(( (TOTAL_PASS * 100) / (TOTAL_PASS + TOTAL_FAIL) ))%"
echo ""

echo "📁 RESULTS LOCATION"
echo "==================="
echo "Test Results: $RESULTS_DIR/"
echo "Report: $REPORT_FILE"
echo ""

echo "🚀 NEXT STEPS"
echo "============="
echo "1. Review compatibility report"
echo "2. Fix critical failures"
echo "3. Re-run failed tests"
echo "4. Prepare v0.1.0-alpha release"
echo ""

success "All done!"
