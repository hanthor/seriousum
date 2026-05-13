# Speed Comparison: Upstream Cilium vs Seriousum

This report publishes timing from the same integration harness run so the comparison is directly traceable.

## Test context

- Focus: `K8sAgentChaosTest`
- Harness: upstream Cilium ginkgo binary (`test.test`)
- Cluster mode: kind, k8s 1.33
- Command paths:
  - Upstream: `just run-upstream cilium-upstream K8sAgentChaosTest 30m`
  - Seriousum: `just run-existing cilium-ginkgo K8sAgentChaosTest 30m`

## Observed runtimes

| Target | Runtime | Outcome |
|---|---:|---|
| Upstream Cilium | 415.813s | PASS (4/4) |
| Seriousum | 411.204s | FAIL (0/4) |

## Interpretation

- Seriousum completed slightly faster by wall-clock time (~4.6s), but this is **not a positive speed win** because the run failed during setup/early behavioral checks.
- Therefore, this dataset supports only:
  - **timing transparency**
  - **parity gap identification**
- It does **not** support a claim of performance parity for this focus group.

## Logs

- Upstream: `target/bench-upstream-K8sAgentChaosTest.log`
- Seriousum: `target/bg-seriousum-K8sAgentChaosTest-20260513-200817.log`

