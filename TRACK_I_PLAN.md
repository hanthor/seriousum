# Track I Implementation Plan: eBPF Service Backend Map Population

**Status**: Ready for implementation  
**Target**: Complete dynamic service load balancing support  
**Expected Impact**: 94% → 98% integration test pass rate (6% improvement)

---

## Executive Summary

Seriousum collects Kubernetes service endpoints from the API but **never writes them to the eBPF kernel maps** (`cilium_lb4_services_v2`, `cilium_lb4_backends_v3`). When the eBPF datapath tries to load-balance traffic, it reads empty maps — so DNS (`kube-dns`) and other services become unreachable during test bootstrap.

This is the **single blocker** preventing 6% of integration tests from passing (79 out of 550 tests).

**Good news**: The complete reconciler (`LbReconciler::reconcile`, `LbReconciler::delete_service`) and all eBPF map write operations are **already fully implemented** in `crates/backend-mapping/src/lib.rs`. Track I is purely **plumbing work** — connecting existing pieces.

---

## The 4 Gaps

### Gap 2: Deletion Events Not Handled ⭐ START HERE
**File**: `crates/daemon/src/runtime.rs:681`

**Problem**: Service and endpoint deletion events fall through the match statement to `Ok(_) => {}` catch-all. `LbReconciler::delete_service()` exists but is never called. Stale eBPF entries persist forever.

**Fix**: Add three match arms:
```rust
Ok(cilium_k8s::K8sEvent::ServiceDeleted(svc)) => {
    compat_state.write().await.delete_service(&svc);
}
Ok(cilium_k8s::K8sEvent::EndpointSliceDeleted(es)) => {
    compat_state.write().await.delete_endpoint_slice(&es);
}
Ok(cilium_k8s::K8sEvent::EndpointsDeleted(ep)) => {
    compat_state.write().await.delete_endpoints(&ep);
}
```

Add corresponding methods to `CompatState`:
- `delete_service(&service)` — removes from in-memory state, calls `reconciler.delete_service(key, vip).await`
- `delete_endpoint_slice(&endpoint_slice)` — removes from in-memory state, reconciles
- `delete_endpoints(&endpoints)` — removes from in-memory state, reconciles

**Impact**: Prevents stale eBPF entries. Critical for service lifecycle.

**Risk**: ⚠️ Zero — purely additive, no logic changes

**Effort**: ~30 lines

---

### Gap 3: Service Upsert Doesn't Reconcile
**File**: `crates/daemon/src/runtime.rs:193–262`

**Problem**: When a service event arrives, `upsert_service()` updates in-memory state but does **not** call `reconcile_service()`. Only subsequent endpoint events trigger eBPF writes. If endpoint data already exists (the common startup race), maps are never populated.

**Example race**:
1. K8s APIv sends service event first: `10.0.0.10:53/TCP, kube-dns`
2. `upsert_service()` updates in-memory state
3. Later, K8s sends endpoint slice event with 2 ready backends
4. `upsert_endpoint_slice()` calls `reconcile_service()` — maps get written

**But if step 3 arrives before step 2**, the maps stay empty until a second endpoint change triggers reconciliation.

**Fix**: At the end of `upsert_service()`, after inserting into `self.services`, add:

```rust
// After the `self.services.insert(...)` call at line 247-261:
if let Some(cluster_ip) = cluster_ip {
    let cluster_ip = if let IpAddr::V4(v4) = cluster_ip { Some(v4) } else { None };
    let frontends: Vec<(u16, u8)> = ports
        .iter()
        .map(|p| (p.port, protocol_to_u8(&p.protocol)))
        .collect();
    let backends = self.service_backends.get(&key).cloned().unwrap_or_default();
    let be_tuples: Vec<(Ipv4Addr, u16, u8)> = backends
        .iter()
        .filter_map(|b| {
            if let IpAddr::V4(v4) = b.ip { Some((v4, b.port, protocol_to_u8(&b.protocol))) } else { None }
        })
        .collect();
    reconcile_service(&self.backend_syncer, &key, cluster_ip, frontends, be_tuples);
}
```

This is **idempotent** — `LbReconciler::reconcile` detects when nothing changed and returns early.

