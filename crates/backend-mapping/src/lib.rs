//! Track I: LB reconciler — syncs Kubernetes service backends to eBPF maps.
//!
//! Cilium's eBPF datapath forwards ClusterIP traffic by looking up the service
//! frontend in `cilium_lb4_services_v2` and the backend in `cilium_lb4_backends_v3`.
//! This reconciler keeps those maps consistent with what the Kubernetes API reports.
//!
//! ## Map layout
//!
//! `cilium_lb4_services_v2` (key=`Lb4Key`, value=`Lb4Service`):
//! - Slot 0 (master): count=N backends, rev_nat_index=svc_id
//! - Slots 1..=N: backend_id pointing into `cilium_lb4_backends_v3`
//!
//! `cilium_lb4_backends_v3` (key=u32 backend_id, value=`Lb4Backend`):
//! - One entry per unique (backend_ip, port, proto)
//!
//! ## ID stability
//!
//! Backend IDs are allocated once per unique (ip, port, proto) tuple and reused
//! across services. Service (rev_nat) IDs track the service key stable across
//! endpoint slice updates.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// ─── eBPF map constants ───────────────────────────────────────────────────────

pub const SVC_MAP_NAME: &str = "cilium_lb4_services_v2";
pub const BACKEND_MAP_NAME: &str = "cilium_lb4_backends_v3";
const BPF_GLOBALS: &str = "/sys/fs/bpf/tc/globals";

// ─── Struct layout matching Cilium's lb4_key (12 bytes) ───────────────────────

/// Key for `cilium_lb4_services_v2`.
///
/// Must match `struct lb4_key` in `bpf/lib/common.h` exactly.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lb4Key {
    /// Virtual IP in **network** (big-endian) byte order.
    pub address: [u8; 4],
    /// Frontend port in **network** byte order.
    pub dport: u16,
    /// 0 = master entry, 1..=N = backend slot index.
    pub backend_slot: u16,
    /// IP protocol: IPPROTO_TCP=6, IPPROTO_UDP=17.
    pub proto: u8,
    /// Lookup scope. 0 = external (LB_LOOKUP_SCOPE_EXT).
    pub scope: u8,
    pub pad: [u8; 2],
}

// Safety: Lb4Key is #[repr(C)], Copy, contains only primitive types.
#[allow(unsafe_code)]
unsafe impl aya::Pod for Lb4Key {}

impl Lb4Key {
    pub fn new(vip: Ipv4Addr, port: u16, proto: u8, slot: u16) -> Self {
        Self {
            address: vip.octets(),
            dport: port.to_be(),
            backend_slot: slot,
            proto,
            scope: 0,
            pad: [0; 2],
        }
    }
}

// ─── Struct layout matching Cilium's lb4_service (12 bytes) ───────────────────

/// Value for `cilium_lb4_services_v2`.
///
/// Must match `struct lb4_service` in `bpf/lib/common.h` exactly.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Lb4Service {
    /// Backend ID for slot entries, 0 for master entry.
    pub backend_id: u32,
    /// Number of active backends (master entry only).
    pub count: u16,
    /// Reverse NAT index (big-endian). Equals service ID.
    pub rev_nat_index: u16,
    pub flags: u8,
    pub flags2: u8,
    /// Number of quarantined backends.
    pub qcount: u16,
}

// Safety: Lb4Service is #[repr(C)], Copy, contains only primitive types.
#[allow(unsafe_code)]
unsafe impl aya::Pod for Lb4Service {}

// ─── Struct layout matching Cilium's lb4_backend (v3, 12 bytes) ───────────────

/// Value for `cilium_lb4_backends_v3`.
///
/// Must match `struct lb4_backend` in `bpf/lib/common.h` exactly.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Lb4Backend {
    /// Backend IP in **network** byte order.
    pub address: [u8; 4],
    /// Backend port in **network** byte order.
    pub port: u16,
    /// IP protocol: IPPROTO_TCP=6, IPPROTO_UDP=17.
    pub proto: u8,
    /// Backend state flags. 0 = active (BE_STATE_ACTIVE).
    pub flags: u8,
    /// Cluster ID. 0 = local cluster.
    pub cluster_id: u16,
    /// Zone. 0 = no zone.
    pub zone: u8,
    pub pad: u8,
}

