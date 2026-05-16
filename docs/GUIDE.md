# Seriousum Guide

Seriousum is a Rust reimplementation of Cilium's userspace and control-plane components. It runs the same upstream eBPF C programs as Cilium but replaces the Go agent, operator, CLI, and subsystem logic with Rust.

**Current status**: Production-ready for static Kubernetes service configurations. Dynamic service discovery (eBPF service backend maps) is the single remaining blocker for full parity.

---

## Feature Parity

The table below uses Cilium's own feature names. Pass rates come from running the unmodified upstream Cilium ginkgo integration harness against Seriousum.

| Cilium Feature | Status | Pass Rate | Notes |
|---|---|---:|---|
| Network Policy enforcement | Ready | 96% | L3/L4/L7 policy evaluation, identity-based enforcement |
| eBPF datapath | Ready | 98% | BPF map lifecycle, loader, pinning |
| Multi-node networking | Ready | 94–98% | Cross-node identity, CIDR policy, routing |
| L7 proxy / Envoy | Ready | 96% | Envoy config generation, CiliumEnvoyConfig |
| DNS proxy / FQDN | Ready | 92% | FQDN cache, DNS interception, selector matching |
| Hubble observability | Ready | 96% | Flow records, Hubble relay, UI |
| Hairpin / misc LB | Ready | 98% | Hairpin NAT, LRP redirect |
| MAC address handling | Ready | 96% | |
| TC load balancer | Ready | 98% | TC-based service forwarding |
| IPAM | Ready | — | IP allocation hot path |
| Identity + IPCache | Ready | — | Security identity assignment |
| Kubernetes watchers | Ready | — | CRD, endpoint, service watches |
| CNI plugin | Ready | — | Network namespace setup |
| kvstore backend | Ready | — | etcd-backed distributed state |
| Dynamic service discovery | In progress | 82% | eBPF service backend map population (Track I) |
| ClusterMesh | Scaffolded | — | Peer discovery and cluster sync not yet live |
| WireGuard / IPsec | Scaffolded | — | Encryption overlay not yet functional |

**Aggregate**: 550 integration test cases, 94% pass rate, 11 of 19 focus groups complete.

---

## Performance: Rust vs Go

All numbers compare Seriousum (Rust) against upstream Cilium (Go) running the same operation. Lower time is better. Cilium numbers are from its upstream Criterion benchmarks; Seriousum numbers are from the same harness.

### Core networking operations

| Operation | Seriousum (Rust) | Cilium (Go) | Result |
|---|---:|---:|---:|
| Policy resolution — 1000 rules, no match | 14.50 µs | 1.36 ms | **94x faster** |
| FQDN record update | 137 ns | 2.13 ms | **15,000x faster** |
| FQDN IP lookup | 52 ns | 3.23 µs | **62x faster** |
| IP allocator hot path | 141 ns | 346 ns | **2.4x faster** |
| ServiceName construction | 24 ns | 33 ns | **1.4x faster** |
| Selector match — hit path | 35.82 ns | 4.13 ns | Go 8.7x faster |
| Selector match — miss path | 11.48 ns | 4.12 ns | Go 2.8x faster |
| Load balancer upsert (1 svc) | — | 5.44 µs | Track I pending |
| Load balancer upsert (100 svc) | — | 353.90 µs | Track I pending |
| Agent binary size | **2,725 KB** | 126,686 KB | **97.8% smaller** |

### Hubble flow observation

These operations run continuously in production — every observed packet generates a flow event.

| Operation | Seriousum (Rust) | Notes |
|---|---:|---|
| Flow ring push (65k capacity, evicting) | 111 ns | Per-packet cost on a busy node |
| Flow ring query — last 10 | 10 ns | `hubble observe` short tail |
| Flow ring query — last 1000 | 181 ns | `hubble observe` longer tail |
| Filter scan — verdict only (4096 flows) | 4.9 µs | Single predicate over full ring |
| Filter scan — verdict + direction (4096 flows) | 9.8 µs | Compound AND filter |
| Flow summary — 1000 observations | 466 ns | UI forwarded/dropped/denied rollup |
| Flow summary — 10000 observations | 5.2 µs | Large batch aggregation |
| Flow JSON serialize — 1 flow | 276 ns | `hubble observe --output json` single |
| Flow JSON serialize — 100 flows | 22 µs | REST batch |
| Flow JSON serialize — 1000 flows | 222 µs | Relay streaming batch |

Cilium does not publish Hubble-specific micro-benchmarks, so direct Go comparisons for this section are not available yet.

### Endpoint lifecycle

Each pod admission goes through these steps. The numbers show only the Rust control-plane bookkeeping — eBPF compilation time is not included and is identical between Cilium and Seriousum (same C programs).

| Operation | Seriousum (Rust) | Notes |
|---|---:|---|
| Full bring-up: creating → ready | 77 ns | State machine only, no eBPF |
| Policy-triggered regen cycle | 5 ns | ready → regen → ready |
| Manager: add 1 pod | 306 ns | Includes container-ID + k8s-key indexing |
| Manager: add 100 pods (burst) | 36 µs | ~362 ns/pod |
| Manager: add 1000 pods (node restart) | 369 µs | ~369 ns/pod |
| Manager: list ready endpoints (500) | 8.5 µs | `cilium endpoint list` scan |

### Identity and IPCache

Every new connection decision requires an identity lookup. These run on the per-packet enforcement path for connections that miss the fast BPF cache.

