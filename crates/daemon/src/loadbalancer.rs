//! Track I: daemon-side LB reconciler shim.
//!
//! Wraps [`backend_mapping::LbReconciler`] so that `runtime.rs` can call a
//! single async method whenever Kubernetes services or endpoint slices change.

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::Mutex;
use backend_mapping::{BackendInfo, FrontendPort, LbReconciler};
use tracing::{info, warn};

pub use backend_mapping::LbReconciler as BackendSyncer;

/// Pending service reconciliation waiting for eBPF maps to become available.
#[derive(Debug)]
pub struct PendingReconcile {
    pub service_key: String,
    pub cluster_ip: Option<Ipv4Addr>,
    pub frontends: Vec<(u16, u8)>,  // (port, proto)
    pub backends: Vec<(Ipv4Addr, u16, u8)>,  // (ip, port, proto)
}

/// Convenience shim called from `CompatState::upsert_endpoint_slice` and
/// `CompatState::upsert_endpoints`.
///
/// Converts the daemon's internal `CompatService` and `CompatBackend` types
/// into the types expected by [`LbReconciler::reconcile`] and fires the
/// reconciler on a spawned Tokio task so callers don't have to be async.
///
/// If eBPF maps are not yet available, queues the reconciliation for retry
/// once maps become available (via `drain_pending_reconciles`).
pub fn reconcile_service(
    reconciler: &LbReconciler,
    service_key: &str,
    cluster_ip: Option<Ipv4Addr>,
    frontends: Vec<(u16, u8)>,    // (port, proto)
    backends: Vec<(Ipv4Addr, u16, u8)>, // (ip, port, proto)
    pending_queue: Arc<Mutex<Vec<PendingReconcile>>>,
) {
    let Some(vip) = cluster_ip else {
        return; // headless service — nothing to reconcile
    };

    let reconciler = reconciler.clone();
    let service_key_str = service_key.to_string();
    let frontends_copy = frontends.clone();
    let backends_copy = backends.clone();

    let fe: Vec<FrontendPort> = frontends
        .into_iter()
        .map(|(port, proto)| FrontendPort { port, proto, target_port: port })
        .collect();

    let be: Vec<BackendInfo> = backends
        .into_iter()
        .map(|(ip, port, proto)| BackendInfo { ip, port, proto })
        .collect();

    tokio::spawn(async move {
        if !reconciler.maps_available() {
            warn!(
                service = service_key_str,
                svc_map = backend_mapping::SVC_MAP_NAME,
                backend_map = backend_mapping::BACKEND_MAP_NAME,
                revnat_map = "cilium_lb4_reverse_nat",
                "eBPF LB maps not yet available — queuing for retry after datapath init"
            );
            queue_pending_reconcile(
                pending_queue,
                service_key_str,
                Some(vip),
                frontends_copy,
                backends_copy,
            );
            return;
        }

        if let Err(e) = reconciler.reconcile(&service_key_str, vip, &fe, &be).await {
            warn!(service = service_key_str, "LB reconcile failed: {e}");
        }
    });
}

/// Queue a pending reconciliation for later (when maps become available).
/// Called by reconcile_service when maps are not yet available.
pub fn queue_pending_reconcile(
    pending_queue: Arc<Mutex<Vec<PendingReconcile>>>,
    service_key: String,
    cluster_ip: Option<Ipv4Addr>,
    frontends: Vec<(u16, u8)>,
    backends: Vec<(Ipv4Addr, u16, u8)>,
) {
    if let Ok(mut queue) = pending_queue.lock() {
        queue.push(PendingReconcile {
            service_key,
            cluster_ip,
            frontends,
            backends,
        });
        info!("queued pending reconcile");
    }
}
