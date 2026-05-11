//! Benchmark: load-balancer backend selection.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_loadbalancer::MaglevHash;
use std::hint::black_box;

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

criterion_group!(lb_benches, bench_round_robin_baseline, bench_maglev);
criterion_main!(lb_benches);
