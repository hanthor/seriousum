# AGENTS.md — AI Agent Guide for seriousum

This file tells AI coding agents (pi, Claude, Cursor, etc.) everything they need to know to contribute to this project effectively.

---

## What this project is

**seriousum** is a full port of [Cilium](https://github.com/cilium/cilium) from Go to Rust.  
Cilium is a Kubernetes CNI plugin that provides eBPF-powered networking, security, and observability.

- **eBPF C programs** (`bpf/*.c`) stay in C — they are compiled by clang and loaded by the Rust agent.
- **All Go code** is being ported to Rust, one package at a time.
- The porting work is tracked via [GitHub issues #22–#46](https://github.com/hanthor/seriousum/issues).

## Repository layout

```
seriousum/
├── bpf/                    # eBPF C programs (keep as C, do not port)
├── crates/                 # Rust crates — one per Cilium subsystem
│   ├── api/                # REST API types
│   ├── bgp/                # BGP control plane
│   ├── cli/                # cilium-dbg + cilium-cli
│   ├── clustermesh/        # Multi-cluster
│   ├── cni/                # CNI plugin binary
│   ├── controller/         # Generic controller loop
│   ├── crypto/             # TLS/WireGuard key management
│   ├── daemon/             # Main agent orchestration
│   ├── datapath/           # eBPF program loader + tc/XDP hooks
│   ├── ebpf/               # eBPF map types (HashMap, LRU, Array…)
│   ├── endpoint/           # Endpoint lifecycle
│   ├── endpoints/          # Endpoint manager (P2 scaffold)
│   ├── envoy/              # Envoy xDS management server
│   ├── fqdn/               # DNS proxy + FQDN policy
│   ├── hubble/             # Hubble flow exporter + CLI
│   ├── identity/           # Security identity + IPCache
│   ├── ipam/               # IP address management
│   ├── k8s/                # Kubernetes client + watchers
│   ├── kvstore/            # etcd client
│   ├── loadbalancer/       # Service LB + eBPF map reconciler
│   ├── metrics/            # Prometheus metrics
│   ├── monitor/            # eBPF perf event consumer
│   ├── network/            # Netlink, routing, egress gateway
│   ├── node/               # Node identity + addressing
│   ├── operator/           # Kubernetes operator
│   ├── policy/             # Network policy engine
│   ├── proxy/              # L7 proxy integration
│   ├── service-observer/   # K8s service watcher (P1)
│   ├── wireguard/          # WireGuard + IPsec
│   └── backend-mapping/    # Backend selection engine
├── src/
│   ├── main.rs             # cilium-agent entry point
│   └── bin/                # cilium-dbg, hubble-relay, operator…
├── images/                 # Dockerfiles
├── scripts/                # Build + test scripts
├── justfile                # Task runner (run with `just`)
├── .agents/skills/         # AI agent skills (this file's siblings)
│   ├── cilium-porting/     # How to port a Go package to Rust
│   └── cilium-test/        # How to run Cilium ginkgo tests
├── AGENTS.md               # ← you are here
└── PORTING.md              # Detailed Go→Rust porting reference
```

---

## Skills

This repo ships two agent skills. Load them when relevant:

| Skill | When to use | Load with |
|-------|------------|-----------|
| `cilium-porting` | Implementing any porting track (issues #22–#45) | `/skill:cilium-porting` |
| `cilium-test` | Running Cilium integration tests to validate code | `/skill:cilium-test` |

---

## Track assignments (GitHub issues)

Each issue is a self-contained porting track. Pick one, load the `cilium-porting` skill, and implement.

| Issue | Track | Crate | Go source | Urgency |
|-------|-------|-------|-----------|---------|
| [#22](https://github.com/hanthor/seriousum/issues/22) | A | `crates/ebpf` | `pkg/bpf` + `pkg/maps` | 🔴 Critical |
| [#23](https://github.com/hanthor/seriousum/issues/23) | B | `crates/datapath` | `pkg/datapath/loader` | 🔴 Critical |
| [#24](https://github.com/hanthor/seriousum/issues/24) | C | `crates/cni` | `plugins/cilium-cni` | 🔴 Critical |
| [#25](https://github.com/hanthor/seriousum/issues/25) | D | `crates/k8s` | `pkg/k8s/watchers` | 🔴 Critical |
| [#26](https://github.com/hanthor/seriousum/issues/26) | E | `crates/identity` | `pkg/identity` + `pkg/ipcache` | 🔴 Critical |
| [#27](https://github.com/hanthor/seriousum/issues/27) | F | `crates/policy` | `pkg/policy` | 🔴 Critical |
| [#28](https://github.com/hanthor/seriousum/issues/28) | G | `crates/endpoint` | `pkg/endpoint` | 🔴 Critical |
| [#29](https://github.com/hanthor/seriousum/issues/29) | H | `crates/ipam` | `pkg/ipam` | 🔴 Critical |
| [#30](https://github.com/hanthor/seriousum/issues/30) | I | `crates/loadbalancer` | `pkg/loadbalancer` | 🔴 Critical |
| [#31](https://github.com/hanthor/seriousum/issues/31) | J | `crates/kvstore` | `pkg/kvstore` | 🟠 High |
| [#32](https://github.com/hanthor/seriousum/issues/32) | K | `crates/fqdn` | `pkg/fqdn` | 🟠 High |
| [#33](https://github.com/hanthor/seriousum/issues/33) | L | `crates/hubble` | `pkg/hubble` | 🟠 High |
| [#34](https://github.com/hanthor/seriousum/issues/34) | M | `crates/envoy` | `pkg/envoy` | 🟠 High |
| [#35](https://github.com/hanthor/seriousum/issues/35) | N | `crates/wireguard` | `pkg/wireguard` | 🟡 Medium |
| [#36](https://github.com/hanthor/seriousum/issues/36) | O | `crates/clustermesh` | `pkg/clustermesh` | 🟡 Medium |
| [#37](https://github.com/hanthor/seriousum/issues/37) | P | `crates/bgp` | `pkg/bgp` | 🟡 Medium |
| [#38](https://github.com/hanthor/seriousum/issues/38) | Q | `crates/network` | `pkg/egressgateway` | 🟡 Medium |
| [#39](https://github.com/hanthor/seriousum/issues/39) | R | `crates/operator` | `operator/pkg` | 🟠 High |
| [#40](https://github.com/hanthor/seriousum/issues/40) | S | `crates/daemon` | `daemon/` | 🔴 Critical |
| [#41](https://github.com/hanthor/seriousum/issues/41) | T | `crates/cli` | `cilium-dbg/cmd` | 🟡 Medium |
| [#42](https://github.com/hanthor/seriousum/issues/42) | U | `crates/cli` | `cilium-cli/` | 🟡 Medium |
| [#43](https://github.com/hanthor/seriousum/issues/43) | V | `crates/metrics` | `pkg/metrics` | 🟠 High |
| [#44](https://github.com/hanthor/seriousum/issues/44) | W | `src/bin/hubble-relay.rs` | `hubble-relay/` | 🟡 Medium |
| [#45](https://github.com/hanthor/seriousum/issues/45) | X | `crates/api` | `api/v1` | 🟠 High |

---

## Parallel track groups

These groups can be worked **simultaneously** with no merge conflicts:

```
Group 1 — Kernel / datapath (independent):
  #22 Track A  eBPF maps
  #23 Track B  Datapath loader
  #24 Track C  CNI plugin
  #35 Track N  WireGuard + IPsec

Group 2 — Control plane state (independent):
  #25 Track D  K8s watchers
  #26 Track E  Identity + IPCache
  #29 Track H  IPAM
  #31 Track J  kvstore

Group 3 — Policy + endpoints (depends on Groups 1+2):
  #27 Track F  Policy engine
  #28 Track G  Endpoint manager
  #30 Track I  Load balancer

Group 4 — Higher-level services (depends on Groups 1+2):
  #32 Track K  FQDN proxy
  #33 Track L  Hubble
  #34 Track M  Envoy xDS
  #37 Track P  BGP
  #38 Track Q  Egress gateway

Group 5 — Integration (depends on Groups 1–3):
  #39 Track R  Operator
  #40 Track S  Daemon (wires everything)
  #43 Track V  Metrics
  #44 Track W  Hubble relay

Group 6 — Tooling (depends on Group 5):
  #41 Track T  cilium-dbg CLI
  #42 Track U  cilium-cli
  #45 Track X  REST API server
```

---

## Code standards

### Must pass before merging
```bash
cargo test --workspace          # 0 failures
cargo clippy --all-targets -- -D warnings   # 0 warnings
cargo fmt -- --check            # 0 formatting issues
```

### Style rules
- All public items have doc comments (`/// …`)
- No `unwrap()` or `expect()` in production code paths — use `?` or explicit error handling
- No `println!` — use `tracing::info!` / `tracing::debug!`
- Async where Go used goroutines (`tokio::spawn`, `async fn`)
- `Arc<RwLock<T>>` for shared state, `mpsc::channel` for event streams
- Mirror the Go type names where it reduces confusion (e.g. `NumericIdentity`, `AddressPair`)

### Unsafe code
- Allowed only for eBPF map pointer casts (in `crates/ebpf`)
- Must be wrapped in a safe abstraction
- Must have a `// SAFETY:` comment explaining why it's sound

---

## Build commands

```bash
# Fast check (no code gen)
cargo check --workspace

# Build all crates
cargo build --workspace

# Run all unit tests
cargo test --workspace

# Build release binaries
cargo build --release --workspace

# Build container images
docker build -f images/cilium-agent.Dockerfile -t seriousum-agent:dev .
docker build -f images/operator.Dockerfile     -t seriousum-operator:dev .

# Justfile (see all recipes)
just --list

# Common recipes
just build          # cargo build --release
just test           # cargo test --workspace
just run "FocusName"  # Run one ginkgo focus group
just test-parallel  # Run 3 focus groups simultaneously
```

---

## Go source reference

The upstream Go source is at `~/dev/cilium`.  
Always read it before implementing a type — behaviour must match exactly.

```bash
# Find Go source for a concept
grep -r "type ServiceID" ~/dev/cilium/pkg --include="*.go" -l

# Read a package
ls ~/dev/cilium/pkg/loadbalancer/

# Count LOC in a package
find ~/dev/cilium/pkg/policy -name "*.go" ! -name "*_test.go" | xargs wc -l | tail -1
```

---

## Testing philosophy

1. **Unit tests** — every ported function, in `crates/<name>/src/lib.rs` `#[cfg(test)]` module
2. **Integration tests** — end-to-end with a real kind cluster, using upstream Cilium ginkgo suite
3. **Compatibility validation** — a track is done when the corresponding ginkgo focus group passes at ≥80%

See `/skill:cilium-test` for full test execution instructions.

---

## Key contacts

- Repo: https://github.com/hanthor/seriousum
- Track issues: https://github.com/hanthor/seriousum/issues?q=is%3Aopen+label%3Aporting
- Roadmap tracker: https://github.com/hanthor/seriousum/issues/46
