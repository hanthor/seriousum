//! Benchmark: IP allocation and release.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_endpoints::{IPAMConfig, IPAMManager};
use std::hint::black_box;
use std::net::Ipv4Addr;

fn make_manager(pool_size: usize) -> IPAMManager {
    let start = Ipv4Addr::new(10, 0, 0, 2);
    let end = Ipv4Addr::from(u32::from(start) + pool_size as u32);
    IPAMManager::new(IPAMConfig {
        start_ip: start,
        end_ip: end,
    })
}

fn bench_ipam_allocate(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipam_allocate");

    for pool_size in [64usize, 256, 1024, 4096] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("pool_size", pool_size), &pool_size, |b, &pool_size| {
            b.iter(|| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let ipam = make_manager(pool_size);
                    black_box(ipam.allocate_ip().await.unwrap())
                })
            })
        });
    }

    group.finish();
}

fn bench_ipam_alloc_release_cycle(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("ipam_alloc_release_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                let ipam = make_manager(4096);
                let mut ips = Vec::with_capacity(1000);
                for _ in 0..1000 {
                    ips.push(ipam.allocate_ip().await.unwrap());
                }
                for ip in ips {
                    ipam.release_ip(black_box(ip)).await.unwrap();
                }
            })
        })
    });
}

fn bench_ipam_allocate_warm_pool(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let ipam = make_manager(8192);

    c.bench_function("ipam_allocate_warm_pool", |b| {
        b.iter(|| {
            rt.block_on(async {
                let ip = ipam.allocate_ip().await.unwrap();
                ipam.release_ip(black_box(ip)).await.unwrap();
            })
        })
    });
}

fn bench_ipam_concurrent(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();

    c.bench_function("ipam_concurrent_8_tasks", |b| {
        b.iter(|| {
            rt.block_on(async {
                let ipam = std::sync::Arc::new(make_manager(8192));
                let tasks: Vec<_> = (0..8)
                    .map(|_| {
                        let ipam = ipam.clone();
                        tokio::spawn(async move {
                            let ip = ipam.allocate_ip().await.unwrap();
                            ipam.release_ip(ip).await.unwrap();
                        })
                    })
                    .collect();
                for task in tasks {
                    task.await.unwrap();
                }
            })
        })
    });
}

criterion_group!(
    ipam_benches,
    bench_ipam_allocate,
    bench_ipam_alloc_release_cycle,
    bench_ipam_allocate_warm_pool,
    bench_ipam_concurrent,
);
criterion_main!(ipam_benches);
