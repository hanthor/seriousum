//! Benchmark: IPAM (IP allocation and release)
//!
//! Measures how quickly the endpoint IP allocator can hand out and reclaim
//! addresses from a subnet pool.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_endpoints::{IpamPool, IpamMetrics};
use std::hint::black_box;
use std::net::Ipv4Addr;

fn bench_ipam_alloc(c: &mut Criterion) {
    let mut g = c.benchmark_group("ipam_allocate");

    for pool_size in [64usize, 256, 1024, 4096] {
        g.throughput(Throughput::Elements(1));

        // 10.0.0.0/16 gives 65 534 usable IPs — more than enough
        let base = u32::from(Ipv4Addr::new(10, 0, 0, 0));

        g.bench_with_input(
            BenchmarkId::new("pool_size", pool_size),
            &pool_size,
            |b, &sz| {
                b.iter_custom(|iters| {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let start = std::time::Instant::now();
                    rt.block_on(async {
                        for _ in 0..iters {
                            let pool = IpamPool::new(base, sz as u32);
                            let _ip = black_box(pool.allocate().await);
                        }
                    });
                    start.elapsed()
                })
            },
        );
    }
    g.finish();
}

fn bench_ipam_alloc_release_cycle(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let base = u32::from(Ipv4Addr::new(10, 0, 0, 0));
    let pool = IpamPool::new(base, 4096);

    c.bench_function("ipam_alloc_release_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut ips = Vec::with_capacity(1000);
                for _ in 0..1000 {
                    if let Some(ip) = pool.allocate().await {
                        ips.push(ip);
                    }
                }
                for ip in ips {
                    pool.release(black_box(ip)).await;
                }
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

    let base = u32::from(Ipv4Addr::new(10, 0, 0, 0));

    c.bench_function("ipam_concurrent_8_tasks", |b| {
        b.iter(|| {
            rt.block_on(async {
                let pool = std::sync::Arc::new(IpamPool::new(base, 8192));
                let handles: Vec<_> = (0..8)
                    .map(|_| {
                        let p = pool.clone();
                        tokio::spawn(async move {
                            let ip = p.allocate().await;
                            if let Some(ip) = ip {
                                p.release(ip).await;
                            }
                        })
                    })
                    .collect();
                for h in handles {
                    let _ = h.await;
                }
            })
        })
    });
}

criterion_group!(ipam_benches, bench_ipam_alloc, bench_ipam_alloc_release_cycle, bench_ipam_concurrent);
criterion_main!(ipam_benches);
