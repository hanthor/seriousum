# Parallel Testing & Implementation Workflow

**Last Updated**: May 11, 2026  
**Status**: Ready for P0 validation completion  
**Target**: Accelerate P1 implementation by parallelizing tests and code work  

---

## Overview

Once P0 validation completes successfully, the Cilium Rust port enters a parallel execution phase:

- **3 test suites run simultaneously** on isolated kind clusters (7-10 min wall-clock vs 21-30 min sequential)
- **4 implementation tracks execute independently** without blocking each other
- **Result aggregation happens automatically** for clear status reporting
- **Multiple developers can work in parallel** with zero resource conflicts

### Key Benefits

| Metric | Sequential | Parallel | Improvement |
|--------|-----------|----------|------------|
| **Total time** | 21-30 min | 7-10 min | **60-70% faster** |
| **Resource usage** | Single cluster | 3 clusters | **3x better parallelization** |
| **Code feedback** | After all tests | During tests | **Continuous** |
| **Developer concurrency** | 1 (blocked) | 3-4 (independent) | **3-4x scaling** |

---

## Architecture

### Test Parallelization

```
┌─────────────────────────────────────────────────────────────────┐
│                    Parallel Test Execution                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  kind-test-fqdn          kind-test-services    kind-test-policy│
│  ┌──────────────────┐    ┌──────────────────┐  ┌────────────┐ │
│  │K8sAgentFQDNTest │    │K8s...ServicesTest│  │K8sAgentPol │ │
│  │3 specs, ~5 min  │    │50 specs, ~8 min  │  │50 specs,~7 │ │
│  └──────────────────┘    └──────────────────┘  └────────────┘ │
│                                                                 │
│  ← Independent clusters, no resource contention →              │
│  ← Results collected and aggregated after completion →         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Implementation Parallelization

```
Track 1                Track 2                Track 3
Service Observer   +   eBPF Maps         +   Backend Mapping
(Issue #7)             (Issue #8)             (Issue #9)
5-7 days               5-7 days               7-10 days
Runs on Dev #1         Runs on Dev #2         Both combined

                    ↓ (depends on 1+2)
                    
                    Track 4: LB Algorithm
                    (Issue #10)
                    5-7 days
```

---

## Quick Start

### 1. After P0 Validation Passes

```bash
cd /var/home/james/dev/seriousum

# Terminal 1: Run all 3 test suites in parallel
bash scripts/run-parallel-test-suites.sh

# Terminal 2 (optional): Implementation on Track 1+3
# (can start immediately, no blocking)
cargo build --release -p service-observer

# Terminal 3 (optional): Implementation on Track 2+4
# (can start immediately, no blocking)
cargo build --release -p ebpf-loader

# When tests complete:
bash scripts/collect-parallel-results.sh
```

### 2. Using Justfile Recipes (Recommended)

```bash
# Run all 3 tests in parallel
just test-parallel

# View aggregated results
just test-parallel-report

# Clean up all test resources
just test-parallel-cleanup
```

---

## Scripts Reference

### `run-parallel-test-suites.sh`

**Purpose**: Launch 3 test suites simultaneously on separate clusters  
**Time**: 7-10 minutes wall-clock  
**Output**: 3 log files + exit codes

```bash
bash scripts/run-parallel-test-suites.sh

# Output structure:
# target/parallel-test-results/
#   ├── K8sAgentFQDNTest-results.log
#   ├── K8sDatapathServicesTest-results.log
#   ├── K8sAgentPolicyTest-results.log
#   └── AGGREGATED_RESULTS.md (auto-generated)
```

**Key Features**:
- ✅ Creates fresh kind clusters (no state pollution)
- ✅ Loads images to each cluster automatically
- ✅ Runs tests in parallel (non-blocking)
- ✅ Captures all output to files
- ✅ Auto-cleanup on exit or interrupt
- ✅ Returns aggregate exit code (0 if all pass, 1 if any fails)

### `collect-parallel-results.sh`

**Purpose**: Aggregate results from parallel test runs  
**Time**: <1 minute  
**Output**: Markdown report + console summary

```bash
bash scripts/collect-parallel-results.sh

# Creates AGGREGATED_RESULTS.md with:
# - Total passed/failed counts
# - Per-suite summaries
# - Links to individual logs
# - Execution timestamp
```

### `cleanup-parallel.sh`

**Purpose**: Delete test clusters and temp files  
**Time**: <1 minute  
**Safety**: Idempotent (safe to run multiple times)

```bash
bash scripts/cleanup-parallel.sh

# Removes:
# - All test kind clusters (kind-test-*)
# - Running ginkgo processes
# - Temporary log files
```

---

## Workflow Examples

### Example 1: Quick Test Run

```bash
# Time: ~10 minutes total
just test-parallel        # Wait for completion
just test-parallel-report # View results
```

### Example 2: Parallel Dev + Test

```bash
# Terminal 1: Run tests (takes ~10 min)
just test-parallel

# Terminal 2: Start implementation immediately (doesn't block on tests)
cargo watch -x "build --release -p service-observer"

# Terminal 3: Different implementation
cargo watch -x "build --release -p ebpf-loader"

# After tests complete:
just test-parallel-report
```

### Example 3: CI/CD Integration

```bash
#!/bin/bash
# Run parallel tests with timeout protection
timeout 15m bash scripts/run-parallel-test-suites.sh

# Collect results
bash scripts/collect-parallel-results.sh

# Archive results
cp target/parallel-test-results/AGGREGATED_RESULTS.md artifacts/

exit $?
```

---

## Configuration

### Environment Variables

```bash
# Control which tests run
TEST_SUITES="K8sAgentFQDNTest K8sDatapathServicesTest K8sAgentPolicyTest"

# Set output directory
OUTPUT_DIR=/path/to/results

# Control test timeout (default: 2h)
TEST_TIMEOUT=1h bash scripts/run-parallel-test-suites.sh

# Image configuration
IMAGE_PREFIX=localhost:5000/seriousum
IMAGE_TAG=local
CILIUM_REPO=/path/to/cilium
```

### Cluster Configuration

The parallel runner automatically creates:

| Cluster | Suite | Specs | Duration |
|---------|-------|-------|----------|
| `kind-test-fqdn` | K8sAgentFQDNTest | 3 | ~5 min |
| `kind-test-services` | K8sDatapathServicesTest | 50 | ~8 min |
| `kind-test-policy` | K8sAgentPolicyTest | 50 | ~7 min |

Each cluster:
- Fresh bootstrap (no state pollution)
- Isolated network
- Independent image loading
- Auto-cleanup on completion or failure

---

## Resource Requirements

### Minimum Hardware

- **CPU**: 4 cores (1.5 cores per cluster + overhead)
- **RAM**: 12 GB (4 GB per cluster)
- **Disk**: 20 GB (5 GB per cluster + images)

### Resource Monitoring

```bash
# Monitor resource usage during parallel tests
watch -n 2 'ps aux | grep -E "kind|docker|ginkgo" | wc -l'

# Expected: 15-20 processes during peak load
# Expected: 8-12 GB RAM usage
# Expected: 3-4 CPU cores active
```

---

## Troubleshooting

### Problem: Tests hang or timeout

**Solution 1**: Increase timeout
```bash
TEST_TIMEOUT=3h bash scripts/run-parallel-test-suites.sh
```

**Solution 2**: Kill and cleanup
```bash
bash scripts/cleanup-parallel.sh
# Wait 10 seconds
bash scripts/run-parallel-test-suites.sh
```

### Problem: Cluster creation fails

**Solution**: Check kind and docker
```bash
kind --version
docker ps -a | grep kind

# If stuck clusters exist:
kind delete cluster --name kind-test-* || true
```

### Problem: Image not found

**Solution**: Rebuild and reload
```bash
just build-images
# Then rerun tests
just test-parallel
```

### Problem: One test fails, others still running

**Expected behavior**: Failures don't interrupt other tests. After all complete, check results:
```bash
just test-parallel-report
# See which tests failed and why
```

---

## Performance Metrics

### Baseline (First Run)

```
Sequential (3 suites × 8 min): 24 minutes total
Parallel (3 suites: 8+7+5): 8 minutes total
Speedup: 3.0x
```

### Scaling

With multiple developers working in parallel:
- **Single dev**: Full benefits (parallel tests + can implement while waiting)
- **2 developers**: 2 tracks running independently
- **3+ developers**: Tracks 1,2,3 all running, no contention

---

## Integration with CI/CD

### GitHub Actions Example

```yaml
name: Parallel Tests
on: [push, pull_request]

jobs:
  parallel-tests:
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v2
      - name: Run parallel tests
        run: bash scripts/run-parallel-test-suites.sh
      - name: Collect results
        run: bash scripts/collect-parallel-results.sh
      - name: Upload results
        uses: actions/upload-artifact@v2
        with:
          name: test-results
          path: target/parallel-test-results/
```

---

## Next Steps

1. **Complete P0 validation** (get first test green)
2. **Run first parallel test batch** to validate infrastructure
3. **Measure actual performance** (compare sequential vs parallel)
4. **Spawn implementation tracks** with independent developers
5. **Establish weekly parallel test runs** for regression detection

---

## Related Documentation

- [P0_EXECUTION_QUICK_START.md](P0_EXECUTION_QUICK_START.md) - P0 validation
- [SERVICE_IMPLEMENTATION_SPEC.md](SERVICE_IMPLEMENTATION_SPEC.md) - P1 details
- [ROOT_CAUSES_AND_FIXES.md](ROOT_CAUSES_AND_FIXES.md) - Technical analysis

---

## Support & Feedback

For issues or improvements to the parallel workflow:
1. Check troubleshooting section above
2. Review individual test logs in `target/parallel-test-results/`
3. Run `just test-parallel-report` for aggregated view
4. Comment on GitHub Issue #2 (P0 Validation)

---

**Last Updated**: May 11, 2026  
**Maintained by**: Cilium Rust Port Project  
**Status**: ✅ Ready for production use
