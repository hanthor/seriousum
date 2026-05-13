//! Benchmark: FQDN cache operations.

use criterion::{Criterion, criterion_group, criterion_main};
use seriousum_fqdn::{DnsCache, FqdnSelector};
use std::collections::HashMap;
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

fn bench_fqdn_selector_string(c: &mut Criterion) {
    let selectors = vec![
        FqdnSelector::new("cilium.io"),
        FqdnSelector::new("[a-z]*.cilium.io"),
        FqdnSelector::new("a{1,2}.cilium.io"),
        FqdnSelector::new("*.cilium.io"),
    ];

    c.bench_function("fqdn_selector_string", |b| {
        b.iter(|| {
            for selector in &selectors {
                black_box(selector.to_string());
            }
        })
    });
}

fn build_json_fixture(entries: usize) -> HashMap<String, Vec<IpAddr>> {
    let cache = DnsCache::new(0);
    for i in 0..entries {
        let name = format!("domain-{i}.example.com");
        let ips = vec![
            IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8)),
            IpAddr::V4(Ipv4Addr::new(10, 1, (i / 256) as u8, (i % 256) as u8)),
        ];
        cache.update(name, &ips, 86_400).unwrap();
    }
    cache.snapshot()
}

fn bench_fqdn_json_marshal_100(c: &mut Criterion) {
    let snapshot = build_json_fixture(100);
    c.bench_function("fqdn_json_marshal_100", |b| {
        b.iter(|| black_box(serde_json::to_vec(black_box(&snapshot)).unwrap()))
    });
}

fn bench_fqdn_json_unmarshal_100(c: &mut Criterion) {
    let snapshot = build_json_fixture(100);
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    c.bench_function("fqdn_json_unmarshal_100", |b| {
        b.iter(|| {
            let decoded: HashMap<String, Vec<IpAddr>> =
                serde_json::from_slice(black_box(&bytes)).unwrap();
            black_box(decoded)
        })
    });
}

fn bench_fqdn_json_marshal_1000(c: &mut Criterion) {
    let snapshot = build_json_fixture(1000);
    c.bench_function("fqdn_json_marshal_1000", |b| {
        b.iter(|| black_box(serde_json::to_vec(black_box(&snapshot)).unwrap()))
    });
}

fn bench_fqdn_json_unmarshal_1000(c: &mut Criterion) {
    let snapshot = build_json_fixture(1000);
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    c.bench_function("fqdn_json_unmarshal_1000", |b| {
        b.iter(|| {
            let decoded: HashMap<String, Vec<IpAddr>> =
                serde_json::from_slice(black_box(&bytes)).unwrap();
            black_box(decoded)
        })
    });
}

criterion_group!(
    fqdn_benches,
    bench_fqdn_lookup,
    bench_fqdn_update,
    bench_fqdn_selector_string,
    bench_fqdn_json_marshal_100,
    bench_fqdn_json_unmarshal_100,
    bench_fqdn_json_marshal_1000,
    bench_fqdn_json_unmarshal_1000,
);
criterion_main!(fqdn_benches);
