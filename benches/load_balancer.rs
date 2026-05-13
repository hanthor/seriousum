//! Benchmark: load-balancer core value operations.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use seriousum_loadbalancer::{
    Backend, BackendID, BackendState, L3n4Addr, L3n4AddrID, L4Type, SVC, ServiceID, ServiceName,
    diff_backends,
};
use std::net::{IpAddr, Ipv4Addr};

fn make_backend(id: u32) -> Backend {
    Backend::with_state(
        BackendID(id + 1),
        L3n4Addr::new(
            IpAddr::V4(Ipv4Addr::new(10, 0, (id / 250) as u8, (id % 250 + 1) as u8)),
            8080,
            L4Type::TCP,
        ),
        BackendState::Active,
    )
}

fn make_backends(n: usize) -> Vec<Backend> {
    (0..n).map(|i| make_backend(i as u32)).collect()
}

fn bench_diff_backends(c: &mut Criterion) {
    let mut group = c.benchmark_group("lb_diff_backends");

    for n in [1usize, 8, 64, 512, 4096] {
        group.throughput(Throughput::Elements(n as u64));
        let desired = make_backends(n);
        let mut actual = desired.clone();
        if let Some(last) = actual.last_mut() {
            last.transition_to(BackendState::Quarantined).unwrap();
        }

        group.bench_with_input(BenchmarkId::new("backends", n), &n, |b, _| {
            b.iter(|| black_box(diff_backends(black_box(&desired), black_box(&actual))))
        });
    }

    group.finish();
}

fn bench_service_name_new(c: &mut Criterion) {
    c.bench_function("lb_service_name_new", |b| {
        b.iter(|| black_box(ServiceName::new("bar", "baz").with_cluster("foo")))
    });
}

fn bench_service_name_display(c: &mut Criterion) {
    let name = ServiceName::new("bar", "baz").with_cluster("foo");
    c.bench_function("lb_service_name_display", |b| {
        b.iter(|| black_box(name.to_string()))
    });
}

fn bench_l3n4addr_display_ipv4(c: &mut Criterion) {
    let addr = L3n4Addr::new(
        IpAddr::V4(Ipv4Addr::new(192, 168, 123, 210)),
        8080,
        L4Type::TCP,
    );
    c.bench_function("lb_l3n4addr_display_ipv4", |b| {
        b.iter(|| black_box(addr.to_string()))
    });
}

fn make_service_with_backends(n: usize) -> SVC {
    let mut service = SVC::new(L3n4AddrID::new(
        L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 1)), 80, L4Type::TCP),
        ServiceID(100),
    ));
    service.name = Some(ServiceName::new("default", "benchmark"));
    service.backends = make_backends(n);
    service
}

fn bench_active_backends(c: &mut Criterion) {
    let service = make_service_with_backends(1000);
    c.bench_function("lb_active_backends_1000", |b| {
        b.iter(|| black_box(service.active_backends()))
    });
}

fn bench_update_backend_state(c: &mut Criterion) {
    let service = make_service_with_backends(1000);
    c.bench_function("lb_update_backend_state", |b| {
        b.iter(|| {
            let mut svc = service.clone();
            svc.update_backend_state(BackendID(1000), BackendState::Maintenance)
                .unwrap();
            black_box(svc)
        })
    });
}

criterion_group!(
    lb_benches,
    bench_diff_backends,
    bench_service_name_new,
    bench_service_name_display,
    bench_l3n4addr_display_ipv4,
    bench_active_backends,
    bench_update_backend_state,
);
criterion_main!(lb_benches);