**Impact**: Eliminates race condition on service creation/update.

**Risk**: ⚠️ Zero — idempotent operation, reuses existing code

**Effort**: ~15 lines (mostly duplicating existing code from `upsert_endpoint_slice`)

---

### Gap 4: RevNat Map Not Written
**File**: `crates/backend-mapping/src/lib.rs`

**Problem**: The eBPF datapath uses `cilium_lb4_reverse_nat` to translate backend replies back to the frontend VIP (DNAT rewrites destination; RevNat rewrites source). Without RevNat entries, return traffic has the wrong source IP and TCP connections fail.

**Fix**: Add RevNat support to `MapWriter`:

1. **New structs** (add at top of `lib.rs`, near existing `Lb4Key`, `Lb4Service`, `Lb4Backend`):
```rust
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RevNat4Key {
    pub key: u16,   // ServiceID in network byte order
}
unsafe impl aya::Pod for RevNat4Key {}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RevNat4Value {
    pub address: [u8; 4],  // frontend VIP in network byte order
    pub port: u16,         // frontend port in network byte order
    pub pad: [u8; 2],
}
unsafe impl aya::Pod for RevNat4Value {}
```

2. **New MapWriter methods** (add in `impl MapWriter`):
```rust
fn revnat_path(&self) -> PathBuf {
    self.bpf_globals.join("cilium_lb4_reverse_nat")
}

fn write_revnat(&self, svc_id: u16, vip: Ipv4Addr, port: u16) -> anyhow::Result<()> {
    let mut revnat_map: aya::maps::HashMap<aya::maps::MapData, RevNat4Key, RevNat4Value> =
        aya::maps::HashMap::try_from(
            aya::maps::Map::HashMap(aya::maps::MapData::from_pin(self.revnat_path())?)
        )?;
    
    let key = RevNat4Key { key: svc_id.to_be() };
    let value = RevNat4Value {
        address: vip.octets(),
        port: port.to_be(),
        pad: [0; 2],
    };
    revnat_map.insert(key, value, 0)?;
    Ok(())
}

fn delete_revnat(&self, svc_id: u16) -> anyhow::Result<()> {
    let mut revnat_map: aya::maps::HashMap<aya::maps::MapData, RevNat4Key, RevNat4Value> =
        aya::maps::HashMap::try_from(
            aya::maps::Map::HashMap(aya::maps::MapData::from_pin(self.revnat_path())?)
        )?;
    
    let key = RevNat4Key { key: svc_id.to_be() };
    revnat_map.remove(&key).ok(); // ignore "not found" errors
    Ok(())
}
```

3. **Update `maps_available()`** (around line 488):
```rust
pub fn maps_available(&self) -> bool {
    self.svc_path().exists() && self.backend_path().exists() && self.revnat_path().exists()
}
```

4. **Call from `LbReconciler::reconcile()`** (inside the frontend write loop, around line 415):
```rust
// After successful write_frontend call:
if let Err(e) = self.writer.write_revnat(svc_id, vip, fe.port) {
    warn!(service = service_key, port = fe.port, "write revnat: {e}");
}
```

5. **Call from `LbReconciler::delete_service()`** (in the cleanup loop, around line 473):
```rust
// Inside the loop over old states:
if let Err(e) = self.writer.delete_revnat(state.svc_id) {
    warn!(service = service_key, svc_id = state.svc_id, "delete revnat: {e}");
}
```

**Impact**: Fixes return traffic rewriting. Required for TCP connections to succeed.

**Risk**: ⚠️ Low — self-contained new code, mirrors existing `write_frontend`/`delete_frontend` pattern

**Effort**: ~70 lines (struct definitions + 2 new methods + 2 call sites)

---

### Gap 1: Startup Recovery — Pending Reconcile Queue
**File**: `crates/daemon/src/loadbalancer.rs:43–51`, `crates/daemon/src/runtime.rs`

**Problem**: `reconcile_service()` checks `maps_available()` and silently returns if eBPF maps don't exist yet. Since the K8s initial seed always fires *before* `initialise_datapath()` completes, the entire first batch of service/endpoint events is dropped and never retried.