// Safety: Lb4Backend is #[repr(C)], Copy, contains only primitive types.
#[allow(unsafe_code)]
unsafe impl aya::Pod for Lb4Backend {}

impl Lb4Backend {
    pub fn new(ip: Ipv4Addr, port: u16, proto: u8) -> Self {
        Self {
            address: ip.octets(),
            port: port.to_be(),
            proto,
            flags: 0,
            cluster_id: 0,
            zone: 0,
            pad: 0,
        }
    }
}

// ─── Reverse NAT map (cilium_lb4_reverse_nat) ─────────────────────────────────

/// Key for `cilium_lb4_reverse_nat` — maps ServiceID back to frontend VIP+port.
/// Size: 2 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RevNat4Key {
    pub key: u16, // ServiceID in network byte order
}
#[allow(unsafe_code)]
unsafe impl aya::Pod for RevNat4Key {}

/// Value for `cilium_lb4_reverse_nat` — frontend VIP + port for return traffic rewriting.
/// Size: 8 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RevNat4Value {
    pub address: [u8; 4], // frontend VIP in network byte order
    pub port: u16,        // frontend port in network byte order
    pub pad: [u8; 2],
}
#[allow(unsafe_code)]
unsafe impl aya::Pod for RevNat4Value {}

// ─── Backend identity (for ID allocation) ─────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BackendIdentity {
    ip: Ipv4Addr,
    port: u16,
    proto: u8,
}

// ─── Per-service state ─────────────────────────────────────────────────────────

/// What we wrote to the map for one (vip, port, proto) frontend.
#[derive(Clone, Debug)]
struct FrontendState {
    svc_id: u32,
    /// Backend IDs in slot order.
    backend_ids: Vec<u32>,
}

// ─── Map writer ───────────────────────────────────────────────────────────────

/// Opens `cilium_lb4_services_v2` and `cilium_lb4_backends_v3` and writes entries.
///
/// Returns an error if the maps don't exist (eBPF programs not loaded yet).
#[derive(Debug)]
struct MapWriter {
    bpf_globals: PathBuf,
}

impl MapWriter {
    fn new(bpf_globals: impl Into<PathBuf>) -> Self {
        Self {
            bpf_globals: bpf_globals.into(),
        }
    }

    fn svc_path(&self) -> PathBuf {
        self.bpf_globals.join(SVC_MAP_NAME)
    }

    fn backend_path(&self) -> PathBuf {
        self.bpf_globals.join(BACKEND_MAP_NAME)
    }

    fn revnat_path(&self) -> PathBuf {
        self.bpf_globals.join("cilium_lb4_reverse_nat")
    }

    fn open_svc_map(
        &self,
    ) -> anyhow::Result<aya::maps::HashMap<aya::maps::MapData, Lb4Key, Lb4Service>> {
        let map_data = aya::maps::MapData::from_pin(&self.svc_path())
            .map_err(|e| anyhow::anyhow!("open {}: {e}", self.svc_path().display()))?;
        aya::maps::HashMap::try_from(aya::maps::Map::HashMap(map_data))
            .map_err(|e| anyhow::anyhow!("cast {}: {e}", SVC_MAP_NAME))
    }

    fn open_backend_map(
        &self,
    ) -> anyhow::Result<aya::maps::HashMap<aya::maps::MapData, u32, Lb4Backend>> {
        let map_data = aya::maps::MapData::from_pin(&self.backend_path())
            .map_err(|e| anyhow::anyhow!("open {}: {e}", self.backend_path().display()))?;
        aya::maps::HashMap::try_from(aya::maps::Map::HashMap(map_data))
            .map_err(|e| anyhow::anyhow!("cast {}: {e}", BACKEND_MAP_NAME))
    }

    /// Writes the master entry (slot 0) and all backend slot entries for one frontend.
    fn write_frontend(
        &self,
        vip: Ipv4Addr,
        port: u16,
        proto: u8,
        svc_id: u32,
        backend_ids: &[u32],
    ) -> anyhow::Result<()> {
        let mut svc_map = self.open_svc_map()?;

        // Master entry: slot 0
        let master_key = Lb4Key::new(vip, port, proto, 0);
        let master_val = Lb4Service {
            backend_id: 0,
            count: backend_ids.len() as u16,
            rev_nat_index: (svc_id as u16).to_be(),
            ..Default::default()
        };
        svc_map.insert(&master_key, &master_val, 0)?;

        // Backend slot entries: slots 1..=N
        for (i, &backend_id) in backend_ids.iter().enumerate() {
            let slot_key = Lb4Key::new(vip, port, proto, (i + 1) as u16);
            let slot_val = Lb4Service {
                backend_id,
                count: 0,
                rev_nat_index: (svc_id as u16).to_be(),
                ..Default::default()
            };
            svc_map.insert(&slot_key, &slot_val, 0)?;
        }

        Ok(())
    }

