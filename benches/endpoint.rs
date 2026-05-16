//! Benchmark: endpoint lifecycle — creation, identity assignment, regeneration.
//!
//! Every pod joining a Kubernetes cluster triggers this path: Cilium creates an
//! endpoint, waits for an identity from kvstore, regenerates eBPF programs, and
//! marks the endpoint ready. This measures the control-plane bookkeeping
//! (state machine transitions and manager index updates) before eBPF compilation.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_endpoint::{Endpoint, EndpointID, EndpointManager, EndpointState};
use std::hint::black_box;

fn make_endpoint(id: u16) -> Endpoint {
    let mut ep = Endpoint::new(EndpointID(id));
    ep.container_id = format!("container-{id}");
    ep.pod_name = format!("pod-{id}");
    ep.pod_namespace = "default".into();
    ep
}

fn bring_up(ep: &mut Endpoint) {
    ep.set_state(EndpointState::WaitingForIdentity, "init").unwrap();
    ep.set_identity(1000 + ep.id.0 as u32);
    ep.set_state(EndpointState::WaitingToRegenerate, "got identity").unwrap();
    ep.set_state(EndpointState::Regenerating, "start regen").unwrap();
    ep.set_state(EndpointState::Ready, "done").unwrap();
}

// --- Full endpoint bring-up cycle ---
//
// creating → waiting_for_identity → waiting_to_regenerate → regenerating → ready
// The critical path that determines how fast Cilium can admit a new pod.

fn bench_endpoint_full_lifecycle(c: &mut Criterion) {
    c.bench_function("endpoint_full_lifecycle", |b| {
        b.iter(|| {
            let mut ep = make_endpoint(1);
            bring_up(&mut ep);
            black_box(ep.is_ready())
        })
    });
}

// --- Policy-triggered regeneration ---
//
// An already-ready endpoint receiving a policy update goes through
// ready → waiting_to_regenerate → regenerating → ready.

fn bench_endpoint_regen(c: &mut Criterion) {
    c.bench_function("endpoint_regen_cycle", |b| {
        let mut ep = make_endpoint(1);
        bring_up(&mut ep);

        b.iter(|| {
            ep.set_state(EndpointState::WaitingToRegenerate, "policy update").unwrap();
            ep.set_state(EndpointState::Regenerating, "start").unwrap();
            ep.set_state(EndpointState::Ready, "done").unwrap();
            black_box(ep.policy_revision += 1)
        })
    });
}

// --- Manager: pod burst (add + index) ---
//
// Simulates a node burst: N pods starting, each registered into the manager's
// container-ID and kubernetes-key indices. Comparable to what Cilium does on
// node restart when restoring all endpoints from disk.

fn bench_manager_add_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("endpoint_manager_add");

    for count in [1usize, 10, 100, 1000] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("pods", count), &count, |b, &n| {
            b.iter(|| {
                let mgr = EndpointManager::new();
                for i in 0..n {
                    mgr.add_endpoint(make_endpoint(i as u16)).unwrap();
                }
                black_box(mgr.count())
            })
        });
    }

    group.finish();
}

// --- Manager: ready endpoint listing ---
//
// `cilium endpoint list` and Hubble enumerate endpoints by state.
// Measures the cost of scanning and filtering a pre-populated manager.

fn bench_manager_list_ready(c: &mut Criterion) {
    let mut group = c.benchmark_group("endpoint_manager_list_ready");

    for count in [10usize, 100, 500] {
        let mgr = EndpointManager::new();
        for i in 0..count {
            let mut ep = make_endpoint(i as u16);
            bring_up(&mut ep);
            mgr.add_endpoint(ep).unwrap();
        }

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("endpoints", count), &count, |b, _| {
            b.iter(|| black_box(mgr.ready_endpoints()))
        });
    }

    group.finish();
}

// --- Identity assignment ---
//
// When the kvstore assigns a security identity to an endpoint, `set_identity`
// is called. Measures the simple path (first assignment) vs idempotent path
// (same identity assigned again, e.g., after policy refresh).

fn bench_identity_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("endpoint_set_identity");

    let mut ep = make_endpoint(1);
    group.bench_function("new_identity", |b| {
        let mut id = 1000u32;
        b.iter(|| {
            id += 1;
            black_box(ep.set_identity(id))
        })
    });

    group.bench_function("same_identity", |b| {
        ep.set_identity(42000);
        b.iter(|| black_box(ep.set_identity(42000)))
    });

    group.finish();
}

criterion_group!(
    endpoint_benches,
    bench_endpoint_full_lifecycle,
    bench_endpoint_regen,
    bench_manager_add_batch,
    bench_manager_list_ready,
    bench_identity_set,
);
criterion_main!(endpoint_benches);
