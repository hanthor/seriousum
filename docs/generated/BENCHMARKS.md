# Benchmark Comparison: Seriousum vs Cilium

_Last updated: 2026-05-11 21:12 UTC · commit `ddaa658`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## Methodology

### Binary comparison
- Seriousum: `target/release/seriousum-daemon`
- Cilium: `/usr/bin/cilium-agent` extracted from `quay.io/cilium/cilium-ci:latest`

### Micro-benchmarks
- Framework: **criterion**
- Seriousum benches executed directly from `target/release/deps/* --bench`
- Results parsed from `target/criterion/**/new/estimates.json`
- Scope: Seriousum internal hot paths used for regression tracking

### System-level comparison
- Helm+kind system benchmarks are wired in the repo, but this host could not complete kind cluster boot due local kubelet/cgroup limitations.
- The publishable comparison below therefore includes the directly measured binary comparison plus reproducible Seriousum micro-benchmarks.

## Published Results

### Direct Seriousum vs Cilium comparison

| Metric | Seriousum | Cilium | Delta vs Cilium |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126612 KB | -97.8% |

### Seriousum micro-benchmarks

| Benchmark | Median |
|---|---:|
| Load balancer round-robin (8 backends) | 4.10 ns |
| Load balancer consistent hash (8 backends) | 7.05 ns |
| Policy evaluation (1 policy) | 5.62 µs |
| Policy evaluation (100 policies) | 11.57 µs |
| Selector match (hit) | 35.54 ns |
| IPAM allocate + release ×1000 | 3.19 ms |

### Pending CI-published system metrics

The following comparison slots are intentionally reserved and should be filled by CI or a host with working kind support:

| Metric | Status |
|---|---|
| Startup time | Pending kind-capable runner |
| Idle memory (RSS / pod) | Pending kind-capable runner |
| Idle CPU | Pending kind-capable runner |

## Reproduce locally

```bash
# Build the benchmark binaries
cargo build --profile bench --benches

# Run the three Criterion suites directly
find target/release/deps -maxdepth 1 -type f -name 'load_balancer-*' ! -name '*.d' | head -1 | xargs -r -I{} {} --bench
find target/release/deps -maxdepth 1 -type f -name 'policy_eval-*' ! -name '*.d' | head -1 | xargs -r -I{} {} --bench
find target/release/deps -maxdepth 1 -type f -name 'ipam-*' ! -name '*.d' | head -1 | xargs -r -I{} {} --bench

# Inspect parsed results
cat docs/generated/benchmark-results.json
```

## Notes

- This publication is intentionally conservative: it only includes numbers successfully measured on this host.
- The repo still contains automation for future Helm+kind system benchmarks.
- Future expansions can add direct upstream Go micro-benchmarks for policy and allocator internals.
