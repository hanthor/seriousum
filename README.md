# Seriousum

[![Release](https://img.shields.io/badge/release-v0.1.0--alpha-blue)](https://github.com/hanthor/seriousum/releases/tag/v0.1.0-alpha)
![License](https://img.shields.io/badge/license-Apache%202.0-green)
[![Tests](https://img.shields.io/badge/tests-430%2F430-brightgreen)](docs/FULL_TEST_SUITE_CATALOG.md)
[![Parity Proof](https://img.shields.io/badge/parity%20proof-not%20yet%20proven-yellow)](docs/PARITY_PROOF_DASHBOARD.md)

**Published evidence (latest):**
- **Parity proof**: [docs/PARITY_PROOF.md](docs/PARITY_PROOF.md)
- **Speed comparison (same ginkgo harness)**: [docs/SPEED_COMPARISON.md](docs/SPEED_COMPARISON.md)

A Rust-based reimplementation effort for major Cilium userspace and control-plane components.

Seriousum currently delivers a substantial Rust port of core Cilium subsystems, compatibility-oriented binaries, benchmark comparisons against upstream Cilium, and a formal **parity proof dashboard** that tracks what is and is not yet proven.

- **Repository**: https://github.com/hanthor/seriousum
- **Release**: `v0.1.0-alpha`
- **Current parity verdict**: **not yet proven**
- **Parity proof dashboard**: [docs/PARITY_PROOF_DASHBOARD.md](docs/PARITY_PROOF_DASHBOARD.md)

---

## Current project state

### What exists today
- 24 core implementation tracks in Rust
- 35 crates
- 32,658 lines of production Rust
- 430 unit tests passing
- benchmark comparisons against upstream Cilium Go hot paths
- installation paths via Helm, containers, binaries, and source
- compatibility-oriented CLI and runtime wrappers

### What is true today
Seriousum has **strong evidence for partial parity**.

It does **not** yet have proof for a full claim like:

> “Seriousum fully reimplements Cilium in Rust.”

That stronger claim requires more than code volume and unit tests. It requires full scope accounting, full upstream behavioral validation, operational proof, and soak/recovery evidence. The repo now tracks that explicitly in the parity proof dashboard.

See: [docs/PARITY_PROOF_DASHBOARD.md](docs/PARITY_PROOF_DASHBOARD.md)

---

## Parity proof summary

Current dashboard result:

| Pillar | Status |
|---|---|
| Scope inventory | 🟡 Partial |
| Implementation coverage | 🟡 Partial |
| Behavioral test parity | 🟡 Partial |
| Operational parity | 🟡 Partial |
| Performance parity | 🟡 Partial |
| Production / soak proof | 🔴 Missing |

**Overall:** ⚠️ **NOT YET PROVEN**

Why this matters: the project now distinguishes between:
- implemented code
- compatibility evidence
- benchmark evidence
- full parity proof

That keeps claims honest and measurable.

---

## Implemented track areas

### Infrastructure (A-D)
- eBPF map infrastructure
- eBPF datapath loader
- CNI plugin
- Kubernetes watchers

### Control plane (E-J)
- Identity + IPCache
- Policy engine
- Endpoint management
- IPAM
- Load balancing
- kvstore backend

### Networking (K-P)
- FQDN DNS proxy
- Hubble observability
- Envoy / L7 policy scaffolding
- WireGuard + IPsec scaffolding
- ClusterMesh scaffolding
- BGP scaffolding

### Operations (Q-X)
- Egress gateway scaffolding
- Kubernetes operator
- Daemon orchestration
- `cilium-dbg`-compatible CLI surface
- `cilium`-compatible CLI surface
- Metrics + monitor
- Hubble relay
- REST API server

For component-by-component status, see:
- [docs/component-porting-compliance.md](docs/component-porting-compliance.md)
- [docs/parity-matrix.md](docs/parity-matrix.md)

---

## Quick start

### Prerequisites
```bash
rustc --version   # 1.95.0+
docker --version
kubectl version
helm version
```

### Build from source
```bash
git clone https://github.com/hanthor/seriousum.git
cd seriousum
cargo build --release --bins
cargo test --workspace --lib
```

### Install with Helm
```bash
helm install cilium ./install/kubernetes/seriousum \
  --namespace kube-system \
  --create-namespace
```

### Use container images
```bash
docker pull ghcr.io/hanthor/seriousum/agent:v0.1.0-alpha
docker pull ghcr.io/hanthor/seriousum/operator:v0.1.0-alpha
docker pull ghcr.io/hanthor/seriousum/tools:v0.1.0-alpha
```

### Full installation guide
See [docs/INSTALLATION.md](docs/INSTALLATION.md).

---

## Benchmark comparisons vs upstream Cilium

Seriousum now publishes direct-ish and approximate benchmark comparisons against upstream Cilium Go benchmarks.

Primary published comparisons include:
- binary size
- selector match hit/miss
- policy resolve no-match
- allocator hot path
- ServiceName formatting/construction
- L3n4Addr string formatting
- load balancer upsert paths
- FQDN lookup/update
- FQDN JSON marshal/unmarshal
- Maglev table build

Full report:
- [docs/generated/BENCHMARKS.md](docs/generated/BENCHMARKS.md)
- [docs/generated/benchmark-results.json](docs/generated/benchmark-results.json)

<!-- BENCHMARK_START -->
## 📊 Benchmarks

> Last run: **2026-05-12 05:47 UTC** · commit `ecd9499`
> Published comparison report: [docs/generated/BENCHMARKS.md](docs/generated/BENCHMARKS.md)

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **2725 KB** | 126612 KB | -97.8% |
| Selector match hit | **36.87 ns** | 4.14 ns | 8.91x |
| Selector match miss | **12.77 ns** | 4.11 ns | 3.11x |
| Policy resolve no-match | **25.07 µs** | 1.32 ms | 0.02x |
| IP allocator hot path | **139.66 ns** | 381.80 ns | 0.37x |
| ServiceName construction | **21.07 ns** | 33.44 ns | 0.63x |
| FQDN lookup | **46.34 ns** | 3.29 µs | 0.01x |
| FQDN JSON marshal 100 | **2.90 µs** | 136.36 µs | 0.02x |

### Seriousum micro-benchmarks

| Benchmark | Median |
|---|---:|
| LB round-robin (8 backends) | 4.10 ns |
| LB consistent hash (8 backends) | 7.02 ns |
| Policy eval (1 policy) | 5.59 µs |
| Policy eval (100 policies) | 11.54 µs |
| Selector match (hit) | 36.87 ns |
| Selector match (miss) | 12.77 ns |
| IPAM alloc warm pool | 139.66 ns |
| IPAM alloc + release ×1000 | 3.17 ms |
| ServiceName display | 35.03 ns |
| Load balancer upsert 1 | 1.58 µs |
| Load balancer upsert 100 | 29.21 µs |
| FQDN update | 184.01 ns |
| FQDN selector string | 64.63 ns |

> System startup / memory / CPU status: **pending-kind-capable-runner**

<details>
<summary>Reproduce locally</summary>

~~~bash
./scripts/benchmark.sh --skip-kind --cilium-source /path/to/cilium
~~~

</details>
<!-- BENCHMARK_END -->

---

## Documentation map

### Most important docs
- [docs/PARITY_PROOF_DASHBOARD.md](docs/PARITY_PROOF_DASHBOARD.md) — what full parity proof requires
- [docs/INSTALLATION.md](docs/INSTALLATION.md) — installation and deployment
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) — debugging and common issues
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — system overview
- [docs/component-porting-compliance.md](docs/component-porting-compliance.md) — crate-level coverage
- [docs/parity-matrix.md](docs/parity-matrix.md) — Rust ↔ Cilium mapping
- [docs/generated/BENCHMARKS.md](docs/generated/BENCHMARKS.md) — benchmark evidence
- [docs/INDEX.md](docs/INDEX.md) — documentation index

### Release / project status
- [RELEASE_v0.1.0-alpha.md](RELEASE_v0.1.0-alpha.md)
- [COMPLIANCE_CERTIFICATION.md](COMPLIANCE_CERTIFICATION.md)

Note: some older docs use stronger language than the current parity proof model. The dashboard should be treated as the authoritative statement of proof status.

---

## Repository structure

```text
seriousum/
├── crates/                         # Rust implementation crates
├── benches/                        # Criterion benchmark suites
├── docs/                           # Main documentation
│   ├── generated/                  # Published generated evidence
│   └── archive/                    # Historical docs
├── install/kubernetes/seriousum/   # Helm chart
├── images/                         # Container build files
├── scripts/                        # Build, benchmark, validation automation
└── .github/workflows/              # CI workflows
```

---

## What “done” looks like

Seriousum should only claim full Rust reimplementation parity when all of the following are true:
- frozen upstream target release recorded
- full scope inventory completed
- remaining Go runtime exceptions resolved for the claimed scope
- unmodified upstream integration matrix passes
- install / upgrade / rollback parity verified
- performance budgets met
- soak / chaos / recovery evidence published

That checklist is tracked in:
- [docs/PARITY_PROOF_DASHBOARD.md](docs/PARITY_PROOF_DASHBOARD.md)
- [docs/generated/parity-proof.json](docs/generated/parity-proof.json)

---

## Development

### Build and test
```bash
cargo build --release --bins
cargo test --workspace --lib
cargo check --benches
```

### Run benchmark publishing locally
```bash
./scripts/benchmark.sh --skip-kind --cilium-source /path/to/cilium
```

### Validate parity proof artifacts
```bash
bash scripts/check-parity-proof.sh
```

### Other useful docs
- [docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md)
- [PORTING.md](PORTING.md)
- [AGENTS.md](AGENTS.md)

---

## License

Apache 2.0.

---

## Summary

Seriousum is best understood today as:
- a serious Rust port of major Cilium subsystems,
- with substantial implementation and benchmark evidence,
- but **without full parity proof yet**.

That is the current, accurate project status.
