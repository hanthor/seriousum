# Benchmark Comparison: Seriousum vs Cilium

_Last updated: 2026-05-16 07:48 UTC · commit `aaf891e`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## Comparison categories

- **Direct-ish**: same or very similar operation shape between Seriousum and upstream Cilium.
- **Approximate**: useful directional comparison, but underlying implementation or data model differs.
- **Pending**: reserved for system-level kind/Helm comparison on capable runners.

## Direct-ish comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126686 KB | -97.8% |
| Selector match hit | **35.82 ns** | 4.13 ns | 8.67x |
| Selector match miss | **11.48 ns** | 4.12 ns | 2.79x |
| Policy resolve no-match | **14.50 µs** | 1.36 ms | 0.01x |
| IP allocator hot path | **140.96 ns** | 345.60 ns | 0.41x |
| ServiceName construction | **24.01 ns** | 32.91 ns | 0.73x |
| L3n4Addr IPv4 string+protocol | **104.76 ns** | 62.08 ns | 1.69x |
| FQDN lookup | **51.91 ns** | 3.23 µs | 0.02x |
| FQDN update | **137.46 ns** | 2.13 ms | 0.00x |

## Approximate comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Consistent-hash table build | **N/A** | 3.19 ms | N/A |
| ServiceName string/key | **34.52 ns** | 33.84 ns | 1.02x |
| Load balancer upsert 1 | **N/A** | 5.44 µs | N/A |
| Load balancer upsert 100 | **N/A** | 353.90 µs | N/A |
| FQDN selector string | **64.25 ns** | 80.98 ns | 0.79x |
| FQDN JSON marshal 100 | **3.23 µs** | 150.20 µs | 0.02x |
| FQDN JSON unmarshal 100 | **15.43 µs** | 384.96 µs | 0.04x |
| FQDN JSON marshal 1000 | **35.04 µs** | 1.60 ms | 0.02x |
| FQDN JSON unmarshal 1000 | **165.50 µs** | 4.09 ms | 0.04x |

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
| Load balancer round-robin (8 backends) | N/A |
| Load balancer consistent hash select (8 backends) | N/A |
| Policy evaluation (1 policy) | 286.86 ns |
| Policy evaluation (100 policies) | 31.70 µs |
| Policy evaluation no match 1000 | 14.50 µs |
| Selector match (hit) | 35.82 ns |
| Selector match (miss) | 11.48 ns |
| IPAM allocate warm pool | 140.96 ns |
| IPAM allocate + release ×1000 | 3.11 ms |
| Maglev table build (1000 backends) | N/A |
| ServiceName construction | 24.01 ns |
| ServiceName display | 34.52 ns |
| L3n4Addr IPv4 display | 104.76 ns |
| Load balancer upsert 1 | N/A |
| Load balancer upsert 100 | N/A |
| Load balancer update backends 100 | N/A |
| FQDN lookup | 51.91 ns |
| FQDN update | 137.46 ns |
| FQDN selector string | 64.25 ns |
| FQDN JSON marshal 100 | 3.23 µs |
| FQDN JSON unmarshal 100 | 15.43 µs |
| FQDN JSON marshal 1000 | 35.04 µs |
| FQDN JSON unmarshal 1000 | 165.50 µs |

## Upstream Cilium Go micro-benchmarks

| Benchmark | Result |
|---|---:|
| Selector match valid 1000 | 4.13 ns |
| Selector match invalid 1000 | 4.12 ns |
| Policy resolve no matching rules | 1.36 ms |
| Hash allocator alloc any | 345.60 ns |
| Maglev lookup table build 16381 | 3.19 ms |
| ServiceName construction | 32.91 ns |
| ServiceName key | 33.84 ns |
| L3n4Addr IPv4 string+protocol | 62.08 ns |
| Load balancer upsert service+frontends 1 | 5.44 µs |
| Load balancer upsert service+frontends 100 | 353.90 µs |
| FQDN get IPs | 3.23 µs |
| FQDN update IPs | 2.13 ms |
| FQDN selector string | 80.98 ns |
| FQDN JSON marshal 100 | 150.20 µs |
| FQDN JSON unmarshal 100 | 384.96 µs |
| FQDN JSON marshal 1000 | 1.60 ms |
| FQDN JSON unmarshal 1000 | 4.09 ms |

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
