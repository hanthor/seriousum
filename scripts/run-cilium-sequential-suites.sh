#!/usr/bin/env bash
set -euo pipefail

# Run multiple Cilium integration test suites sequentially on a single kind cluster
# Avoids resource exhaustion from parallel cluster creation

ROOT_DIR=$(git rev-parse --show-toplevel)
cd "$ROOT_DIR"

CILIUM_REPO=${CILIUM_REPO:-/var/home/james/dev/cilium}
IMAGE_PREFIX=${IMAGE_PREFIX:-localhost:5000/seriousum}
IMAGE_TAG=${IMAGE_TAG:-local}
KIND_CLUSTER=${KIND_CLUSTER:-kind}
BIN_DIR=${BIN_DIR:-$ROOT_DIR/target/cilium-dropin}
KUBECONFIG_FILE=${KUBECONFIG_FILE:-}
TEST_TIMEOUT=${TEST_TIMEOUT:-10m}
BUILD_IMAGES=${BUILD_IMAGES:-1}
INSTALL_DROPIN=${INSTALL_DROPIN:-1}
CLEANUP_BETWEEN=${CLEANUP_BETWEEN:-1}  # Uninstall Cilium between suites

# Test suites to run (focus patterns)
declare -a TEST_SUITES=(
  "K8sAgentFQDNTest"
  "K8sDatapathServicesTest"
  "K8sAgentPolicyTest"
)

usage() {
  cat <<'EOF'
Usage: scripts/run-cilium-sequential-suites.sh [options]

Run multiple Cilium integration test suites sequentially on a single kind cluster.
Avoids resource exhaustion and enables rapid iteration.

Options:
  -f, --focus SUITE          Only run specific suite (default: run all)
      --suites "S1,S2,S3"    Custom comma-separated list of suites
      --image-prefix PREFIX  Image prefix (default: localhost:5000/seriousum)
      --image-tag TAG        Image tag (default: local)
      --kind-cluster NAME    Kind cluster name (default: kind)
      --test-timeout DURATION Test timeout per suite (default: 10m)
      --skip-build           Skip image build
      --skip-dropin          Skip drop-in installation
      --no-cleanup           Don't uninstall Cilium between suites
  -h, --help                 Show this help

Environment Variables:
  TEST_TIMEOUT, BUILD_IMAGES, INSTALL_DROPIN, CLEANUP_BETWEEN, KUBECONFIG_FILE
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -f|--focus)
      TEST_SUITES=("$2")
      shift 2
      ;;
    --suites)
      IFS=',' read -ra TEST_SUITES <<< "$2"
      shift 2
      ;;
    --image-prefix)
      IMAGE_PREFIX=$2
      shift 2
      ;;
    --image-tag)
      IMAGE_TAG=$2
      shift 2
      ;;
    --kind-cluster)
      KIND_CLUSTER=$2
      shift 2
      ;;
    --test-timeout)
      TEST_TIMEOUT=$2
      shift 2
      ;;
    --skip-build)
      BUILD_IMAGES=0
      shift
      ;;
    --skip-dropin)
      INSTALL_DROPIN=0
      shift
      ;;
    --no-cleanup)
      CLEANUP_BETWEEN=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

if [ -z "$KUBECONFIG_FILE" ]; then
  KUBECONFIG_FILE="$ROOT_DIR/target/cilium-kind/$KIND_CLUSTER.kubeconfig"
fi

export KUBECONFIG="$KUBECONFIG_FILE"
export IMAGE_PREFIX IMAGE_TAG TEST_TIMEOUT

# Track results
declare -A SUITE_RESULTS
declare -A SUITE_TIMES
TOTAL_START=$(date +%s)

echo "========================================================================"
echo "Cilium Sequential Integration Test Runner"
echo "========================================================================"
echo ""
echo "Configuration:"
echo "  Kind Cluster:     $KIND_CLUSTER"
echo "  Image Prefix:     $IMAGE_PREFIX"
echo "  Image Tag:        $IMAGE_TAG"
echo "  Per-Suite Timeout: $TEST_TIMEOUT"
echo "  Test Suites:      ${TEST_SUITES[@]}"
echo "  Cleanup Between:  $([ "$CLEANUP_BETWEEN" = 1 ] && echo 'Yes' || echo 'No')"
echo ""

