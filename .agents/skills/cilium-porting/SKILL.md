---
name: cilium-porting
description: Port a Cilium Go package to Rust. Use when assigned a Track issue (A–X) from the seriousum porting roadmap (GitHub issue #46). Provides step-by-step porting workflow, Go→Rust translation patterns, Cilium-specific idioms, and test validation instructions. Triggers on any task mentioning "port", "Track [A-X]", a pkg/* Go path, or a crates/* target.
compatibility: Requires /var/home/james/dev/cilium (Go source) and /var/home/james/dev/seriousum (Rust workspace). rustc 1.95+, cargo, kind, kubectl, helm.
---

# Cilium Porting Skill

## Paths

- Go source root: `/var/home/james/dev/cilium`
- Rust workspace:  `/var/home/james/dev/seriousum`
- Porting guide:   `/var/home/james/dev/seriousum/PORTING.md`
- Track issues:    https://github.com/hanthor/seriousum/issues (see #46 for map)

---

## Step 1 — Read the Go source thoroughly

```bash
# Read every non-test .go file in the target package
find /var/home/james/dev/cilium/<pkg> -name "*.go" ! -name "*_test.go" | sort
```

For each file:
1. Understand the **public interface** (exported types + functions)
2. Note **dependencies** on other cilium packages
3. Identify **syscalls / kernel interfaces** (netlink, eBPF, sockets)
4. Find **concurrency primitives** (mutex, channel, goroutine, sync.Map)
5. Note **error types** returned

---

## Step 2 — Map Go idioms to Rust

| Go | Rust |
|----|------|
| `struct` with methods | `struct` + `impl` |
| `interface` | `trait` |
| `goroutine` + `channel` | `tokio::spawn` + `mpsc::channel` |
| `sync.Mutex` | `tokio::sync::Mutex` or `std::sync::Mutex` |
| `sync.RWMutex` | `tokio::sync::RwLock` |
| `sync.Map` | `DashMap` or `Arc<RwLock<HashMap>>` |
| `context.Context` | Pass `CancellationToken` or use `select!` |
| `error` interface | `thiserror` enum |
| `fmt.Errorf("…: %w", err)` | `anyhow::Context::context()` |
| `log.WithField` | `tracing::info!(field = val, "msg")` |
| `time.Sleep` | `tokio::time::sleep` |
| `select {}` (multi-channel) | `tokio::select!` |
| `defer f()` | `scopeguard::defer!` or `Drop` impl |
| `init()` | `once_cell::sync::Lazy` or `OnceCell` |
| `go func() { for range ch }` | `tokio::spawn(async { while let Some(x) = rx.recv().await })` |
| `[]byte` | `Vec<u8>` or `Bytes` |
| `map[K]V` | `HashMap<K, V>` |
| `unsafe.Pointer` | `*mut T` in `unsafe` block (document why) |

### Cilium-specific patterns

| Cilium Go | Rust equivalent |
|-----------|-----------------|
| `hive.Cell` / `cell.Provide` | Struct with `new()` taking dependencies |
| `statedb.Table[T]` | `Arc<RwLock<HashMap<K, T>>>` (or `dashmap`) |
| `job.Group` background work | `tokio::task::JoinSet` |
| `stream.Observable` | `tokio::sync::broadcast` or `futures::Stream` |
| `option.DaemonConfig` | Flat `Config` struct with builder |
| `k8s.Client` | `kube::Client` |
| `labels.Labels` | `HashMap<String, String>` (keep `source:key=value` format) |
| `types.IPv4` / `types.IPv6` | `std::net::Ipv4Addr` / `Ipv6Addr` |
| `cidr.CIDR` | `ipnet::IpNet` |
| `NumericIdentity` | `u32` newtype |
| `ebpf.Map` | `aya::maps::HashMap` (or appropriate map type) |

---

## Step 3 — Set up the target crate

```bash
cd /var/home/james/dev/seriousum

# Check what's already scaffolded
cat crates/<crate>/src/lib.rs

# Check Cargo.toml for existing dependencies
cat crates/<crate>/Cargo.toml
```

Add dependencies as needed (use workspace versions where available):
```toml
# In crates/<crate>/Cargo.toml
[dependencies]
tokio        = { workspace = true }
serde        = { workspace = true }
thiserror    = "2"
anyhow       = { workspace = true }
tracing      = { workspace = true }
dashmap      = "6"
ipnet        = { workspace = true }
aya          = { workspace = true }          # for eBPF crates
kube         = { workspace = true }          # for K8s crates
k8s-openapi  = { workspace = true }          # for K8s crates
etcd-client  = "0.14"                        # for kvstore
```

---

## Step 4 — Port each type

Order of operations:
1. **Value types** (no IO, no async) — port + unit-test first
2. **Repository types** (hold state, no external IO) — port + unit-test
3. **IO types** (make syscalls, talk to K8s, eBPF) — port + integration-test

Template for a typical ported type:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Ported from cilium/pkg/<foo>/bar.go
pub struct Bar {
    // ...
}

impl Bar {
    pub fn new(/* deps */) -> Self { ... }
}
```

---

## Step 5 — Write tests

Every ported function needs at minimum:
- A success-path unit test
- An error/edge-case unit test

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_<name>_success() {
        // arrange
        // act
        // assert
    }

    #[test]
    fn test_<name>_error_case() { ... }

    // Async tests
    #[tokio::test]
    async fn test_<name>_async() { ... }
}
```

For IO types that touch the kernel (eBPF, netlink), add an `#[ignore]` integration test:
```rust
#[test]
#[ignore = "requires root + kernel eBPF support"]
fn test_bpf_map_real() { ... }
```

---

## Step 6 — Verify compilation and tests

```bash
cd /var/home/james/dev/seriousum

# Check only the target crate (fast)
cargo check -p <crate-name>

# Build
cargo build -p <crate-name>

# All tests in the crate
cargo test -p <crate-name> -- --nocapture

# Full workspace (must stay green)
cargo test --workspace

# Clippy (0 warnings policy)
cargo clippy -p <crate-name> -- -D warnings
```

---

## Step 7 — Run Cilium integration tests

Use the `cilium-test` skill to validate against the real Cilium test harness:

```
/skill:cilium-test
```

Or manually:
```bash
cd /var/home/james/dev/seriousum
./scripts/run-cilium-kind-test.sh --focus "<FocusForThisTrack>" --timeout 45m
```

Focus groups per track:
| Track | ginkgo focus |
|-------|-------------|
| D (K8s watchers) | `K8sAgentFQDNTest\|K8sAgentPerNodeConfigTest` |
| F (Policy) | `K8sAgentPolicyTest` |
| G (Endpoint) | `K8sAgentChaosTest` |
| I (LB maps) | `K8sDatapathServicesTest` |
| K (FQDN) | `K8sAgentFQDNTest` |
| L (Hubble) | `K8sAgentHubbleTest` |
| R (Operator) | All suites (operator underpins everything) |

---

## Step 8 — Open a PR

```bash
cd /var/home/james/dev/seriousum

# Create branch named after the track
git checkout -b port/track-<letter>-<name>

git add crates/<crate>/
git commit -m "port: Track <X> — <description>

Ports cilium/pkg/<foo> → crates/<crate>.

Implements:
- <TypeA>: <what it does>
- <TypeB>: <what it does>

Tests: N unit, M integration
Refs: hanthor/seriousum#<issue-number>"

git push origin port/track-<letter>-<name>
gh pr create --repo hanthor/seriousum \
  --title "port: Track <X> — <short description>" \
  --body "Closes #<issue>" \
  --label "porting"
```

---

## Quick Reference — Common Cilium Types

```rust
// Labels (cilium source:key=value format)
pub type Labels = HashMap<String, String>;
fn label_key(source: &str, key: &str) -> String {
    format!("{source}:{key}")
}

// NumericIdentity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NumericIdentity(pub u32);

impl NumericIdentity {
    pub const WORLD: Self = Self(1);
    pub const HOST:  Self = Self(2);
    pub const LOCAL_NODE: Self = Self(6);
}

// IPv4/IPv6 address pair (Cilium endpoint addressing)
pub struct AddressPair {
    pub ipv4: Option<std::net::Ipv4Addr>,
    pub ipv6: Option<std::net::Ipv6Addr>,
}

// Endpoint ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EndpointID(pub u16);
```
