//! Benchmark: Hubble flow observation — ingestion, querying, filtering, and aggregation.
//!
//! These are the operations Hubble runs continuously in production: every packet
//! observed by the eBPF datapath produces a flow event pushed into a ring buffer,
//! filtered by namespace/verdict, and periodically aggregated for the UI and relay.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use seriousum_core::{Protocol, SecurityIdentity};
use seriousum_hubble::{
    AndFilter, DirectionFilter, Endpoint, Flow, FlowDirection, FlowEndpoint, FlowFilter,
    FlowMetadata, FlowObservation, FlowRing, FlowSummary, FlowVerdict, TrafficDirection, Verdict,
    VerdictFilter,
};
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr};

fn make_flow_endpoint(ip: [u8; 4], port: u16) -> FlowEndpoint {
    FlowEndpoint::new()
        .with_ip(IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])))
        .with_port(port)
        .with_identity(SecurityIdentity::world())
}

fn make_flow(i: usize, verdict: Verdict) -> Flow {
    Flow {
        verdict,
        drop_reason: 0,
        source: Endpoint {
            ip: Some(IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8))),
            port: Some(12000 + i as u16 % 40000),
            identity: Some(1000 + i as u32 % 100),
            namespace: "default".into(),
            pod_name: format!("client-{i}"),
            labels: vec!["app=client".into()],
        },
        destination: Endpoint {
            ip: Some(IpAddr::V4(Ipv4Addr::new(10, 96, 0, 1))),
            port: Some(80),
            identity: Some(42),
            namespace: "backend".into(),
            pod_name: "api-server".into(),
            labels: vec!["app=api".into()],
        },
        l4: None,
        traffic_direction: TrafficDirection::Ingress,
        reply: false,
        is_reply: None,
        node_name: "node-1".into(),
        event_type: None,
        time: None,
    }
}

fn make_observation(i: usize, verdict: FlowVerdict) -> FlowObservation {
    let src = make_flow_endpoint([10, 0, (i / 256) as u8, (i % 256) as u8], 12000 + i as u16 % 40000);
    let dst = make_flow_endpoint([10, 96, 0, 1], 80);
    let meta = FlowMetadata::new(
        format!("flow-{i}"),
        src,
        dst,
        Protocol::Tcp,
        FlowDirection::Ingress,
    );
    FlowObservation::new(meta, verdict)
}

// --- Flow ring ingestion ---
//
// The core path: eBPF perf event → Rust handler → FlowRing::push.
// Measures throughput at different ring capacities to show the eviction cost.

fn bench_flow_ring_push(c: &mut Criterion) {
    let mut group = c.benchmark_group("hubble_flow_ring_push");

    for capacity in [100usize, 4096, 65536] {
        group.throughput(Throughput::Elements(1));
        let flow = make_flow(0, Verdict::Forwarded);
        group.bench_with_input(BenchmarkId::new("capacity", capacity), &capacity, |b, &cap| {
            let mut ring = FlowRing::new(cap);
            for i in 0..cap {
                ring.push(make_flow(i, Verdict::Forwarded));
            }
            b.iter(|| ring.push(black_box(flow.clone())))
        });
    }

    group.finish();
}

// --- Flow ring query ---
//
// The hubble observe path: CLI / UI requests the last N flows.

fn bench_flow_ring_last_n(c: &mut Criterion) {
    let mut group = c.benchmark_group("hubble_flow_ring_last_n");

    let mut ring = FlowRing::new(65536);
    for i in 0..65536 {
        ring.push(make_flow(i, if i % 10 == 0 { Verdict::Dropped } else { Verdict::Forwarded }));
    }

    for n in [10usize, 100, 1000] {
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("last_n", n), &n, |b, &n| {
            b.iter(|| black_box(ring.last_n(n)))
        });
    }

    group.finish();
}

// --- Flow filtering ---
//
// Hubble relay applies filter predicates when streaming flows to `hubble observe`
// or the UI. This benchmarks single-predicate and compound AND filters over a
// full 4096-entry ring.

fn bench_flow_filter_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("hubble_flow_filter_scan");

    let mut ring = FlowRing::new(4096);
    for i in 0..4096 {
        ring.push(make_flow(i, if i % 5 == 0 { Verdict::Dropped } else { Verdict::Forwarded }));
    }

    group.bench_function("verdict_only", |b| {
        let filter = VerdictFilter { verdicts: vec![Verdict::Dropped] };
        b.iter(|| black_box(ring.iter().filter(|f| filter.matches(f)).count()))
    });

    group.bench_function("verdict_and_direction", |b| {
        let filter = AndFilter {
            filters: vec![
                Box::new(VerdictFilter { verdicts: vec![Verdict::Dropped] }),
                Box::new(DirectionFilter { direction: TrafficDirection::Ingress }),
            ],
        };
        b.iter(|| black_box(ring.iter().filter(|f| filter.matches(f)).count()))
    });

    group.finish();
}

// --- Flow summary aggregation ---
//
// Hubble Relay periodically rolls up observations into summary counts
// (forwarded / dropped / denied) for the UI panel.

fn bench_flow_summary(c: &mut Criterion) {
    let mut group = c.benchmark_group("hubble_flow_summary");

    for count in [100usize, 1000, 10000] {
        group.throughput(Throughput::Elements(count as u64));
        let observations: Vec<FlowObservation> = (0..count)
            .map(|i| {
                let v = match i % 10 {
                    0 => FlowVerdict::Dropped,
                    1 => FlowVerdict::Denied,
                    _ => FlowVerdict::Forwarded,
                };
                make_observation(i, v)
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("aggregate", count), &count, |b, _| {
            b.iter(|| black_box(FlowSummary::from_observations(black_box(&observations))))
        });
    }

    group.finish();
}

// --- Flow JSON serialization ---
//
// Hubble relay serializes flows to JSON for the gRPC-gateway REST clients
// and `hubble observe --output json`.

fn bench_flow_json_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("hubble_flow_json_serialize");

    for count in [1usize, 100, 1000] {
        let flows: Vec<Flow> = (0..count).map(|i| make_flow(i, Verdict::Forwarded)).collect();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::new("flows", count), &count, |b, _| {
            b.iter(|| black_box(serde_json::to_string(black_box(&flows)).unwrap()))
        });
    }

    group.finish();
}

criterion_group!(
    hubble_benches,
    bench_flow_ring_push,
    bench_flow_ring_last_n,
    bench_flow_filter_scan,
    bench_flow_summary,
    bench_flow_json_serialize,
);
criterion_main!(hubble_benches);
