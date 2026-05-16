# PORTING.md — Go → Rust Porting Guide for Cilium

This guide covers everything you need to port a Cilium Go package to Rust for the seriousum project.

Read `AGENTS.md` first for project layout. Then come back here for the detailed translation reference.

---

## Table of Contents

1. [Workflow overview](#1-workflow-overview)
2. [Reading Go source](#2-reading-go-source)
3. [Type system translation](#3-type-system-translation)
4. [Concurrency patterns](#4-concurrency-patterns)
5. [Error handling](#5-error-handling)
6. [Logging and tracing](#6-logging-and-tracing)
7. [Kubernetes integration](#7-kubernetes-integration)
8. [eBPF maps](#8-ebpf-maps)
9. [Netlink / Linux kernel interfaces](#9-netlink--linux-kernel-interfaces)
10. [Cilium-specific idioms](#10-cilium-specific-idioms)
11. [Writing tests](#11-writing-tests)
12. [Validation with ginkgo](#12-validation-with-ginkgo)
13. [Checklist](#13-checklist)

---

## 1. Workflow overview

```
1. Read Go source          pkg/<foo>/*.go  (not test files)
2. Identify public API     exported types + funcs
3. Map types               use §3 table
4. Map concurrency         use §4 table
5. Implement in Rust       crates/<crate>/src/lib.rs
6. Write unit tests        #[cfg(test)] at bottom of lib.rs
7. cargo test -p <crate>   all green, no warnings
8. cargo test --workspace  workspace still green
9. Run ginkgo focus group  see §12
10. Open PR                closes the track issue
```

---

## 2. Reading Go source

```bash
CILIUM=~/dev/cilium

# List all non-test files in a package
find $CILIUM/pkg/identity -name "*.go" ! -name "*_test.go" | sort

# Find where a type is defined
grep -rn "^type NumericIdentity" $CILIUM/pkg --include="*.go"

# Find all methods on a type
grep -n "func (.*NumericIdentity" $CILIUM/pkg/identity/*.go

# Count production LOC
find $CILIUM/pkg/policy -name "*.go" ! -name "*_test.go" | xargs wc -l | tail -1
```

### What to look for

| Thing to find | Why it matters |
|---|---|
| `type Foo struct` | Port this as `pub struct Foo` |
| `type Bar interface` | Port this as `pub trait Bar` |
| `func (f *Foo) Method()` | `impl Foo { pub fn method(&self) }` |
| `var ErrFoo = errors.New(...)` | `#[error("...")] Foo` variant in error enum |
| `sync.Mutex` / `sync.RWMutex` | `tokio::sync::Mutex` / `RwLock` |
| `chan T` | `tokio::sync::mpsc::channel::<T>()` |
| `go func()` | `tokio::spawn(async { })` |
| `context.Context` | Function parameter or `CancellationToken` |
| `init()` | `once_cell::sync::Lazy` |

---

## 3. Type system translation

### Primitives

| Go | Rust |
|----|------|
| `bool` | `bool` |
| `int` | `i64` (or `isize` for counts) |
| `int32` | `i32` |
| `uint32` | `u32` |
| `uint16` | `u16` |
| `uint8` | `u8` |
| `float64` | `f64` |
| `string` | `String` (owned) or `&str` (borrowed) |
| `[]byte` | `Vec<u8>` or `bytes::Bytes` |
| `[]T` | `Vec<T>` |
| `[N]T` | `[T; N]` |
| `map[K]V` | `HashMap<K, V>` |
| `*T` (nullable) | `Option<Box<T>>` or `Option<Arc<T>>` |
| `*T` (non-null) | `&T` or `Arc<T>` |
| `interface{}` / `any` | `Box<dyn Any>` (rare) or generic `<T>` |

### Structs

```go
// Go
type Endpoint struct {
    ID       uint16
    IPv4     net.IP
    IPv6     net.IP
    Labels   labels.Labels
    Identity *identity.Identity
}
```

```rust
// Rust
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub id: u16,
    pub ipv4: Option<std::net::Ipv4Addr>,
    pub ipv6: Option<std::net::Ipv6Addr>,
    pub labels: HashMap<String, String>,
    pub identity: Option<Arc<Identity>>,
}
```

### Interfaces → Traits

```go
// Go
type PolicyRepository interface {
    AddList(rules api.Rules) (newRev uint64, err error)
    Delete(labels labels.LabelArray) (newRev uint64, deleted uint)
    Resolve(ctx context.Context, id *identity.Identity) *L4Policy
}
```

```rust
// Rust
#[async_trait::async_trait]
pub trait PolicyRepository: Send + Sync {
    async fn add_list(&self, rules: Vec<Rule>) -> Result<u64>;
    fn delete(&self, labels: &[Label]) -> (u64, usize);
    async fn resolve(&self, id: &Identity) -> Option<L4Policy>;
}
```

### Enums

```go
// Go — iota enum
type SVCType string
const (
    SVCTypeClusterIP    SVCType = "ClusterIP"
    SVCTypeNodePort     SVCType = "NodePort"
    SVCTypeLoadBalancer SVCType = "LoadBalancer"
)
```

```rust
// Rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SvcType {
    ClusterIp,
    NodePort,
    LoadBalancer,
}
```

### Newtype wrappers (Cilium uses these heavily)

```go
type NumericIdentity uint32
type EndpointID       uint16
```

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash,
         serde::Serialize, serde::Deserialize)]
pub struct NumericIdentity(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EndpointId(pub u16);

impl NumericIdentity {
    pub const WORLD: Self    = Self(1);
    pub const HOST: Self     = Self(2);
    pub const UNMANAGED: Self = Self(3);
    pub const HEALTH: Self   = Self(4);
    pub const INIT: Self     = Self(5);
    pub const LOCAL_NODE: Self = Self(6);
}
```

---

## 4. Concurrency patterns

### Goroutine → tokio::spawn

```go
// Go
go func() {
    for event := range eventChan {
        handleEvent(event)
    }
}()
```

```rust
// Rust
tokio::spawn(async move {
    while let Some(event) = rx.recv().await {
        handle_event(event).await;
    }
});
```

### Channel patterns

```go
// Go — buffered channel
ch := make(chan Event, 100)
```

```rust
// Rust — bounded mpsc
let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(100);
```

```go
// Go — broadcast (multiple receivers)
// usually via callback slices or separate channels
```

```rust
// Rust — broadcast
let (tx, _) = tokio::sync::broadcast::channel::<Event>(100);
let rx1 = tx.subscribe();
let rx2 = tx.subscribe();
```

### select! on multiple channels

```go
// Go
select {
case msg := <-ch1:
    handle1(msg)
case msg := <-ch2:
    handle2(msg)
case <-ctx.Done():
    return
}
```

```rust
// Rust
tokio::select! {
    Some(msg) = rx1.recv() => handle1(msg).await,
    Some(msg) = rx2.recv() => handle2(msg).await,
    _ = token.cancelled()  => return,
}
```

### Shared mutable state

```go
// Go
type Cache struct {
    mu    sync.RWMutex
    items map[string]Item
}
func (c *Cache) Get(k string) (Item, bool) {
    c.mu.RLock()
    defer c.mu.RUnlock()
    v, ok := c.items[k]
    return v, ok
}
```

```rust
// Rust
pub struct Cache {
    items: Arc<RwLock<HashMap<String, Item>>>,
}
impl Cache {
    pub async fn get(&self, k: &str) -> Option<Item> {
        self.items.read().await.get(k).cloned()
    }
}
// Or use DashMap for lock-free concurrent access:
use dashmap::DashMap;
pub struct Cache { items: Arc<DashMap<String, Item>> }
impl Cache {
    pub fn get(&self, k: &str) -> Option<Item> {
        self.items.get(k).map(|v| v.clone())
    }
}
```

### Background worker (cell/job.Group pattern)

```go
// Go — hive job group
func (m *Manager) Start(ctx context.Context) error {
    m.jobs.Add(job.OneShot("reconcile-loop", m.reconcileLoop))
    return nil
}
```

```rust
// Rust — tokio JoinSet
pub struct Manager {
    tasks: tokio::task::JoinSet<()>,
}
impl Manager {
    pub async fn start(&mut self) {
        self.tasks.spawn(Self::reconcile_loop());
    }
    async fn reconcile_loop() { /* ... */ }
}
```

---

## 5. Error handling

### Define errors with thiserror

```go
// Go
var (
    ErrNotFound = errors.New("not found")
    ErrInvalid  = errors.New("invalid argument")
)
func DoThing() error {
    return fmt.Errorf("do thing: %w", ErrNotFound)
}
```

```rust
// Rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("IO error")]
    Io(#[from] std::io::Error),

    #[error("Kubernetes error")]
    Kube(#[from] kube::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// Usage
fn do_thing() -> Result<()> {
    Err(Error::NotFound("endpoint-42".into()))
}
```

### Context wrapping (anyhow for leaf errors)

```go
fmt.Errorf("loading policy: %w", err)
```

```rust
use anyhow::Context;
load_policy().context("loading policy")?;
```

---

## 6. Logging and tracing

```go
// Go — structured log
log.WithField("endpoint", epID).WithError(err).Error("policy update failed")
```

```rust
// Rust — tracing
tracing::error!(endpoint_id = %ep_id, error = %err, "policy update failed");
tracing::info!(service = %svc_name, backends = backends.len(), "service updated");
tracing::debug!(identity = ?identity, "identity resolved");
```

Instrument async functions:
```rust
#[tracing::instrument(skip(self), fields(ep_id = %self.id))]
pub async fn regenerate(&self) -> Result<()> {
    // ...
}
```

---

## 7. Kubernetes integration

Use kube-rs 0.98 with k8s-openapi 0.24/v1_32.

```go
// Go — list pods
pods, err := client.CoreV1().Pods(ns).List(ctx, metav1.ListOptions{})
```

```rust
// Rust
use kube::{Api, Client};
use k8s_openapi::api::core::v1::Pod;

let client = Client::try_default().await?;
let pods: Api<Pod> = Api::namespaced(client, "default");
let list = pods.list(&Default::default()).await?;
```

### Watching resources

```rust
use kube::runtime::watcher;
use futures::TryStreamExt;

let api: Api<Pod> = Api::all(client);
let mut stream = watcher(api, Default::default()).applied_objects();

while let Some(pod) = stream.try_next().await? {
    println!("Pod: {}", pod.name_any());
}
```

### Custom resources (CiliumNetworkPolicy)

```rust
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "cilium.io",
    version = "v2",
    kind = "CiliumNetworkPolicy",
    namespaced
)]
pub struct CiliumNetworkPolicySpec {
    pub egress:  Option<Vec<EgressRule>>,
    pub ingress: Option<Vec<IngressRule>>,
}
```

---

## 8. eBPF maps

Use aya-rs. The C eBPF programs stay in `bpf/*.c`; the Rust agent just loads them and reads/writes their maps.

```go
// Go — update eBPF map
m.Update(key, value, ebpf.UpdateNoExist)
```

```rust
// Rust — aya
use aya::maps::HashMap;

let mut map: HashMap<_, u32, u32> = HashMap::try_from(bpf.map_mut("my_map")?)?;
map.insert(42u32, 100u32, 0)?;  // 0 = any flags

let val: u32 = map.get(&42u32, 0)?;
```

### Map types

| Cilium Go map type | aya-rs type |
|---|---|
| `ebpf.Hash` | `aya::maps::HashMap` |
| `ebpf.LRUHash` | `aya::maps::LruHashMap` |
| `ebpf.PerCPUHash` | `aya::maps::PerCpuHashMap` |
| `ebpf.Array` | `aya::maps::Array` |
| `ebpf.PerCPUArray` | `aya::maps::PerCpuArray` |
| `ebpf.LPMTrie` | `aya::maps::LpmTrie` |
| `ebpf.ProgramArray` | `aya::maps::ProgramArray` |
| `ebpf.PerfEventArray` | `aya::maps::PerfEventArray` |
| `ebpf.RingBuf` | `aya::maps::RingBuf` |

### Loading programs from ELF

```rust
use aya::Ebpf;

let mut bpf = Ebpf::load(include_bytes_aligned!("../../bpf/bpf_lxc.o"))?;
// Or at runtime:
let mut bpf = Ebpf::load_file("/var/lib/cilium/bpf/bpf_lxc.o")?;
```

### Attaching tc programs

```rust
use aya::programs::{tc, SchedClassifier, TcAttachType};

let prog: &mut SchedClassifier = bpf.program_mut("handle_egress")?.try_into()?;
prog.load()?;
tc::qdisc_add_clsact("eth0")?;
prog.attach("eth0", TcAttachType::Egress)?;
```

---

## 9. Netlink / Linux kernel interfaces

Use the `rtnetlink` and `nix` crates for kernel interfaces.

```go
// Go — create veth pair
link := &netlink.Veth{
    LinkAttrs: netlink.LinkAttrs{Name: "lxc12345"},
    PeerName:  "eth0",
}
netlink.LinkAdd(link)
```

```rust
// Rust — rtnetlink
use rtnetlink::Handle;

async fn create_veth(handle: &Handle, name: &str, peer: &str) -> anyhow::Result<()> {
    handle.link().add()
        .veth(name.into(), peer.into())
        .execute()
        .await?;
    Ok(())
}
```

### Common netlink operations

```rust
// Bring link up
handle.link().set(index).up().execute().await?;

// Add IP address
handle.address().add(index, ip.into(), prefix_len).execute().await?;

// Add route
handle.route().add()
    .v4()
    .destination_prefix(dst, prefix_len)
    .gateway(gw)
    .execute()
    .await?;

// Move link into netns
handle.link().set(index).setns_by_pid(pid).execute().await?;
```

---

## 10. Cilium-specific idioms

### Labels (source:key=value format)

```go
// Cilium label format: "source:key=value"
l := labels.NewLabel("app", "frontend", "k8s")
// → "k8s:app=frontend"
```

```rust
pub fn format_label(source: &str, key: &str, value: &str) -> String {
    format!("{source}:{key}={value}")
}
// "k8s:app=frontend"
```

### Identity reserved ranges

```rust
impl NumericIdentity {
    /// Reserved identities (0–9)
    pub const WORLD: Self         = Self(1);
    pub const HOST: Self          = Self(2);
    pub const UNMANAGED: Self     = Self(3);
    pub const HEALTH: Self        = Self(4);
    pub const INIT: Self          = Self(5);
    pub const LOCAL_NODE: Self    = Self(6);
    pub const REMOTE_NODE: Self   = Self(7);
    pub const INGRESS: Self       = Self(8);
    pub const WORLD_IPV4: Self    = Self(9);

    /// Cluster-local allocated range start
    pub const MIN_CLUSTER_LOCAL: Self = Self(256);
}
```

### Endpoint state machine

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointState {
    Creating,
    WaitingForIdentity,
    WaitingToRegenerate,
    Regenerating,
    Ready,
    Disconnecting,
    Disconnected,
    Invalid,
}
```

### Policy verdict

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyVerdict {
    Allow,
    Deny,
    Redirect, // to L7 proxy
}
```

---

## 11. Writing tests

### Unit tests (in lib.rs)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Naming: test_<type>_<scenario>
    #[test]
    fn test_numeric_identity_reserved_are_correct() {
        assert_eq!(NumericIdentity::HOST.0, 2);
        assert_eq!(NumericIdentity::WORLD.0, 1);
    }

    #[tokio::test]
    async fn test_policy_cache_add_and_retrieve() {
        let cache = PolicyCache::new();
        let policy = test_policy("default", "allow-web");

        cache.add(policy.clone()).await;
        let policies = cache.list("default").await;

        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].name, "allow-web");
    }

    // Tests requiring root/kernel: mark #[ignore]
    #[test]
    #[ignore = "requires root and eBPF kernel support"]
    fn test_bpf_hash_map_real_kernel() {
        // ...
    }

    // Helper: build test fixtures
    fn test_policy(namespace: &str, name: &str) -> NetworkPolicy {
        NetworkPolicy {
            name: name.to_string(),
            namespace: namespace.to_string(),
            ..Default::default()
        }
    }
}
```

### Run tests

```bash
# Single crate
cargo test -p seriousum-policy -- --nocapture

# Single test
cargo test -p seriousum-policy test_policy_cache_add_and_retrieve

# All tests including ignored (needs root)
sudo cargo test --workspace -- --include-ignored
```

---

## 12. Validation with ginkgo

After implementing a track, validate against the upstream Cilium integration tests.

### Load the test skill

```
/skill:cilium-test
```

### Quick validation (one focus group)

```bash
cd ~/dev/seriousum

# 1. Build
cargo build --release --locked
docker build -f images/cilium-agent.Dockerfile -t seriousum-agent:dev .

# 2. Run the matching focus group for your track
./scripts/run-cilium-kind-test.sh \
  --focus "K8sAgentPolicyTest" \   # ← change to match your track
  --timeout 45m
```

### Track → focus group mapping

| Track | Focus regex |
|-------|-------------|
| A (eBPF maps) | `K8sDatapathServicesTest` (map-driven) |
| B (Datapath) | `K8sDatapathServicesTest` |
| C (CNI) | Any (CNI required for all) |
| D (K8s watchers) | `K8sAgentFQDNTest` |
| E (Identity) | `K8sAgentPolicyTest` |
| F (Policy) | `K8sAgentPolicyTest` |
| G (Endpoint) | `K8sAgentChaosTest` |
| H (IPAM) | Any (IPAM required for all) |
| I (LB) | `K8sDatapathServicesTest` |
| J (kvstore) | `K8sAgentPolicyTest` |
| K (FQDN) | `K8sAgentFQDNTest` |
| L (Hubble) | `K8sAgentHubbleTest` |
| R (Operator) | All suites |

### Pass criteria

| Pass rate | Meaning |
|-----------|---------|
| < 40% | Core functionality missing — keep implementing |
| 40–70% | Partially working — fix most common failures |
| 70–85% | Good — some edge cases remain |
| ≥ 85% | ✅ Ready to merge |

---

## 13. Checklist

Before opening a PR for a porting track:

- [ ] Read all `.go` files in the target package
- [ ] All exported types and functions ported
- [ ] Unit tests for every ported function (min 1 success + 1 error case)
- [ ] `cargo test --workspace` — 0 failures
- [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings
- [ ] `cargo fmt -- --check` — passes
- [ ] Ginkgo focus group for this track passes at ≥ 80%
- [ ] Doc comments on all `pub` items
- [ ] No `unwrap()` / `expect()` in non-test code
- [ ] Branch named `port/track-<letter>-<short-name>`
- [ ] PR references the track issue (`Closes #22`)
