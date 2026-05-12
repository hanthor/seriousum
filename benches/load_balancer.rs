//! Benchmark: load-balancer backend selection.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_loadbalancer::{Backend, Frontend, L3n4Addr, L4Protocol, LoadBalancer, MaglevHash, Service, ServiceName, SvcType};
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};

fn make_backends(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("10.0.{}.{}:8080", i / 256, i % 256))
        .collect()
}

fn bench_maglev(c: &mut Criterion) {
    let mut group = c.benchmark_group("lb_consistent_hash");

    for n in [1usize, 8, 64, 512, 4096] {
        group.throughput(Throughput::Elements(1));
        let maglev = MaglevHash::new(make_backends(n)).unwrap();
        let key = format!("192.168.1.100:{n}");

        group.bench_with_input(BenchmarkId::new("backends", n), &n, |b, _| {
            b.iter(|| black_box(maglev.select(black_box(key.as_bytes())).unwrap()))
        });
    }

    group.finish();
}

fn bench_round_robin_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("lb_round_robin");

    for n in [1usize, 8, 64, 512, 4096] {
        group.throughput(Throughput::Elements(1));
        let backends = make_backends(n);
        let index = std::sync::atomic::AtomicUsize::new(0);

        group.bench_with_input(BenchmarkId::new("backends", n), &n, |b, _| {
            b.iter(|| {
                let i = index.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % backends.len();
                black_box(&backends[i])
            })
        });
    }

    group.finish();
}

fn bench_maglev_build(c: &mut Criterion) {
    let backends = make_backends(1000);
    c.bench_function("lb_maglev_build_1000", |b| {
        b.iter(|| black_box(MaglevHash::new(black_box(backends.clone())).unwrap()))
    });
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
    let addr = L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 123, 210)), 8080, L4Protocol::TCP);
    c.bench_function("lb_l3n4addr_display_ipv4", |b| {
        b.iter(|| black_box(addr.to_string()))
    });
}

fn bench_upsert_service_100(c: &mut Criterion) {
    c.bench_function("lb_upsert_service_100", |b| {
        b.iter(|| {
            let lb = LoadBalancer::new();
            for i in 0..100 {
                let name = ServiceName::new("test", format!("svc-{i}"));
                let frontend = Frontend::new(
                    L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, (i % 250 + 1) as u8)), 8080, L4Protocol::TCP),
                    SvcType::ClusterIp,
                    name.clone(),
                );
                let service = Service::new(name).with_frontends(vec![frontend]);
                black_box(lb.upsert_service(service).unwrap());
            }
        })
    });
}

fn bench_update_backends_100(c: &mut Criterion) {
    let lb = LoadBalancer::new();
    let service_name = ServiceName::new("test", "svc");
    let frontend = Frontend::new(
        L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 8080, L4Protocol::TCP),
        SvcType::ClusterIp,
        service_name.clone(),
    );
    lb.upsert_service(Service::new(service_name.clone()).with_frontends(vec![frontend]))
        .unwrap();

    let backends: Vec<_> = (0..100)
        .map(|i| {
            Backend::new(
                service_name.clone(),
                L3n4Addr::new(IpAddr::V4(Ipv4Addr::new(10, 1, 0, (i % 250 + 1) as u8)), 8080, L4Protocol::TCP),
            )
        })
        .collect();

    c.bench_function("lb_update_backends_100", |b| {
        b.iter(|| black_box(lb.update_backends(&service_name, black_box(backends.clone())).unwrap()))
    });
}

criterion_group!(
    lb_benches,
    bench_round_robin_baseline,
    bench_maglev,
    bench_maglev_build,
    bench_service_name_new,
    bench_service_name_display,
    bench_l3n4addr_display_ipv4,
    bench_upsert_service_100,
    bench_update_backends_100,
);
criterion_main!(lb_benches);
