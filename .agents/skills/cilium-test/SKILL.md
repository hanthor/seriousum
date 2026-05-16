---
name: cilium-test
description: Run the upstream Cilium ginkgo integration test suite against the seriousum Rust agent to validate compatibility. Use after implementing any porting track, or when asked to "run cilium tests", "validate compatibility", "run ginkgo", or "test against cilium". Provides exact commands to build the Rust image, spin up a kind cluster, inject the image, and execute any of the 19 focus groups.
compatibility: Requires kind, kubectl, helm, docker, go 1.21+. Needs 12 GB RAM and 4 CPUs. Run on Linux only (eBPF). Must be run as a user with Docker socket access.
---

# Cilium Test Skill

Validates seriousum Rust code against the upstream Cilium ginkgo test harness.

## Paths

```
Rust workspace:        ~/dev/seriousum
Cilium Go source:      ~/dev/cilium   (upstream test suite lives here)
Test runner script:    ~/dev/seriousum/scripts/run-cilium-kind-test.sh
Justfile recipes:      ~/dev/seriousum/justfile
```

---

## Quick Start — Run a single focus group

```bash
cd ~/dev/seriousum

# 1. Build the Rust agent image
cargo build --release --locked
docker build -f images/cilium-agent.Dockerfile \
  -t seriousum-agent:dev .

# 2. Run a focus group (pick from the table below)
./scripts/run-cilium-kind-test.sh \
  --focus "K8sAgentFQDNTest" \
  --timeout 30m
```

The script handles everything: creates a kind cluster, loads the image, installs Cilium via Helm, runs ginkgo, collects results, and tears down the cluster.

---

## Focus Group Reference

| Focus ID | ginkgo --focus regex | Tests | Prereqs |
|----------|---------------------|-------|---------|
| f01 | `K8sAgentChaosTest` | Graceful shutdown, restart resilience | — |
| f02 | `K8sAgentFQDNTest\|K8sAgentPerNodeConfigTest` | FQDN policy, per-node config | — |
| f03 | `K8sAgentPolicyTest Clusterwide\|External\|Namespaces` | Policy scoping | — |
| f04 | `K8sAgentPolicyTest Multi-node.*validates fromEntities` | Multi-node identity | 3-node cluster |
| f05 | `K8sAgentPolicyTest Multi-node.*validates ingress` | CIDR ingress policy | 3-node cluster |
| f06 | `K8sAgentPolicyTest Basic\|K8sPolicyTestExtended` | L7 proxy, KubeAPIServer policy | — |
| f10 | `K8sAgentHubbleTest` | Hubble L3/L4/L7 flows, TLS | Hubble enabled |
| f11 | `K8sDatapathServicesTest.*Tests with TC` | N/S LB TC/DSR/Geneve | net-next kernel |
| f12 | `K8sDatapathServicesTest.*GH\|NodePort\|security\|direct` | N/S LB misc | net-next kernel |
| f13 | `K8sDatapathServicesTest.*XDP.*DSR\|Hybrid` | XDP DSR/Hybrid | net-next kernel |
| f14 | `K8sDatapathServicesTest.*XDP.*SNAT\|host policy.*NodePort` | XDP SNAT | net-next kernel |
| f15 | `K8sDatapathServicesTest Checks device\|in-cluster KPR` | E/W LB, KPR, HealthCheck | — |
| f16 | `K8sDatapathServicesTest.*hairpin\|TFTP\|L4 policy\|L7 policy` | E/W LB misc | kube-proxy |
| f17 | `K8sDatapathServicesTest.*kube-proxy` | E/W LB with kube-proxy | kube-proxy |
| f18 | `K8sDatapathLRPTests` | Local redirect policy | — |
| f19 | `K8sSpecificMACAddressTests` | Pod MAC address | — |

**Start here for basic validation** (no special kernel needed): f01, f02, f03, f06, f15, f18

---

## Detailed Usage

### Build & load the image

```bash
cd ~/dev/seriousum

# Release build (fast image, ~2.6 MB)
cargo build --release --locked

# Build agent Docker image
docker build \
  -f images/cilium-agent.Dockerfile \
  -t seriousum-agent:$(git rev-parse --short HEAD) \
  -t seriousum-agent:dev \
  .

# Verify the image runs
docker run --rm seriousum-agent:dev --version
```

### Create a kind cluster and load the image

```bash
# 2-node cluster (sufficient for most tests)
kind create cluster --name cilium-test \
  --image kindest/node:v1.33.1 \
  --wait 60s

# Load our image
kind load docker-image seriousum-agent:dev --name cilium-test

export KUBECONFIG=$(kind get kubeconfig --name cilium-test)
kubectl get nodes
```

### Install Cilium with the Rust agent

