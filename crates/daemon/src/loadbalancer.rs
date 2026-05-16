//! Track I: daemon-side LB reconciler shim.
//!
//! Wraps [`backend_mapping::LbReconciler`] so that `runtime.rs` can call a
//! single async method whenever Kubernetes services or endpoint slices change.

use std::net::Ipv4Addr;
use backend_mapping::{BackendInfo, FrontendPort, LbReconciler};
use tracing::warn;

pub use backend_mapping::LbReconciler as BackendSyncer;

/// Convenience shim called from `CompatState::upsert_endpoint_slice` and
/// `CompatState::upsert_endpoints`.
///
/// Converts the daemon's internal `CompatService` and `CompatBackend` types
/// into the types expected by [`LbReconciler::reconcile`] and fires the
/// reconciler on a spawned Tokio task so callers don't have to be async.
pub fn reconcile_service(
    reconciler: &LbReconciler,
    service_key: &str,
    cluster_ip: Option<Ipv4Addr>,
    frontends: Vec<(u16, u8)>,    // (port, proto)
    backends: Vec<(Ipv4Addr, u16, u8)>, // (ip, port, proto)
) {
    let Some(vip) = cluster_ip else {
        return; // headless service — nothing to reconcile
    };

    let reconciler = reconciler.clone();
    let service_key = service_key.to_string();

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
                service = service_key,
                svc_map = backend_mapping::SVC_MAP_NAME,
                backend_map = backend_mapping::BACKEND_MAP_NAME,
                "eBPF LB maps not yet available — will retry on next endpoint update"
            );
            return;
        }

        if let Err(e) = reconciler.reconcile(&service_key, vip, &fe, &be).await {
            warn!(service = service_key, "LB reconcile failed: {e}");
        }
    });
}