    /// Deletes map entries for a frontend (master + up to `old_count` backend slots).
    fn delete_frontend(
        &self,
        vip: Ipv4Addr,
        port: u16,
        proto: u8,
        slot_count: usize,
    ) -> anyhow::Result<()> {
        let mut svc_map = self.open_svc_map()?;
        for slot in 0..=(slot_count as u16) {
            let key = Lb4Key::new(vip, port, proto, slot);
            let _ = svc_map.remove(&key); // ignore "not found"
        }
        Ok(())
    }

    /// Writes a backend entry.
    fn write_backend(&self, backend_id: u32, backend: &Lb4Backend) -> anyhow::Result<()> {
        let mut backend_map = self.open_backend_map()?;
        backend_map.insert(&backend_id, backend, 0)?;
        Ok(())
    }

    /// Deletes a backend entry.
    fn delete_backend(&self, backend_id: u32) -> anyhow::Result<()> {
        let mut backend_map = self.open_backend_map()?;
        let _ = backend_map.remove(&backend_id);
        Ok(())
    }

    fn open_revnat_map(
        &self,
    ) -> anyhow::Result<aya::maps::HashMap<aya::maps::MapData, RevNat4Key, RevNat4Value>> {
        let map_data = aya::maps::MapData::from_pin(&self.revnat_path())
            .map_err(|e| anyhow::anyhow!("open {}: {e}", self.revnat_path().display()))?;
        aya::maps::HashMap::try_from(aya::maps::Map::HashMap(map_data))
            .map_err(|e| anyhow::anyhow!("cast cilium_lb4_reverse_nat: {e}"))
    }

    fn write_revnat(&self, svc_id: u32, vip: Ipv4Addr, port: u16) -> anyhow::Result<()> {
        let mut revnat_map = self.open_revnat_map()?;
        let key = RevNat4Key {
            key: svc_id.to_be() as u16,
        };
        let value = RevNat4Value {
            address: vip.octets(),
            port: port.to_be(),
            pad: [0; 2],
        };
        revnat_map.insert(key, value, 0)?;
        Ok(())
    }

    fn delete_revnat(&self, svc_id: u32) -> anyhow::Result<()> {
        let mut revnat_map = self.open_revnat_map()?;
        let key = RevNat4Key {
            key: svc_id.to_be() as u16,
        };
        let _ = revnat_map.remove(&key);
        Ok(())
    }
}

// ─── LbReconciler ─────────────────────────────────────────────────────────────

/// State shared across the reconciler.
#[derive(Debug, Default)]
struct Inner {
    /// (ip, port, proto) → allocated backend ID
    backend_ids: HashMap<BackendIdentity, u32>,
    /// backend_id → reference count (how many frontends reference this backend)
    backend_refs: HashMap<u32, u32>,
    /// service key (namespace/name) → per-port frontend states
    service_state: HashMap<String, Vec<(u16, u8, FrontendState)>>, // (port, proto, state)
    /// next service ID to allocate
    next_svc_id: u32,
}

impl Inner {
    fn alloc_svc_id(&mut self) -> u32 {
        let id = self.next_svc_id;
        self.next_svc_id = self.next_svc_id.saturating_add(1);
        id
    }

    fn get_or_alloc_backend_id(
        &mut self,
        identity: &BackendIdentity,
        next_id: &AtomicU32,
    ) -> (u32, bool) {
        if let Some(&id) = self.backend_ids.get(identity) {
            (id, false)
        } else {
            let id = next_id.fetch_add(1, Ordering::Relaxed);
            self.backend_ids.insert(identity.clone(), id);
            (id, true)
        }
    }

    fn inc_backend_ref(&mut self, id: u32) {
        *self.backend_refs.entry(id).or_insert(0) += 1;
    }