```bash
helm repo add cilium https://helm.cilium.io/ 2>/dev/null || true

helm install cilium cilium/cilium \
  --namespace kube-system \
  --set image.repository=seriousum-agent \
  --set image.tag=dev \
  --set image.pullPolicy=Never \
  --set "operator.image.repository=quay.io/cilium/operator" \
  --set operator.image.tag=latest \
  --set ipam.mode=kubernetes \
  --set kubeProxyReplacement=false \
  --set hubble.enabled=true \
  --wait --timeout 10m

# Verify
kubectl -n kube-system get pods -l app.kubernetes.io/part-of=cilium
```

### Build the ginkgo test binary

```bash
# Check out upstream Cilium (or use local copy)
CILIUM_SRC=~/dev/cilium

cd $CILIUM_SRC/test

# Build once, cache for subsequent runs
go install github.com/onsi/ginkgo/ginkgo@v1.16.5
ginkgo build
strip test.test
```

### Run a focus group

```bash
cd $CILIUM_SRC/test

export K8S_VERSION=1.33
export CNI_INTEGRATION=kind
export INTEGRATION_TESTS=true
export CILIUM_NO_IPV6_OUTSIDE=true

./test.test \
  --ginkgo.focus="K8sAgentFQDNTest" \
  --ginkgo.seed=1679952881 \
  --ginkgo.v \
  -- \
  -cilium.image=seriousum-agent \
  -cilium.tag=dev \
  -cilium.operator-image=quay.io/cilium/operator \
  -cilium.operator-tag=latest \
  -cilium.kubeconfig=$KUBECONFIG \
  -cilium.operator-suffix=""
```

### Run multiple focus groups in parallel (3 clusters)

```bash
cd ~/dev/seriousum

# Runs f01, f02, f03 on 3 separate kind clusters simultaneously
./scripts/run-parallel-test-suites.sh f01 f02 f03

# Collect aggregated results
./scripts/collect-parallel-results.sh
cat target/parallel-test-results/AGGREGATED_RESULTS.md
```

### Cleanup

```bash
kind delete cluster --name cilium-test
# Or delete all test clusters at once:
kind get clusters | grep "^cilium\|^bench\|^smoke\|^conn" | xargs -I{} kind delete cluster --name {}
```

---

## Interpret Results

### JUnit XML

The test binary writes `*.xml` files to `test/test_results/`. Parse them:

```bash
# Quick pass/fail summary
grep -h 'tests=\|failures=\|errors=' $CILIUM_SRC/test/test_results/*.xml

# List failing tests
grep '<failure' $CILIUM_SRC/test/test_results/*.xml | grep -oP 'name="[^"]+"'
```

### Expected pass rate by implementation status

| Track status | Expected pass rate |
|---|---|
| Scaffold only (current) | 0–20% (basic infra only) |
| K8s watchers + policy model | 40–60% |
| Real eBPF maps + datapath | 70–85% |
| Full implementation | 90–95% |

### Common failure modes

| Error | Likely cause | Fix |
|-------|-------------|-----|
| `BeforeSuite failed` | Agent not ready / CNI socket missing | Check agent logs: `kubectl -n kube-system logs -l app=cilium-agent` |
| `endpoint not found` | Endpoint manager not implemented | Track G |
| `policy not enforced` | Policy engine not wired to eBPF | Track F + A |
| `service IP unreachable` | LB maps not populated | Track I |
| `FQDN not resolved` | DNS proxy not running | Track K |
| `connection timeout` | eBPF programs not loaded | Track B |

---

## Justfile Shortcuts

```bash
# Run a single focus group
just run "K8sAgentFQDNTest"

# Run all policy tests
just run "K8sAgentPolicyTest"

# Run datapath services
just run "K8sDatapathServicesTest"

# Run 3 suites in parallel
just test-parallel

# Full sequential run of all suites
just test-all-sequential
```

---

## CI Integration

The `.github/workflows/conformance-ginkgo.yaml` workflow runs all 19 focus groups on every push to main. Results are uploaded as JUnit artifacts and summarised in the PR checks.

To trigger manually:
```bash
gh workflow run conformance-ginkgo.yaml \
  --repo hanthor/seriousum \
  --field focus=f02-agent-fqdn
```

---

## Validating a Specific Track

After implementing a track, run the corresponding focus group:

```bash
TRACK=F   # e.g. policy engine
FOCUS="K8sAgentPolicyTest"

cd ~/dev/seriousum
cargo build --release --locked
docker build -f images/cilium-agent.Dockerfile -t seriousum-agent:dev .

./scripts/run-cilium-kind-test.sh \
  --focus "$FOCUS" \
  --timeout 45m \
  --cluster-name "validate-track-$TRACK"
```

Target: **≥80% pass rate** before merging a track PR.
