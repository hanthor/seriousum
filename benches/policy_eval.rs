//! Benchmark: policy evaluation hot path
//!
//! Measures how fast seriousum can evaluate whether a connection is allowed
//! under a set of NetworkPolicy rules.  The equivalent Go path in upstream
//! Cilium lives in pkg/policy and is used on every packet that hits the
//! userspace policy engine.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_policy::{NetworkPolicy, PolicyCache, PolicyEvaluator, Selector};
use std::collections::HashMap;
use std::hint::black_box;

// ── helpers ──────────────────────────────────────────────────────────────────

fn make_selector(pairs: &[(&str, &str)]) -> Selector {
    Selector::new(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

fn make_policy(name: &str, n_ingress_rules: usize) -> NetworkPolicy {
    let mut ingress = Vec::new();
    for i in 0..n_ingress_rules {
        ingress.push(seriousum_policy::IngressRule {
            from: vec![seriousum_policy::PolicyPeer {
                pod_selector: Some(make_selector(&[("app", &format!("client-{i}"))])),
                namespace_selector: None,
                ip_block: None,
            }],
            ports: vec![seriousum_policy::PolicyPort {
                protocol: "TCP".to_string(),
                port: Some(8080 + i as u16),
            }],
        });
    }
    NetworkPolicy {
        name: name.to_string(),
        namespace: "default".to_string(),
        pod_selector: make_selector(&[("app", "server")]),
        ingress_rules: ingress,
        egress_rules: vec![],
        policy_types: vec![seriousum_policy::PolicyType::Ingress],
    }
}

fn pod_labels(app: &str) -> HashMap<String, String> {
    [("app".to_string(), app.to_string())].into()
}

// ── benchmarks ───────────────────────────────────────────────────────────────

fn bench_selector_match(c: &mut Criterion) {
    let selector = make_selector(&[("app", "frontend"), ("tier", "web"), ("env", "prod")]);
    let matching = pod_labels("frontend");
    let non_matching = pod_labels("backend");

    let mut g = c.benchmark_group("selector_match");
    g.bench_function("match_hit", |b| {
        b.iter(|| black_box(selector.matches(black_box(&matching))))
    });
    g.bench_function("match_miss", |b| {
        b.iter(|| black_box(selector.matches(black_box(&non_matching))))
    });
    g.finish();
}

fn bench_policy_eval(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut g = c.benchmark_group("policy_eval");

    for policy_count in [1usize, 10, 100, 1000] {
        g.throughput(Throughput::Elements(policy_count as u64));

        // Build a cache with `policy_count` policies
        let cache = rt.block_on(async {
            let cache = PolicyCache::new();
            for i in 0..policy_count {
                let p = make_policy(&format!("pol-{i}"), 3);
                cache.add_policy(p).await;
            }
            cache
        });

        let evaluator = PolicyEvaluator::new(cache);
        let src = pod_labels("client-0");
        let dst = pod_labels("server");

        g.bench_with_input(
            BenchmarkId::new("policies", policy_count),
            &policy_count,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        black_box(
                            evaluator
                                .is_allowed(
                                    black_box("default"),
                                    black_box(&src),
                                    black_box(&dst),
                                    black_box(8080),
                                    black_box("TCP"),
                                )
                                .await,
                        )
                    })
                })
            },
        );
    }
    g.finish();
}

fn bench_policy_cache_lookup(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Pre-populate with 1 000 policies
    let cache = rt.block_on(async {
        let cache = PolicyCache::new();
        for i in 0..1000usize {
            cache.add_policy(make_policy(&format!("pol-{i}"), 1)).await;
        }
        cache
    });

    c.bench_function("policy_cache_lookup_1000", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box(cache.get_policies_for_namespace(black_box("default")).await)
            })
        })
    });
}

criterion_group!(
    policy_benches,
    bench_selector_match,
    bench_policy_eval,
    bench_policy_cache_lookup,
);
criterion_main!(policy_benches);