    /// Decrements reference count and returns true if the backend should be deleted.
    fn dec_backend_ref(&mut self, id: u32) -> bool {
        if let Some(count) = self.backend_refs.get_mut(&id) {
            if *count <= 1 {
                self.backend_refs.remove(&id);
                return true;
            }
            *count -= 1;
        }
        false
    }
}

/// Backend information for reconciliation.
#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub proto: u8,
}

/// Frontend port information.
#[derive(Debug, Clone)]
pub struct FrontendPort {
    pub port: u16,
    pub proto: u8,
    pub target_port: u16,
}

/// Reconciles Kubernetes services and backends into Cilium's eBPF LB maps.
///
/// Instantiate once and call [`reconcile`] whenever K8s state changes. The
/// reconciler is cheap to clone — it wraps an `Arc<Mutex<Inner>>`.
#[derive(Clone, Debug)]
pub struct LbReconciler {
    inner: Arc<Mutex<Inner>>,
    writer: Arc<MapWriter>,
    next_backend_id: Arc<AtomicU32>,
}

impl LbReconciler {
    /// Creates a reconciler that writes to maps pinned under `bpf_globals`
    /// (default: `/sys/fs/bpf/tc/globals`).
    pub fn new(bpf_globals: impl Into<PathBuf>) -> Self {
        let bpf_globals: PathBuf = bpf_globals.into();
        info!(path = %bpf_globals.display(), "LbReconciler initialized");
        Self {
            inner: Arc::new(Mutex::new(Inner {
                next_svc_id: 1,
                ..Default::default()
            })),
            writer: Arc::new(MapWriter::new(bpf_globals)),
            next_backend_id: Arc::new(AtomicU32::new(1)),
        }
    }

    /// Creates a reconciler using the default BPF globals path.
    pub fn default_path() -> Self {
        Self::new(BPF_GLOBALS)
    }

    /// Reconciles `service_key` (e.g. `"kube-system/kube-dns"`) with the
    /// given frontends (one per service port) and backends.
    ///
    /// Writes additions and deletions to the kernel maps atomically per
    /// frontend. If the maps are not accessible (eBPF not loaded yet), the
    /// call logs a warning and returns `Ok(())` — the reconciler will pick
    /// up the change on the next call once maps become available.
    pub async fn reconcile(
        &self,
        service_key: &str,
        vip: Ipv4Addr,
        frontends: &[FrontendPort],
        backends: &[BackendInfo],
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().await;

        // ── 1. Allocate backend IDs and write new backends ────────────────────
        let mut backend_id_list: Vec<u32> = Vec::with_capacity(backends.len());

        for b in backends {
            let identity = BackendIdentity {
                ip: b.ip,
                port: b.port,
                proto: b.proto,
            };
            let (id, is_new) = inner.get_or_alloc_backend_id(&identity, &self.next_backend_id);
            backend_id_list.push(id);

            if is_new {
                let entry = Lb4Backend::new(b.ip, b.port, b.proto);
                match self.writer.write_backend(id, &entry) {
                    Ok(()) => debug!(backend_id = id, ip = %b.ip, port = b.port, "backend written"),
                    Err(e) => warn!("write backend {id}: {e}"),
                }
            }
        }

        // ── 2. For each frontend port, compute old vs new state ───────────────
        let old_states: Vec<(u16, u8, FrontendState)> =
            inner.service_state.remove(service_key).unwrap_or_default();

        let mut new_states: Vec<(u16, u8, FrontendState)> = Vec::with_capacity(frontends.len());

        for fe in frontends {
            // Reuse existing service ID for this (port, proto) if we had one.
            let svc_id = old_states
                .iter()
                .find(|(p, pr, _)| *p == fe.port && *pr == fe.proto)
                .map(|(_, _, s)| s.svc_id)
                .unwrap_or_else(|| inner.alloc_svc_id());

            let state = FrontendState {
                svc_id,
                backend_ids: backend_id_list.clone(),
            };

            // Write to map.
            match self
                .writer
                .write_frontend(vip, fe.port, fe.proto, svc_id, &backend_id_list)
            {
                Ok(()) => {
                    info!(
                        service = service_key,
                        vip = %vip,
                        port = fe.port,
                        backends = backend_id_list.len(),
                        svc_id,
                        "frontend reconciled"
                    );

                    // Write reverse NAT entry for return traffic rewriting.
                    if let Err(e) = self.writer.write_revnat(svc_id, vip, fe.port) {
                        warn!(service = service_key, port = fe.port, "write revnat: {e}");
                    }
                }
                Err(e) => warn!(service = service_key, port = fe.port, "write frontend: {e}"),
            }

            // Update ref counts for backends.
            for &id in &backend_id_list {
                inner.inc_backend_ref(id);
            }

            new_states.push((fe.port, fe.proto, state));
        }

        // ── 3. Delete frontends that no longer exist ──────────────────────────
        for (port, proto, old_state) in &old_states {
            let still_exists = new_states.iter().any(|(p, pr, _)| p == port && pr == proto);
            if !still_exists {
                let slot_count = old_state.backend_ids.len();
                if let Err(e) = self.writer.delete_frontend(vip, *port, *proto, slot_count) {
                    warn!(service = service_key, port, "delete frontend: {e}");
                }
                // Decrement ref counts; delete orphaned backends.
                for &id in &old_state.backend_ids {
                    if inner.dec_backend_ref(id) {
                        if let Err(e) = self.writer.delete_backend(id) {
                            warn!(backend_id = id, "delete backend: {e}");
                        }
                        // Remove from identity map.
                        inner.backend_ids.retain(|_, v| *v != id);
                    }
                }
            }
        }

        // Also decrement refs for old backends of surviving frontends that changed.
        for (port, proto, old_state) in &old_states {
            let still_exists = new_states.iter().any(|(p, pr, _)| p == port && pr == proto);
            if still_exists {
                for &old_id in &old_state.backend_ids {
                    if !backend_id_list.contains(&old_id) {
                        if inner.dec_backend_ref(old_id) {
                            if let Err(e) = self.writer.delete_backend(old_id) {
                                warn!(backend_id = old_id, "delete stale backend: {e}");
                            }
                            inner.backend_ids.retain(|_, v| *v != old_id);
                        }
                    }
                }
            }
        }

        inner
            .service_state
            .insert(service_key.to_string(), new_states);
        Ok(())
    }

