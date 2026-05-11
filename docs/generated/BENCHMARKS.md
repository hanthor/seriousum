# Benchmark Comparison: Seriousum vs Cilium

_Last updated: 2026-05-11 23:45 UTC · commit `ac32013`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## What is directly compared

The most directly comparable measurements currently published are:
- **Agent binary size**
- **Selector matching hot path**
- **Allocator hot path**
- **Consistent-hash table build** (approximate, implementation details differ)

## Direct comparison

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126612 KB | -97.8% |
| Selector match hot path | **36.58 ns** | 4.27 ns | 8.57x |
| IP allocator hot path | **140.34 ns** | 405.40 ns | 0.35x |
| Consistent-hash table build | **117.45 µs** | 3.30 ms | 0.04x |

### Benchmark mapping
- Seriousum selector: 'selector_match/match_hit'
- Cilium selector: 'pkg/policy/types BenchmarkMatchesValid1000'
- Seriousum allocator: 'ipam_allocate_warm_pool'
- Cilium allocator: 'pkg/ipalloc BenchmarkHashAlloc_AllocAny'
- Seriousum hash-table build: 'lb_maglev_build_1000'
- Cilium hash-table build: 'pkg/maglev BenchmarkGetMaglevTable/16381'

## System metrics

| Metric | Seriousum | Cilium | Delta vs Cilium |
|---|---:|---:|---:|
| Startup time | **N/A s** | N/A s | N/A |
| Idle memory (RSS / pod) | **N/A MiB** | N/A MiB | N/A |
| Idle CPU | **N/A m** | N/A m | N/A |

System metric status: **pending-kind-capable-runner**

## Seriousum micro-benchmarks

| Benchmark | Median |
|---|---:|
| Load balancer round-robin (8 backends) | 4.11 ns |
| Load balancer consistent hash select (8 backends) | 7.13 ns |
| Policy evaluation (1 policy) | 5.63 µs |
| Policy evaluation (100 policies) | 11.69 µs |
| Selector match (hit) | 36.58 ns |
| IPAM allocate warm pool | 140.34 ns |
| IPAM allocate + release ×1000 | 3.16 ms |
| Maglev table build (1000 backends) | 117.45 µs |

## Upstream Cilium Go micro-benchmarks

| Benchmark | Result |
|---|---:|
| Selector match valid 1000 | 4.27 ns |
| Hash allocator alloc any | 405.40 ns |
| Maglev lookup table build 16381 | 3.30 ms |

## Reproduce locally

~~~bash
# Publish micro-benchmarks only
./scripts/benchmark.sh --skip-kind --cilium-source /path/to/cilium

# Publish full report if your host can run kind
./scripts/benchmark.sh --cilium-source /path/to/cilium

# Inspect machine-readable results
cat docs/generated/benchmark-results.json
~~~

## Notes

- System-level Helm+kind metrics remain optional because not every runner can boot kind successfully.
- The selector comparison is the closest direct hot-path comparison currently in the report.
- The allocator and Maglev rows are useful directional comparisons, but implementation details differ between projects.
