#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
cd "$ROOT_DIR"

# Configuration
CILIUM_REPO=${CILIUM_REPO:-/var/home/james/dev/cilium}
IMAGE_PREFIX=${IMAGE_PREFIX:-localhost:5000/seriousum}
IMAGE_TAG=${IMAGE_TAG:-local}
TEST_TIMEOUT=${TEST_TIMEOUT:-2h}

# Test suites to run
declare -a TEST_SUITES=(
  "K8sAgentFQDNTest"
  "K8sDatapathServicesTest"
  "K8sAgentPolicyTest"
)

# Kind cluster names
declare -a CLUSTER_NAMES=(
  "kind-test-fqdn"
  "kind-test-services"
  "kind-test-policy"
)

# Output directories
OUTPUT_DIR=${OUTPUT_DIR:-$ROOT_DIR/target/parallel-test-results}
mkdir -p "$OUTPUT_DIR"

# Functions
cleanup_clusters() {
  echo "==> Cleaning up test clusters..."
  for cluster in "${CLUSTER_NAMES[@]}"; do
    echo "Deleting cluster $cluster..."
    kind delete cluster --name "$cluster" 2>/dev/null || true
  done
}

run_test_suite() {
  local suite_name=$1
  local cluster_name=$2
  local output_file=$3
  
  echo "==> [$suite_name] Starting test on cluster $cluster_name"
  
  {
    bash "$ROOT_DIR/scripts/run-cilium-kind-test.sh" \
      --focus "$suite_name" \
      --kind-cluster "$cluster_name" \
      --skip-build \
      --skip-dropin \
      --test-timeout "$TEST_TIMEOUT" \
      2>&1
  } | tee "$output_file"
  
  local exit_code=${PIPESTATUS[0]}
  echo "==> [$suite_name] Test completed with exit code: $exit_code"
  return $exit_code
}

# Trap for cleanup on interrupt
trap cleanup_clusters EXIT

# Start all tests in parallel
declare -a PIDS
declare -a OUTPUT_FILES

echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║         Starting Parallel Test Execution (3 suites simultaneously)           ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo ""
echo "Test Suites:"
for i in "${!TEST_SUITES[@]}"; do
  suite="${TEST_SUITES[$i]}"
  cluster="${CLUSTER_NAMES[$i]}"
  output_file="$OUTPUT_DIR/${suite}-results.log"
  OUTPUT_FILES[$i]="$output_file"
  
  echo "  [$((i+1))] $suite → cluster: $cluster → output: $output_file"
  
  # Run test in background
  run_test_suite "$suite" "$cluster" "$output_file" &
  PIDS[$i]=$!
done

echo ""
echo "PIDs: ${PIDS[@]}"
echo "Waiting for all tests to complete..."
echo ""

# Wait for all tests and collect exit codes
declare -a EXIT_CODES
for i in "${!PIDS[@]}"; do
  pid=${PIDS[$i]}
  suite=${TEST_SUITES[$i]}
  
  if wait $pid 2>/dev/null; then
    EXIT_CODES[$i]=0
    echo "✓ $suite completed successfully"
  else
    EXIT_CODES[$i]=$?
    echo "✗ $suite failed with exit code ${EXIT_CODES[$i]}"
  fi
done

echo ""
echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║                         Test Results Summary                                ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo ""

# Aggregate results
passed=0
failed=0

for i in "${!TEST_SUITES[@]}"; do
  suite=${TEST_SUITES[$i]}
  exit_code=${EXIT_CODES[$i]}
  output_file=${OUTPUT_FILES[$i]}
  
  if [ $exit_code -eq 0 ]; then
    echo "✅ $suite PASSED"
    ((passed++))
  else
    echo "❌ $suite FAILED (exit: $exit_code)"
    ((failed++))
  fi
  
  # Extract summary from output
  if [ -f "$output_file" ]; then
    echo "   Output: $output_file"
    if grep -q "FAIL\|Failed" "$output_file"; then
      # Try to extract test counts
      local summary=$(grep -E "Ran.*Specs" "$output_file" | tail -1 || echo "")
      if [ -n "$summary" ]; then
        echo "   $summary"
      fi
    fi
  fi
done

echo ""
echo "Summary: $passed passed, $failed failed"
echo "Output: $OUTPUT_DIR"

# Return success only if all tests passed
if [ $failed -eq 0 ]; then
  echo ""
  echo "✅ All parallel tests passed!"
  exit 0
else
  echo ""
  echo "❌ Some tests failed. Review logs in $OUTPUT_DIR"
  exit 1
fi
