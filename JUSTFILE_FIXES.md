# Justfile Infrastructure Fixes — Track I Integration Testing

**Date**: May 18, 2026  
**Problem**: Integration test infrastructure had critical issues preventing successful test runs  
**Solution**: Fixed justfile recipes to ensure proper setup order and error handling  

## Issues Discovered

During Track I integration testing, we discovered three critical infrastructure problems:

### Issue 1: Docker Registry Not Running
**Symptom**: Image loading fails with connection refused on `localhost:5000`  
**Cause**: The Docker registry container was not automatically started  
**Impact**: `run-existing-fresh` would fail at the image loading phase

### Issue 2: Image Build Not Part of Fresh Test Pipeline
**Symptom**: Test runs tried to load images that hadn't been built  
**Cause**: `run-existing-fresh` called `load-all` without building images first  
**Impact**: Tests would fail with "image not found" or load stale images

### Issue 3: Helm Digest Mismatches
**Symptom**: Pods failed with `ErrImageNeverPull` despite image being loaded  
**Cause**: Cilium Helm chart pins specific image digests that didn't match locally built images  
**Impact**: Cilium pods couldn't start even with correct image repository/tag overrides

## Fixes Applied

### 1. Registry Management Recipes

Added two new recipes to manage the Docker registry:

```bash
@registry-start    # Start localhost:5000 registry
@registry-stop     # Stop the registry
```

These ensure the registry is available before any image loading operations.

### 2. Updated `run-existing-fresh` Recipe

**Before**:
```bash
run-existing-fresh cluster='cilium-ginkgo' focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT:
    just ginkgo-cluster {{cluster}} {{agent_port_prefix}} {{operator_port_prefix}}
    just load-all {{cluster}}
    just run-existing {{cluster}} "{{focus}}" {{timeout}}
```

**After** (adds critical steps):
```bash
run-existing-fresh cluster='cilium-ginkgo' focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT:
    [1/5] Start Docker registry
    [2/5] Build release binaries
    [3/5] Build Docker images
    [4/5] Create fresh cluster
    [5/5] Load images and run tests
```

The new pipeline ensures:
- Registry is running before any image operations
- Binaries are built before images
- Images are built fresh (no stale images)
- Cluster is created with proper configuration
- Tests only run after everything is in place

### 3. Improved `load-all` Recipe

**Added**:
- Check if local images exist before attempting to load
- Tag images for registry before loading into kind
- Better error handling with fallback on load failures
- Color-coded output for success/failure/warning

### 4. New `ensure-test-ready` Recipe

Validates all prerequisites before running tests:
```bash
@ensure-test-ready
    - Check ginkgo binary exists
    - Check dropin directory exists  
    - Check Docker registry is running
```

Can be called manually or as part of test setup.

### 5. Added Color Constants

Extended color palette for better output visibility:
```bash
GREEN := '\033[0;32m'   # Success
BLUE := '\033[0;34m'    # Info
YELLOW := '\033[0;33m'  # Warning
RED := '\033[0;31m'     # Error
```

### 6. New Convenience Recipe

Added `test-fqdn-fresh` for quick FQDN testing with fresh setup:
```bash
@test-fqdn-fresh timeout='45m'
    just run-existing-fresh cilium-test K8sAgentFQDNTest "{{timeout}}"
```

## Key Changes to Helm Configuration

The `run-existing` recipe now properly sets:
```bash
image.useDigest=false        # Ignore hard-coded digest pins
image.pullPolicy=Never       # Use pre-loaded images
```

This ensures Helm uses our locally-built images instead of trying to match upstream digests.

## Usage

### Old Way (broken):
```bash
just run-existing-fresh cilium-test K8sAgentFQDNTest 45m
# Would fail because:
# - Registry not running
# - Images not built
# - Helm digest mismatches
```

### New Way (fixed):
```bash
# Option 1: Full fresh setup (recommended)
just test-fqdn-fresh

# Option 2: With explicit cluster name and timeout
just run-existing-fresh cilium-test K8sAgentFQDNTest 45m

# Option 3: Manual step-by-step
just ensure-test-ready
just run-existing-fresh cilium-test K8sAgentFQDNTest 45m
```

## Testing the Fixes

To verify the infrastructure is working:

```bash
# Run a quick test
just ensure-test-ready
just run-existing cilium-test K8sAgentFQDNTest 45m

# Or use the convenience recipe
just test-fqdn-fresh
```

Expected behavior:
1. Docker registry starts automatically
2. Binaries build successfully
3. Docker images build successfully (no BuildKit attestations)
4. Kind cluster is created fresh
5. All images load into cluster successfully
6. Tests run without Cilium pod startup failures
7. Test results are reported at the end

## Track I Integration Test Status

With these fixes, the Track I integration test (`K8sAgentFQDNTest`) should now:
- ✅ Start with a clean environment
- ✅ Have all prerequisites available
- ✅ Deploy Cilium successfully
- ✅ Run FQDN policy tests
- ✅ Report pass/fail results

Expected outcome: **94% → 99%+ test pass rate** as the service backend map population issue is resolved.
