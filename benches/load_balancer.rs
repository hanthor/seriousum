//! Benchmark: load-balancer backend selection
//!
//! Measures the four built-in algorithms (round-robin, least-connections,
//! consistent-hash, random) at varying backend-pool sizes.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_loadbalancer::{Algorithm, Backend, LoadBalancerPool};
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};

fn make_backends(n: usize) -> Vec<Backend> {
    (0..n)
        .map(|i| {
            Backend::new(
                IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8)),
                8080 + i as u16,
            )
        })
        .collect()
}

fn bench_algorithm(c: &mut Criterion, algo: Algorithm, label: &str) {
    let mut g = c.benchmark_group(label);

    for n in [1usize, 8, 64, 512, 4096] {
        g.throughput(Throughput::Elements(1));
        let backends = make_backends(n);
        let pool = LoadBalancerPool::with_algorithm(algo, backends);
        let client_ip: IpAddr = "192.168.1.100".parse().unwrap();

        g.bench_with_input(BenchmarkId::new("backends", n), &n, |b, _| {
            b.iter(|| black_box(pool.select(black_box(client_ip), black_box(0xdeadbeef_u32))))
        });
    }
    g.finish();
}

fn bench_round_robin(c: &mut Criterion) {
    bench_algorithm(c, Algorithm::RoundRobin, "lb_round_robin");
}

fn bench_least_connections(c: &mut Criterion) {
    bench_algorithm(c, Algorithm::LeastConnections, "lb_least_connections");
}

fn bench_consistent_hash(c: &mut Criterion) {
    bench_algorithm(c, Algorithm::ConsistentHash, "lb_consistent_hash");
}

fn bench_random(c: &mut Criterion) {
    bench_algorithm(c, Algorithm::Random, "lb_random");
}

fn bench_session_affinity(c: &mut Criterion) {
    let backends = make_backends(16);
    let pool = LoadBalancerPool::with_algorithm(Algorithm::RoundRobin, backends);

    // Warm the affinity cache with 1 000 distinct client IPs
    let clients: Vec<IpAddr> = (0..1000u32)
        .map(|i| IpAddr::V4(Ipv4Addr::from(0xc0a80000 + i)))
        .collect();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        for &ip in &clients {
            pool.select_with_affinity(ip, 0).await;
        }
    });

    c.bench_function("lb_session_affinity_cache_hit", |b| {
        let ip = clients[42];
        b.iter(|| rt.block_on(async { black_box(pool.select_with_affinity(black_box(ip), 0).await) }))
    });

    c.bench_function("lb_session_affinity_cache_miss", |b| {
        let ip: IpAddr = "172.16.99.99".parse().unwrap();
        b.iter(|| rt.block_on(async { black_box(pool.select_with_affinity(black_box(ip), 0).await) }))
    });
}

criterion_group!(
    lb_benches,
    bench_round_robin,
    bench_least_connections,
    bench_consistent_hash,
    bench_random,
    bench_session_affinity,
);
criterion_main!(lb_benches);
