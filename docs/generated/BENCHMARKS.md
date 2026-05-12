# Benchmark Comparison: Seriousum vs Cilium

_Last updated: 2026-05-12 02:55 UTC · commit `0bd948b`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## Comparison categories

- **Direct-ish**: same or very similar operation shape between Seriousum and upstream Cilium.
- **Approximate**: useful directional comparison, but underlying implementation or data model differs.
- **Pending**: reserved for system-level kind/Helm comparison on capable runners.

## Direct-ish comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126612 KB | -97.8% |
| Selector match hit | **36.27 ns** | 4.50 ns | 8.06x |
| Selector match miss | **11.34 ns** | 4.33 ns | 2.62x |
| IP allocator hot path | **140.99 ns** | 403.10 ns | 0.35x |
| ServiceName construction | **21.16 ns** | 34.14 ns | 0.62x |
| L3n4Addr IPv4 string+protocol | **93.00 ns** | 63.73 ns | 1.46x |
| FQDN lookup | **46.66 ns** | 3.73 µs | 0.01x |
| FQDN update | **184.10 ns** | 2.22 ms | 0.00x |

## Approximate comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Consistent-hash table build | **117.16 µs** | 3.30 ms | 0.04x |
| ServiceName string/key | **35.63 ns** | 34.57 ns | 1.03x |
| Load balancer upsert 100 | **29.97 µs** | 317.43 µs | 0.09x |
| FQDN selector string | **66.19 ns** | 94.93 ns | 0.70x |
| FQDN JSON marshal 100 | **3.03 µs** | 138.63 µs | 0.02x |
| FQDN JSON unmarshal 100 | **14.79 µs** | 411.03 µs | 0.04x |

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
- Seriousum FQDN JSON ops: 'fqdn_json_marshal_100' + 'fqdn_json_unmarshal_100'
- Cilium FQDN JSON ops: 'BenchmarkMarshalJSON100' + 'BenchmarkUnmarshalJSON100'
- Seriousum LB batch op: 'lb_upsert_service_100'
- Cilium LB batch op: 'Benchmark_UpsertServiceAndFrontends_100'

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
| Load balancer consistent hash select (8 backends) | 7.18 ns |
| Policy evaluation (1 policy) | 5.65 µs |
| Policy evaluation (100 policies) | 11.62 µs |
| Selector match (hit) | 36.27 ns |
| Selector match (miss) | 11.34 ns |
| IPAM allocate warm pool | 140.99 ns |
| IPAM allocate + release ×1000 | 3.17 ms |
| Maglev table build (1000 backends) | 117.16 µs |
| ServiceName construction | 21.16 ns |
| ServiceName display | 35.63 ns |
| L3n4Addr IPv4 display | 93.00 ns |
| Load balancer upsert 100 | 29.97 µs |
| Load balancer update backends 100 | 13.31 µs |
| FQDN lookup | 46.66 ns |
| FQDN update | 184.10 ns |
| FQDN selector string | 66.19 ns |
| FQDN JSON marshal 100 | 3.03 µs |
| FQDN JSON unmarshal 100 | 14.79 µs |

## Upstream Cilium Go micro-benchmarks

| Benchmark | Result |
|---|---:|
| Selector match valid 1000 | 4.50 ns |
| Selector match invalid 1000 | 4.33 ns |
| Hash allocator alloc any | 403.10 ns |
| Maglev lookup table build 16381 | 3.30 ms |
| ServiceName construction | 34.14 ns |
| ServiceName key | 34.57 ns |
| L3n4Addr IPv4 string+protocol | 63.73 ns |
| Load balancer upsert service+frontends 100 | 317.43 µs |
| FQDN get IPs | 3.73 µs |
| FQDN update IPs | 2.22 ms |
| FQDN selector string | 94.93 ns |
| FQDN JSON marshal 100 | 138.63 µs |
| FQDN JSON unmarshal 100 | 411.03 µs |

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
