# Benchmark Comparison: Seriousum vs Cilium

_Last updated: 2026-05-12 05:47 UTC · commit `ecd9499`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## Comparison categories

- **Direct-ish**: same or very similar operation shape between Seriousum and upstream Cilium.
- **Approximate**: useful directional comparison, but underlying implementation or data model differs.
- **Pending**: reserved for system-level kind/Helm comparison on capable runners.

## Direct-ish comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126612 KB | -97.8% |
| Selector match hit | **36.87 ns** | 4.14 ns | 8.91x |
| Selector match miss | **12.77 ns** | 4.11 ns | 3.11x |
| Policy resolve no-match | **25.07 µs** | 1.32 ms | 0.02x |
| IP allocator hot path | **139.66 ns** | 381.80 ns | 0.37x |
| ServiceName construction | **21.07 ns** | 33.44 ns | 0.63x |
| L3n4Addr IPv4 string+protocol | **91.10 ns** | 60.00 ns | 1.52x |
| FQDN lookup | **46.34 ns** | 3.29 µs | 0.01x |
| FQDN update | **184.01 ns** | 2.23 ms | 0.00x |

## Approximate comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Consistent-hash table build | **125.19 µs** | 3.16 ms | 0.04x |
| ServiceName string/key | **35.03 ns** | 33.34 ns | 1.05x |
| Load balancer upsert 1 | **1.58 µs** | 6.12 µs | 0.26x |
| Load balancer upsert 100 | **29.21 µs** | 297.22 µs | 0.10x |
| FQDN selector string | **64.63 ns** | 82.24 ns | 0.79x |
| FQDN JSON marshal 100 | **2.90 µs** | 136.36 µs | 0.02x |
| FQDN JSON unmarshal 100 | **14.63 µs** | 392.91 µs | 0.04x |
| FQDN JSON marshal 1000 | **31.27 µs** | 1.50 ms | 0.02x |
| FQDN JSON unmarshal 1000 | **156.00 µs** | 4.33 ms | 0.04x |

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
- Seriousum FQDN JSON ops: 'fqdn_json_marshal_100' + 'fqdn_json_unmarshal_100' + 'fqdn_json_marshal_1000' + 'fqdn_json_unmarshal_1000'
- Cilium FQDN JSON ops: 'BenchmarkMarshalJSON100' + 'BenchmarkUnmarshalJSON100' + 'BenchmarkMarshalJSON1000' + 'BenchmarkUnmarshalJSON1000'
- Seriousum LB batch ops: 'lb_upsert_service_1' + 'lb_upsert_service_100'
- Cilium LB batch ops: 'Benchmark_UpsertServiceAndFrontends_1' + 'Benchmark_UpsertServiceAndFrontends_100'

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
| Load balancer round-robin (8 backends) | 4.10 ns |
| Load balancer consistent hash select (8 backends) | 7.02 ns |
| Policy evaluation (1 policy) | 5.59 µs |
| Policy evaluation (100 policies) | 11.54 µs |
| Policy evaluation no match 1000 | 25.07 µs |
| Selector match (hit) | 36.87 ns |
| Selector match (miss) | 12.77 ns |
| IPAM allocate warm pool | 139.66 ns |
| IPAM allocate + release ×1000 | 3.17 ms |
| Maglev table build (1000 backends) | 125.19 µs |
| ServiceName construction | 21.07 ns |
| ServiceName display | 35.03 ns |
| L3n4Addr IPv4 display | 91.10 ns |
| Load balancer upsert 1 | 1.58 µs |
| Load balancer upsert 100 | 29.21 µs |
| Load balancer update backends 100 | 13.09 µs |
| FQDN lookup | 46.34 ns |
| FQDN update | 184.01 ns |
| FQDN selector string | 64.63 ns |
| FQDN JSON marshal 100 | 2.90 µs |
| FQDN JSON unmarshal 100 | 14.63 µs |
| FQDN JSON marshal 1000 | 31.27 µs |
| FQDN JSON unmarshal 1000 | 156.00 µs |

## Upstream Cilium Go micro-benchmarks

| Benchmark | Result |
|---|---:|
| Selector match valid 1000 | 4.14 ns |
| Selector match invalid 1000 | 4.11 ns |
| Policy resolve no matching rules | 1.32 ms |
| Hash allocator alloc any | 381.80 ns |
| Maglev lookup table build 16381 | 3.16 ms |
| ServiceName construction | 33.44 ns |
| ServiceName key | 33.34 ns |
| L3n4Addr IPv4 string+protocol | 60.00 ns |
| Load balancer upsert service+frontends 1 | 6.12 µs |
| Load balancer upsert service+frontends 100 | 297.22 µs |
| FQDN get IPs | 3.29 µs |
| FQDN update IPs | 2.23 ms |
| FQDN selector string | 82.24 ns |
| FQDN JSON marshal 100 | 136.36 µs |
| FQDN JSON unmarshal 100 | 392.91 µs |
| FQDN JSON marshal 1000 | 1.50 ms |
| FQDN JSON unmarshal 1000 | 4.33 ms |

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
