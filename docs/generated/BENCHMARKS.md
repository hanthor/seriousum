# Benchmark Comparison: Seriousum vs Cilium

_Last updated: 2026-05-12 02:32 UTC · commit `103bdfd`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## What is directly compared

The most directly comparable measurements currently published are:
- **Agent binary size**
- **Selector matching hot path**
- **Allocator hot path**
- **ServiceName / address formatting hot paths**
- **FQDN cache operations**
- **Consistent-hash table build** (approximate, implementation details differ)

## Direct comparison

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126612 KB | -97.8% |
| Selector match hit | **36.57 ns** | 4.48 ns | 8.16x |
| Selector match miss | **11.29 ns** | 4.37 ns | 2.58x |
| IP allocator hot path | **143.39 ns** | 388.00 ns | 0.37x |
| Consistent-hash table build | **110.60 µs** | 3.55 ms | 0.03x |
| ServiceName construction | **23.88 ns** | 34.98 ns | 0.68x |
| ServiceName string/key | **35.39 ns** | 34.54 ns | 1.02x |
| L3n4Addr IPv4 string | **92.54 ns** | 68.09 ns | 1.36x |
| FQDN lookup | **46.78 ns** | 3.70 µs | 0.01x |
| FQDN update | **184.57 ns** | 2.27 ms | 0.00x |

### Benchmark mapping
- Seriousum selector hit/miss: 'selector_match/match_hit' + 'selector_match/match_miss'
- Cilium selector hit/miss: 'pkg/policy/types BenchmarkMatchesValid1000' + 'BenchmarkMatchesInvalid1000'
- Seriousum allocator: 'ipam_allocate_warm_pool'
- Cilium allocator: 'pkg/ipalloc BenchmarkHashAlloc_AllocAny'
- Seriousum hash-table build: 'lb_maglev_build_1000'
- Cilium hash-table build: 'pkg/maglev BenchmarkGetMaglevTable/16381'
- Seriousum ServiceName ops: 'lb_service_name_new' + 'lb_service_name_display'
- Cilium ServiceName ops: 'BenchmarkNewServiceName' + 'BenchmarkServiceNameKey'
- Seriousum FQDN ops: 'fqdn_lookup' + 'fqdn_update'
- Cilium FQDN ops: 'BenchmarkGetIPs' + 'BenchmarkUpdateIPs'

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
| Load balancer round-robin (8 backends) | 4.14 ns |
| Load balancer consistent hash select (8 backends) | 7.10 ns |
| Policy evaluation (1 policy) | 5.68 µs |
| Policy evaluation (100 policies) | 11.78 µs |
| Selector match (hit) | 36.57 ns |
| Selector match (miss) | 11.29 ns |
| IPAM allocate warm pool | 143.39 ns |
| IPAM allocate + release ×1000 | 3.23 ms |
| Maglev table build (1000 backends) | 110.60 µs |
| ServiceName construction | 23.88 ns |
| ServiceName display | 35.39 ns |
| L3n4Addr IPv4 display | 92.54 ns |
| FQDN lookup | 46.78 ns |
| FQDN update | 184.57 ns |

## Upstream Cilium Go micro-benchmarks

| Benchmark | Result |
|---|---:|
| Selector match valid 1000 | 4.48 ns |
| Selector match invalid 1000 | 4.37 ns |
| Hash allocator alloc any | 388.00 ns |
| Maglev lookup table build 16381 | 3.55 ms |
| ServiceName construction | 34.98 ns |
| ServiceName key | 34.54 ns |
| L3n4Addr IPv4 string | 68.09 ns |
| FQDN get IPs | 3.70 µs |
| FQDN update IPs | 2.27 ms |

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