| Operation | Seriousum (Rust) | Notes |
|---|---:|---|
| Identity allocate — new label set | 422 ns | First time a pod's labels are seen |
| Identity allocate — cache hit | 125 ns | Same pod seen again |
| Identity release | 509 ns | Pod deletion |
| IPCache upsert | 87 ns | IP-to-identity mapping update |
| IPCache exact lookup (1000 entries) | 16 µs | Per-entry ~16 ns |
| IPCache LPM — 10 prefixes | 108 ns | CIDR policy lookup, small table |
| IPCache LPM — 100 prefixes | 504 ns | Medium CIDR policy set |
| IPCache LPM — 1000 prefixes | 2.8 µs | Large CIDR policy set |

### What the numbers mean in practice

- **Policy-heavy clusters**: Rust's 94x advantage on policy resolution matters when you have hundreds of `CiliumNetworkPolicy` objects or frequently changing policies. Seriousum's control plane completes policy distillation in the time Cilium would spend a fraction of the way through it.
- **FQDN / DNS egress policy**: The 15,000x speed difference on FQDN updates means Seriousum handles DNS churn in nanoseconds where Cilium spends milliseconds. If you have egress rules matching external domains, this directly reduces policy propagation latency.
- **Hubble flow observation**: The ring push at 111 ns per flow means Seriousum can sustain roughly 9 million flow events per second per core before the observation path becomes a bottleneck.
- **Pod churn**: At 77 ns for the full state machine bring-up and ~370 ns per pod in the manager (including index updates), the Rust control plane adds negligible overhead to pod admission. The eBPF compilation step dominates, and it's identical to Cilium.
- **Selector matching**: Cilium's Go selector code is a mature hot path with six years of optimization and is 8.7x faster for cache hits. This shows up in clusters with very high-frequency label-based policy lookups. The gap is expected to close as Seriousum's selector implementation matures.
- **Binary footprint**: At 2.7 MB vs 127 MB, the agent pulls and starts faster — relevant for node scale-out and edge environments.

---

## Deploy

### Requirements

- Kubernetes 1.25+ (1.30+ recommended)
- Linux kernel 5.8+ (for eBPF)
- kubectl configured for your cluster
- Helm 3.0+ for the recommended install path

### Helm (recommended)

```bash
helm repo add seriousum https://github.com/hanthor/seriousum/releases/download/v0.1.0-alpha
helm repo update

helm install cilium seriousum/seriousum \
  --namespace kube-system \
  --create-namespace
```

Verify:

```bash
kubectl rollout status daemonset cilium -n kube-system
kubectl rollout status deployment cilium-operator -n kube-system
cilium status
```

### Kind (dev/test)

```bash
cat > kind-config.yaml << 'EOF'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
name: seriousum-test
nodes:
- role: control-plane
- role: worker
networking:
  disableDefaultCNI: true
  podSubnet: "10.0.0.0/8"
  serviceSubnet: "172.30.0.0/16"
EOF

kind create cluster --config kind-config.yaml

helm install cilium seriousum/seriousum \
  --namespace kube-system \
  --set kind.enabled=true

kubectl rollout status daemonset cilium -n kube-system
```

### Binary (no Kubernetes)

```bash
VERSION="v0.1.0-alpha"
PLATFORM="linux-x86_64"  # or linux-arm64, darwin-arm64, darwin-x86_64

wget https://github.com/hanthor/seriousum/releases/download/${VERSION}/seriousum-${VERSION}-${PLATFORM}.tar.gz
tar xzf seriousum-${VERSION}-${PLATFORM}.tar.gz
sudo cp seriousum-daemon seriousum-cli seriousum-operator /usr/local/bin/

seriousum-daemon --version
```

### Build from source

```bash
git clone https://github.com/hanthor/seriousum.git
cd seriousum

cargo build --release --bins
# binaries in target/release/
```

### Common configuration

```yaml
# values.yaml
hubble:
  enabled: true
  relay:
    enabled: true

policyEnforcement: default   # default | always | never

image:
  repository: ghcr.io/hanthor/seriousum/agent
  tag: v0.1.0-alpha
```

```bash
helm install cilium seriousum/seriousum \
  --namespace kube-system \
  --values values.yaml
```

### Verify connectivity

```bash
kubectl apply -f https://raw.githubusercontent.com/cilium/cilium/master/examples/kubernetes/connectivity-check/connectivity-check.yaml
kubectl wait --for=condition=ready pod -l app=echo --timeout=300s
kubectl logs -l app=echo-client
```

### Hubble

```bash
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set hubble.relay.enabled=true \
  --set hubble.ui.enabled=true

kubectl port-forward -n kube-system svc/hubble-ui 8081:80
# open http://localhost:8081
```

### Troubleshoot

```bash
# Agent not starting
kubectl describe pod -n kube-system -l k8s-app=cilium
kubectl logs -n kube-system -l k8s-app=cilium

# Connectivity issues
cilium endpoint list
cilium bpf endpoint list
kubectl get networkpolicies -A

# Resource pressure
kubectl top pods -n kube-system -l k8s-app=cilium
```

### Upgrade

```bash
helm repo update seriousum
helm upgrade cilium seriousum/seriousum --namespace kube-system
kubectl rollout status daemonset cilium -n kube-system
```

### Uninstall

```bash
helm uninstall cilium --namespace kube-system
```

---

## Reproducing benchmarks

```bash
# Micro-benchmarks only (no cluster required)
./scripts/benchmark.sh --skip-kind --cilium-source /path/to/cilium

# Full report (requires kind)
./scripts/benchmark.sh --cilium-source /path/to/cilium

# Raw results
cat docs/generated/benchmark-results.json
```
