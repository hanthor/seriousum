# Parity Proof (Published)

This document publishes the current parity proof status using direct upstream-vs-seriousum evidence from the same Cilium ginkgo harness.

## Verdict

**Current verdict: NOT YET PROVEN**

Seriousum does not currently meet full behavioral parity with upstream Cilium for the tested focus group.

## Evidence set

Focus group tested:

- `K8sAgentChaosTest`

Execution harness:

- Same upstream Cilium `test.test` binary and flags
- Upstream run via `just run-upstream ...`
- Seriousum run via `just run-existing ...`

## Result summary

| Target | Specs passed | Specs failed | Duration | Verdict |
|---|---:|---:|---:|---|
| Upstream Cilium | 4 | 0 | 415.813s | PASS |
| Seriousum | 0 | 4 | 411.204s | FAIL |

## Blocking parity gap

Seriousum fails in `BeforeAll`/`BeforeEach` because required local agent API calls fail over the expected Unix socket path:

- `dial unix /var/run/cilium/cilium.sock: connect: no such file or directory`
- failing calls include:
  - `GET /v1/service`
  - `GET /v1/endpoint`

## Reproduction

```bash
# Upstream baseline
just ginkgo-cluster cilium-upstream 236 237
just load-upstream cilium-upstream
just run-upstream cilium-upstream K8sAgentChaosTest 30m

# Seriousum comparison run
just ginkgo-cluster cilium-ginkgo 234 235
just load-all cilium-ginkgo
just run-existing cilium-ginkgo K8sAgentChaosTest 30m
```

## Log evidence

- Upstream PASS log: `target/bench-upstream-K8sAgentChaosTest.log`
- Seriousum FAIL log: `target/bg-seriousum-K8sAgentChaosTest-20260513-200817.log`

## Baseline artifacts

- `docs/archive/parity-baseline-chaos-upstream.json`
- Captured from `just run-upstream-fresh cilium-upstream K8sAgentChaosTest 30m`
- Includes upstream `cilium-dbg status --all-controllers -o json`, cilium probe settings, node taints, CoreDNS readiness/events, and `/healthz` vs `/v1/healthz` shapes