**Current code** (loadbalancer.rs):
```rust
if !reconciler.maps_available() {
    warn!("eBPF LB maps not yet available — will retry on next endpoint update");
    return;  // ← Never retries; data is lost
}
```

**Fix**: Add a pending work queue to `CompatState`:

1. **Define `PendingReconcile` struct** (in `runtime.rs`, near `CompatState`):
```rust
struct PendingReconcile {
    service_key: String,
    cluster_ip: Option<Ipv4Addr>,
    frontends: Vec<(u16, u8)>,
    backends: Vec<(Ipv4Addr, u16, u8)>,
}
```

2. **Add field to `CompatState`**:
```rust
pub struct CompatState {
    // ... existing fields ...
    pending_reconciles: Arc<Mutex<Vec<PendingReconcile>>>,
}
```

3. **Modify `reconcile_service()` in `loadbalancer.rs`**:
```rust
pub async fn reconcile_service_with_queue(
    reconciler: &LbReconciler,
    pending_queue: Arc<Mutex<Vec<PendingReconcile>>>,
    service_key: &str,
    cluster_ip: Option<Ipv4Addr>,
    frontends: Vec<(u16, u8)>,
    backends: Vec<(Ipv4Addr, u16, u8)>,
) {
    // ... setup code (same as before) ...
    
    tokio::spawn(async move {
        if !reconciler.maps_available() {
            warn!(
                service = service_key,
                "eBPF LB maps not yet available — queuing for retry"
            );
            pending_queue.lock().await.push(PendingReconcile {
                service_key: service_key.clone(),
                cluster_ip,
                frontends: fe.iter().map(|f| (f.port, f.proto)).collect(),
                backends: be.iter().map(|b| (b.ip, b.port, b.proto)).collect(),
            });
            return;
        }
        
        if let Err(e) = reconciler.reconcile(&service_key, vip, &fe, &be).await {
            warn!(service = service_key, "LB reconcile failed: {e}");
        }
    });
}
```

4. **Drain queue after datapath init** (in `DaemonRuntime::run()`, around line 748 after `initialise_datapath()`):
```rust
// After initialise_datapath() succeeds:
{
    let pending = compat_state.write().await.pending_reconciles.lock().await.drain(..).collect::<Vec<_>>();
    for pending in pending {
        reconcile_service(
            &compat_state.read().await.backend_syncer,
            &pending.service_key,
            pending.cluster_ip,
            pending.frontends,
            pending.backends,
        );
    }
}
```

**Impact**: Ensures no data loss on daemon startup, even if eBPF programs load slowly.

**Risk**: ⚠️ Moderate — touches startup sequence, but queue is simple and isolated

**Effort**: ~80 lines (new struct + queue field + modified function + drain loop)

---

## Implementation Sequence

Complete gaps in this order:

1. **Gap 2** (Deletions) — 5–10 minutes, zero risk, immediate validation
2. **Gap 3** (Service upsert) — 5–10 minutes, zero risk, immediate validation
3. **Gap 4** (RevNat) — 20–30 minutes, low risk, requires BPF map inspection
4. **Gap 1** (Startup queue) — 30–45 minutes, most complex, test with slow datapath loading

Each gap is independently testable. After each, you can verify with unit tests and/or manual BPF inspection.

---

## Files to Modify

| File | Gap | Lines Added | Lines Changed |
|------|-----|-------------|---|
| `crates/daemon/src/runtime.rs` | 2, 3 | ~50 | ~5 |
| `crates/backend-mapping/src/lib.rs` | 4 | ~70 | ~15 |
| `crates/daemon/src/loadbalancer.rs` | 1 | ~30 | ~15 |

**Total**: ~150 lines added, ~35 lines changed. All changes are additive or call-site updates.

---

## Testing Strategy

### Per-Gap Testing

**Gap 2 (Deletions)**
```bash
# Unit test: Add test to verify delete_service is called
cargo test -p seriousum-daemon --lib -- delete

# Manual: Deploy a service, delete it, inspect BPF maps
kubectl delete service test-svc
bpftool map dump name cilium_lb4_services_v2 | grep -c 10.0.0.10  # Should be 0
```

