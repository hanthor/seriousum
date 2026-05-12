//! Benchmark: FQDN cache operations.

use criterion::{Criterion, criterion_group, criterion_main};
use seriousum_fqdn::cache::DnsCache;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};

fn sample_ips() -> Vec<IpAddr> {
    vec![
        IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)),
        IpAddr::V4(Ipv4Addr::new(1, 2, 3, 5)),
        IpAddr::V4(Ipv4Addr::new(1, 2, 3, 6)),
    ]
}

fn bench_fqdn_lookup(c: &mut Criterion) {
    let cache = DnsCache::new(0);
    let ips = sample_ips();
    cache.update("test.com", &ips, 60).unwrap();

    c.bench_function("fqdn_lookup", |b| {
        b.iter(|| black_box(cache.lookup(black_box("test.com"))))
    });
}

fn bench_fqdn_update(c: &mut Criterion) {
    let cache = DnsCache::new(0);
    let ips = sample_ips();

    c.bench_function("fqdn_update", |b| {
        b.iter(|| black_box(cache.update("test.com", black_box(&ips), 60).unwrap()))
    });
}

criterion_group!(fqdn_benches, bench_fqdn_lookup, bench_fqdn_update);
criterion_main!(fqdn_benches);