    /// Removes all eBPF map entries for a service (e.g. when it is deleted from K8s).
    pub async fn delete_service(&self, service_key: &str, vip: Ipv4Addr) {
        let mut inner = self.inner.lock().await;
        let Some(states) = inner.service_state.remove(service_key) else {
            return;
        };

        for (port, proto, state) in states {
            if let Err(e) = self
                .writer
                .delete_frontend(vip, port, proto, state.backend_ids.len())
            {
                warn!(
                    service = service_key,
                    port, "delete frontend on svc delete: {e}"
                );
            }
            // Delete reverse NAT entry.
            if let Err(e) = self.writer.delete_revnat(state.svc_id) {
                warn!(
                    service = service_key,
                    svc_id = state.svc_id,
                    "delete revnat on svc delete: {e}"
                );
            }
            for id in state.backend_ids {
                if inner.dec_backend_ref(id) {
                    if let Err(e) = self.writer.delete_backend(id) {
                        warn!(backend_id = id, "delete backend on svc delete: {e}");
                    }
                    inner.backend_ids.retain(|_, v| *v != id);
                }
            }
        }
    }

    /// Returns true if the eBPF maps are accessible (eBPF programs loaded).
    pub fn maps_available(&self) -> bool {
        self.writer.svc_path().exists()
            && self.writer.backend_path().exists()
            && self.writer.revnat_path().exists()
    }
}