# Build images once if needed
if [ "$BUILD_IMAGES" = 1 ]; then
  echo "=== Building images (one-time) ==="
  "$ROOT_DIR/images/build-cilium-images.sh" > /dev/null
  echo "✓ Images built"
fi

# Install drop-in once if needed
if [ "$INSTALL_DROPIN" = 1 ]; then
  echo "=== Installing drop-in directory (one-time) ==="
  "$ROOT_DIR/scripts/build-cilium-dropin.sh" > /dev/null
  echo "✓ Drop-in installed"
fi

# Bootstrap cluster once
if ! kubectl cluster-info &>/dev/null; then
  echo "=== Bootstrapping kind cluster ==="
  mkdir -p "$(dirname "$KUBECONFIG_FILE")"
  "$CILIUM_REPO/contrib/scripts/kind.sh" "1" "1" "$KIND_CLUSTER" "" "" "" "" "" "$KUBECONFIG_FILE" &>/dev/null
  echo "✓ Cluster bootstrapped"
else
  echo "✓ Using existing cluster"
fi

echo ""
echo "========================================================================"
echo "Running test suites sequentially"
echo "========================================================================"
echo ""

# Run each suite
for suite in "${TEST_SUITES[@]}"; do
  echo "─────────────────────────────────────────────────────────────────────"
  echo "Suite: $suite"
  echo "─────────────────────────────────────────────────────────────────────"
  
  SUITE_START=$(date +%s)
  
  # Run the test suite
  if timeout "$(echo "$TEST_TIMEOUT" | sed 's/m/*60+/g; s/s//g' | bc)s" \
    "$ROOT_DIR/scripts/run-cilium-kind-test.sh" \
      --load \
      --skip-build \
      --focus "$suite" \
      --test-timeout "$TEST_TIMEOUT" \
      --no-bootstrap-cluster 2>&1 | tee "/tmp/${suite}-run.log"; then
    SUITE_RESULTS[$suite]="PASS"
  else
    SUITE_RESULTS[$suite]="FAIL"
  fi
  
  SUITE_END=$(date +%s)
  SUITE_DURATION=$((SUITE_END - SUITE_START))
  SUITE_TIMES[$suite]=$SUITE_DURATION
  
  echo "✓ ${suite}: ${SUITE_RESULTS[$suite]} (${SUITE_DURATION}s)"
  echo ""
  
  # Cleanup Cilium if requested
  if [ "$CLEANUP_BETWEEN" = 1 ]; then
    echo "Cleaning up Cilium for next suite..."
    kubectl delete namespace cilium-test 2>/dev/null || true
    kubectl delete namespace test-namespace 2>/dev/null || true
    kubectl delete namespace kube-system 2>/dev/null || true
    sleep 5
  fi
done

# Summary
TOTAL_END=$(date +%s)
TOTAL_DURATION=$((TOTAL_END - TOTAL_START))

echo ""
echo "========================================================================"
echo "Sequential Test Summary"
echo "========================================================================"
echo ""

PASS_COUNT=0
FAIL_COUNT=0

for suite in "${TEST_SUITES[@]}"; do
  result=${SUITE_RESULTS[$suite]:-UNKNOWN}
  duration=${SUITE_TIMES[$suite]:-0}
  
  if [ "$result" = "PASS" ]; then
    echo "✅ $suite: $result (${duration}s)"
    ((PASS_COUNT++))
  else
    echo "❌ $suite: $result (${duration}s)"
    ((FAIL_COUNT++))
  fi
done

echo ""
echo "Results: $PASS_COUNT passed, $FAIL_COUNT failed"
echo "Total Time: ${TOTAL_DURATION}s"
echo ""

if [ "$FAIL_COUNT" -eq 0 ]; then
  echo "✅ All suites passed!"
  exit 0
else
  echo "⚠️  Some suites failed. See logs above for details."
  exit 1
fi
