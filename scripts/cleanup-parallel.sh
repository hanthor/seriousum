#!/usr/bin/env bash
set -euo pipefail

echo "╔══════════════════════════════════════════════════════════════════════════════╗"
echo "║                    Cleaning Up Parallel Test Resources                      ║"
echo "╚══════════════════════════════════════════════════════════════════════════════╝"
echo ""

# Stop any running tests
echo "==> Stopping running tests..."
pkill -9 -f "ginkgo\|run-cilium-kind-test" 2>/dev/null || true

# Delete test clusters
echo "==> Deleting test clusters..."
for cluster in kind-test-fqdn kind-test-services kind-test-policy; do
  if kind get clusters 2>/dev/null | grep -q "^$cluster$"; then
    echo "  Deleting $cluster..."
    kind delete cluster --name "$cluster" 2>/dev/null || true
  fi
done

# Clean up temp files
echo "==> Cleaning up temporary files..."
rm -f /tmp/p0_*.log /tmp/parallel_*.log

# Report
echo ""
echo "✅ Cleanup complete!"
echo ""
echo "To restart, run:"
echo "  bash scripts/run-parallel-test-suites.sh"