impl Default for LbReconciler {
    fn default() -> Self {
        Self::default_path()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // These tests run without a real eBPF map — they verify the reconciler's
    // in-memory state management (ID allocation, ref counting, state tracking).
    // Map writes silently fail because the paths don't exist in unit test env.

    fn reconciler() -> LbReconciler {
        LbReconciler::new("/nonexistent/bpf")
    }

    fn tcp_backend(ip: [u8; 4], port: u16) -> BackendInfo {
        BackendInfo {
            ip: Ipv4Addr::from(ip),
            port,
            proto: 6,
        }
    }

    fn tcp_frontend(port: u16) -> FrontendPort {
        FrontendPort {
            port,
            proto: 6,
            target_port: port,
        }
    }

    #[tokio::test]
    async fn allocates_stable_backend_ids() {
        let r = reconciler();
        let vip = Ipv4Addr::new(10, 96, 0, 10);
        let b = vec![tcp_backend([10, 0, 0, 1], 53)];
        let fe = vec![tcp_frontend(53)];

        r.reconcile("kube-system/kube-dns", vip, &fe, &b)
            .await
            .unwrap();
        r.reconcile("kube-system/kube-dns", vip, &fe, &b)
            .await
            .unwrap();

        let inner = r.inner.lock().await;
        // Same backend should map to the same ID on second reconcile.
        assert_eq!(inner.backend_ids.len(), 1);
    }

    #[tokio::test]
    async fn tracks_multiple_frontends() {
        let r = reconciler();
        let vip = Ipv4Addr::new(10, 96, 0, 10);
        let b = vec![tcp_backend([10, 0, 0, 1], 53)];
        let fe = vec![tcp_frontend(53), tcp_frontend(9153)];

        r.reconcile("kube-system/kube-dns", vip, &fe, &b)
            .await
            .unwrap();

        let inner = r.inner.lock().await;
        let states = inner.service_state.get("kube-system/kube-dns").unwrap();
        assert_eq!(states.len(), 2);
    }

    #[tokio::test]
    async fn stable_svc_id_across_backend_changes() {
        let r = reconciler();
        let vip = Ipv4Addr::new(10, 96, 0, 10);
        let fe = vec![tcp_frontend(80)];

        let b1 = vec![tcp_backend([10, 0, 0, 1], 8080)];
        r.reconcile("default/web", vip, &fe, &b1).await.unwrap();
        let svc_id_first = {
            let inner = r.inner.lock().await;
            inner.service_state["default/web"][0].2.svc_id
        };

        // Add a backend — svc_id must be stable.
        let b2 = vec![
            tcp_backend([10, 0, 0, 1], 8080),
            tcp_backend([10, 0, 0, 2], 8080),
        ];
        r.reconcile("default/web", vip, &fe, &b2).await.unwrap();
        let svc_id_second = {
            let inner = r.inner.lock().await;
            inner.service_state["default/web"][0].2.svc_id
        };

        assert_eq!(svc_id_first, svc_id_second);
    }

    #[tokio::test]
    async fn delete_service_clears_state() {
        let r = reconciler();
        let vip = Ipv4Addr::new(10, 96, 0, 10);
        let b = vec![tcp_backend([10, 0, 0, 1], 53)];
        let fe = vec![tcp_frontend(53)];

        r.reconcile("kube-system/kube-dns", vip, &fe, &b)
            .await
            .unwrap();
        r.delete_service("kube-system/kube-dns", vip).await;

        let inner = r.inner.lock().await;
        assert!(!inner.service_state.contains_key("kube-system/kube-dns"));
    }

    #[tokio::test]
    async fn backend_ref_count_drops_on_delete() {
        let r = reconciler();
        let vip = Ipv4Addr::new(10, 96, 0, 10);
        let b = vec![tcp_backend([10, 0, 0, 1], 53)];
        let fe = vec![tcp_frontend(53)];

        r.reconcile("kube-system/kube-dns", vip, &fe, &b)
            .await
            .unwrap();
        let backend_id = {
            let inner = r.inner.lock().await;
            inner.backend_ids[&BackendIdentity {
                ip: b[0].ip,
                port: 53,
                proto: 6,
            }]
        };

        r.delete_service("kube-system/kube-dns", vip).await;

        let inner = r.inner.lock().await;
        // Backend should have been fully released.
        assert!(!inner.backend_refs.contains_key(&backend_id));
    }

    #[tokio::test]
    async fn shared_backend_not_deleted_until_last_ref() {
        let r = reconciler();
        let b = vec![tcp_backend([10, 0, 0, 1], 8080)]; // same backend for both services

        let vip1 = Ipv4Addr::new(10, 96, 0, 10);
        let vip2 = Ipv4Addr::new(10, 96, 0, 11);
        let fe = vec![tcp_frontend(80)];

        r.reconcile("ns/svc1", vip1, &fe, &b).await.unwrap();
        r.reconcile("ns/svc2", vip2, &fe, &b).await.unwrap();

        // Deleting svc1 should NOT delete the backend (still used by svc2).
        r.delete_service("ns/svc1", vip1).await;
        let inner = r.inner.lock().await;
        let id = inner.backend_ids.get(&BackendIdentity {
            ip: b[0].ip,
            port: 8080,
            proto: 6,
        });
        assert!(
            id.is_some(),
            "backend should still exist after first delete"
        );
    }
}
