//! Benchmark: identity allocation and IPCache lookups.
//!
//! Every pod that joins the cluster needs an identity (Cilium's security label → numeric
//! ID mapping), and every connection decision requires an IPCache lookup to map a
//! source/destination IP to an identity. These two operations run on the critical path
//! for both endpoint bring-up and per-packet policy enforcement.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ipnet::IpNet;
use seriousum_identity::{
    allocation::{LabelSet, LocalIdentityCache},
    ipcache::IPCache,
};
use seriousum_core::NumericIdentity;
use std::collections::BTreeMap;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

fn make_labels(app: &str, env: &str, tier: &str) -> LabelSet {
    BTreeMap::from([
        ("app".into(), app.into()),
        ("env".into(), env.into()),
        ("tier".into(), tier.into()),
    ])
}

// --- Identity allocation ---
//
// Two cases matter:
//   1. New allocation — the label set has not been seen before. Pays the cost of
//      a write-lock, hash lookup, and ID allocation.
//   2. Cache hit — same labels seen again. Should be a cheap read-lock + map get.

fn bench_identity_allocate_new(c: &mut Criterion) {
    c.bench_function("identity_allocate_new", |b| {
        let cache = LocalIdentityCache::new(10000, 65535);
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let labels = make_labels(&format!("svc-{counter}"), "prod", "backend");
            let (id, _) = cache.allocate(labels);
            // Release immediately so the pool doesn't exhaust.
            cache.release(id);
            black_box(id)
        })
    });
}

fn bench_identity_allocate_cached(c: &mut Criterion) {
    c.bench_function("identity_allocate_cached", |b| {
        let cache = LocalIdentityCache::new(10000, 65535);
        let labels = make_labels("frontend", "prod", "web");
        let _ = cache.allocate(labels.clone());

        b.iter(|| black_box(cache.allocate(black_box(labels.clone()))))
    });
}

fn bench_identity_release(c: &mut Criterion) {
    c.bench_function("identity_release", |b| {
        let cache = LocalIdentityCache::new(10000, 65535);
        b.iter(|| {
            let labels = make_labels("ephemeral", "prod", "web");
            let (id, _) = cache.allocate(labels);
            black_box(cache.release(id))
        })
    });
}

// --- IPCache: upsert ---
//
// The IPCache is updated whenever an endpoint changes its IP or a CIDR policy
// is installed. Measures the cost of updating the prefix → identity mapping.

fn bench_ipcache_upsert(c: &mut Criterion) {
    c.bench_function("ipcache_upsert", |b| {
        let cache = IPCache::new();
        let mut i = 0u32;
        b.iter(|| {
            i += 1;
            let ip = Ipv4Addr::from(i.wrapping_add(0x0a000000));
            let prefix = IpNet::new(IpAddr::V4(ip), 32).unwrap();
            cache.upsert(black_box(prefix), black_box(NumericIdentity::from(i % 65535 + 1)));
        })
    });
}

// --- IPCache: exact prefix lookup ---
//
// `cilium bpf ipcache get` style lookup — exact /32 match.

fn bench_ipcache_lookup_exact(c: &mut Criterion) {
    let cache = IPCache::new();
    let prefixes: Vec<IpNet> = (1u32..=1000)
        .map(|i| {
            let ip = Ipv4Addr::from(i.wrapping_add(0x0a000000));
            IpNet::new(IpAddr::V4(ip), 32).unwrap()
        })
        .collect();
    for (i, prefix) in prefixes.iter().enumerate() {
        cache.upsert(*prefix, NumericIdentity::from(i as u32 + 1));
    }

    c.bench_function("ipcache_lookup_exact_1000", |b| {
        b.iter(|| {
            for prefix in &prefixes {
                black_box(cache.lookup_by_prefix(black_box(prefix)));
            }
        })
    });
}

// --- IPCache: longest-prefix-match lookup ---
//
// The actual datapath-policy path: given a source IP, find the best-matching
// CIDR prefix and return the associated identity. This runs for every new
// connection that hits a CIDR-based NetworkPolicy.

fn bench_ipcache_lpm(c: &mut Criterion) {
    let mut group = c.benchmark_group("ipcache_lpm_lookup");

    for prefix_count in [10usize, 100, 1000] {
        let cache = IPCache::new();
        // Add a mix of /24 and /32 prefixes.
        for i in 0..prefix_count {
            let net24 = IpNet::from_str(&format!("10.{}.0.0/24", i % 256)).unwrap();
            cache.upsert(net24, NumericIdentity::from(i as u32 + 1000));
        }
        for i in 0..prefix_count {
            let ip = Ipv4Addr::new(10, (i % 256) as u8, 0, (i % 254 + 1) as u8);
            let net32 = IpNet::new(IpAddr::V4(ip), 32).unwrap();
            cache.upsert(net32, NumericIdentity::from(i as u32 + 2000));
        }

        let target = IpAddr::V4(Ipv4Addr::new(10, 42, 0, 1));

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("prefixes", prefix_count),
            &prefix_count,
            |b, _| b.iter(|| black_box(cache.lookup_by_ip(black_box(target)))),
        );
    }

    group.finish();
}

criterion_group!(
    identity_benches,
    bench_identity_allocate_new,
    bench_identity_allocate_cached,
    bench_identity_release,
    bench_ipcache_upsert,
    bench_ipcache_lookup_exact,
    bench_ipcache_lpm,
);
criterion_main!(identity_benches);