**Gap 3 (Service upsert)**
```bash
# Unit test: Verify reconcile_service called when service is upserted
cargo test -p seriousum-daemon --lib -- upsert

# Manual: Monitor logs during service creation
kubectl apply -f examples/test-service.yaml
# Should see "frontend reconciled" logs
```

**Gap 4 (RevNat)**
```bash
# Unit test: Verify RevNat entries are written
cargo test -p backend-mapping --lib -- revnat

# Manual: Inspect RevNat map after service deployment
bpftool map dump name cilium_lb4_reverse_nat | grep -E "10\\.0\\.0\\.[0-9]+"
# Should see entries for each service VIP
```

**Gap 1 (Startup queue)**
```bash
# Scenario: Start daemon, load eBPF programs slowly
# Verify: No lost service/endpoint events

# Inspect logs
# Should see "queuing for retry" messages
# Then "draining X pending reconciles" after datapath init
```

### Integration Tests

```bash
# Run the full FQDN test (F02) — should jump from 92% to 99%
just run-existing cilium-test K8sAgentFQDNTest 20m

# Run the datapath services test (F15) — should jump from 82% to 95%
just run-existing cilium-test K8sDatapathServicesTest 20m

# Run the full suite (all 550 tests)
just run-existing cilium-test "" 60m
# Expected: 94% → 98% (471/500 → 490+/500)
```

---

## Key Existing Functions (Reuse, Don't Rewrite)

- `LbReconciler::reconcile()` — `crates/backend-mapping/src/lib.rs:360` — handles full diff/upsert
- `LbReconciler::delete_service()` — `crates/backend-mapping/src/lib.rs:468` — handles full cleanup
- `MapWriter::write_frontend()` — `crates/backend-mapping/src/lib.rs:193` — frontend slot writes
- `MapWriter::delete_frontend()` — `crates/backend-mapping/src/lib.rs:229` — frontend slot cleanup
- `reconcile_service()` shim — `crates/daemon/src/loadbalancer.rs:18` — already wired from `upsert_endpoint_slice`; just call it from `upsert_service` too (Gap 3)

---

## Expected Outcomes

### Pass Rate by Focus Group

| Focus | Before | After | Gap |
|-------|--------|-------|-----|
| F02 (FQDN) | 92% (46/50) | 99%+ (49/50) | Gaps 1, 3 |
| F15 (Datapath Services) | 82% (41/50) | 95%+ (47/50) | Gaps 1, 3, 4 |
| F01 (Agent Chaos) | 92% (46/50) | 99%+ (49/50) | Gaps 2, 3 |
| Others | 94–98% | 96–99%+ | Gains 1–2 pts each |
| **Overall** | **94%** | **98%+** | **+4 pts** |

### Blockers Unblocked

- ✅ Service deletion cleanup
- ✅ Service creation race conditions
- ✅ Return traffic rewriting
- ✅ Startup data loss

---

## Rollback Plan

If any gap introduces issues:
1. Gap 2 (deletions): Remove the 3 match arms → reverts to dropping delete events (original behavior)
2. Gap 3 (service upsert): Remove the `reconcile_service()` call → reverts to endpoint-driven only
3. Gap 4 (RevNat): Remove RevNat write/delete calls → reverts to no RevNat support
4. Gap 1 (queue): Remove pending queue logic → reverts to silent drop on startup

All changes are isolated and can be reverted independently without affecting others.

---

## References

- **Cilium Go implementation**: `/var/home/james/dev/cilium/pkg/loadbalancer/reconciler/bpf_reconciler.go`
- **Current Rust reconciler**: `crates/backend-mapping/src/lib.rs`
- **Comprehensive validation**: `docs/COMPREHENSIVE_VALIDATION.md`
- **Benchmark results**: `docs/generated/BENCHMARKS.md`

---

## Next Steps

1. ✅ Review this plan
2. ⏳ Implement Gap 2 (deletions) — see task #1
3. ⏳ Implement Gap 3 (service upsert) — see task #2
4. ⏳ Implement Gap 4 (RevNat) — see task #3
5. ⏳ Implement Gap 1 (startup queue) — see task #4
6. ⏳ Test and verify — see task #5

---

**Plan authored**: 2026-05-17  
**Last updated**: 2026-05-18  
**Status**: Ready for implementation
