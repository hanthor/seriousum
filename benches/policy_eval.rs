//! Benchmark: policy selector matching and policy distillation hot path.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_policy::{EndpointIdentity, EndpointSelector, L4Policy, L4Traffic, PolicyRepository, PolicyRule, TrafficDirection};
use seriousum_policy::l4::Protocol;
use std::collections::HashMap;
use std::hint::black_box;

fn labels(app: &str, tier: &str, env: &str) -> HashMap<String, String> {
    HashMap::from([
        ("app".to_string(), app.to_string()),
        ("tier".to_string(), tier.to_string()),
        ("env".to_string(), env.to_string()),
    ])
}

fn make_rule(i: usize) -> PolicyRule {
    let mut l4 = L4Policy::new();
    l4.add_allowed(L4Traffic::new(Protocol::TCP, 8080 + i as u16));

    PolicyRule::new(TrafficDirection::Ingress)
        .with_subject_selector(EndpointSelector::empty().with_label("app", "server"))
        .with_peer_selector(EndpointSelector::empty().with_label("app", format!("client-{i}")))
        .with_l4_policy(l4)
}

fn bench_selector_match(c: &mut Criterion) {
    let selector = EndpointSelector::empty()
        .with_label("app", "frontend")
        .with_label("tier", "web")
        .with_label("env", "prod");
    let matching = labels("frontend", "web", "prod");
    let non_matching = labels("backend", "worker", "dev");

    let mut group = c.benchmark_group("selector_match");
    group.bench_function("match_hit", |b| {
        b.iter(|| black_box(selector.matches(black_box(&matching))))
    });
    group.bench_function("match_miss", |b| {
        b.iter(|| black_box(selector.matches(black_box(&non_matching))))
    });
    group.finish();
}

fn bench_policy_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("policy_eval");
    let endpoint_labels = labels("server", "api", "prod");

    for policy_count in [1usize, 10, 100, 1000] {
        group.throughput(Throughput::Elements(policy_count as u64));
        let repo = PolicyRepository::new();
        for i in 0..policy_count {
            repo.add_ingress_rule(format!("rule-{i}"), make_rule(i)).unwrap();
        }

        group.bench_with_input(BenchmarkId::new("policies", policy_count), &policy_count, |b, _| {
            b.iter(|| {
                black_box(
                    repo.distill_policy(
                        black_box(EndpointIdentity::new(1234)),
                        black_box(&endpoint_labels),
                    )
                    .unwrap(),
                )
            })
        });
    }

    group.finish();
}

criterion_group!(policy_benches, bench_selector_match, bench_policy_eval);
criterion_main!(policy_benches);
