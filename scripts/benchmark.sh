#!/usr/bin/env bash
# Benchmark Seriousum against upstream Cilium and publish repo artifacts.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$REPO_ROOT/target/bench"
PUBLISH_DIR="$REPO_ROOT/docs/generated"
mkdir -p "$OUT_DIR" "$PUBLISH_DIR"

SKIP_KIND=false
CILIUM_IMAGE="quay.io/cilium/cilium-ci:latest"
CLUSTER_NAME="bench-$(date +%s)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-kind) SKIP_KIND=true; shift ;;
    --cilium-image) CILIUM_IMAGE="$2"; shift 2 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

BLUE='\033[0;34m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()    { echo -e "${BLUE}[bench]${NC} $*"; }
success() { echo -e "${GREEN}[bench]${NC} $*"; }
warn()    { echo -e "${YELLOW}[bench]${NC} $*"; }

cleanup() {
  if command -v kind >/dev/null 2>&1; then
    kind delete cluster --name "$CLUSTER_NAME" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

num_or_na() {
  local v="$1"
  [[ -n "$v" ]] && echo "$v" || echo "N/A"
}

percent_delta() {
  local a="$1" b="$2"
  python3 - "$a" "$b" <<'PY'
import sys
try:
    a = float(sys.argv[1])
    b = float(sys.argv[2])
    if b == 0:
        print("N/A")
    else:
        pct = ((a - b) / b) * 100.0
        sign = "+" if pct > 0 else ""
        print(f"{sign}{pct:.1f}%")
except Exception:
    print("N/A")
PY
}

bench_ratio() {
  local seriousum="$1" cilium="$2"
  python3 - "$seriousum" "$cilium" <<'PY'
import re, sys

def parse(v: str):
    m = re.search(r'([0-9]+(?:\.[0-9]+)?)', v)
    return float(m.group(1)) if m else None

s = parse(sys.argv[1])
c = parse(sys.argv[2])
if s is None or c is None or c == 0:
    print("N/A")
else:
    ratio = s / c
    print(f"{ratio:.2f}x")
PY
}

ensure_helm_env() {
  export HELM_CACHE_HOME="$OUT_DIR/helm/cache"
  export HELM_CONFIG_HOME="$OUT_DIR/helm/config"
  export HELM_DATA_HOME="$OUT_DIR/helm/data"
  mkdir -p "$HELM_CACHE_HOME" "$HELM_CONFIG_HOME" "$HELM_DATA_HOME"
}

format_ns() {
  python3 - "$1" <<'PY'
import sys
try:
    ns = float(sys.argv[1])
except Exception:
    print("N/A")
    raise SystemExit
if ns >= 1_000_000:
    print(f"{ns / 1_000_000:.2f} ms")
elif ns >= 1_000:
    print(f"{ns / 1_000:.2f} µs")
else:
    print(f"{ns:.2f} ns")
PY
}

parse_estimate() {
  local json_path="$1"
  if [[ -f "$json_path" ]]; then
    local ns
    ns="$(python3 - "$json_path" <<'PY'
import json, sys
from pathlib import Path
p = Path(sys.argv[1])
obj = json.loads(p.read_text())
print(obj["median"]["point_estimate"])
PY
)"
    format_ns "$ns"
  else
    echo "N/A"
  fi
}

extract_upstream_binary_size_kb() {
  local cid tmp
  tmp="$(mktemp -d)"
  docker pull "$CILIUM_IMAGE" >/dev/null
  cid="$(docker create "$CILIUM_IMAGE")"
  docker cp "$cid":/usr/bin/cilium-agent "$tmp/cilium-agent"
  docker rm "$cid" >/dev/null
  local size
  size=$(( $(stat -c%s "$tmp/cilium-agent") / 1024 ))
  rm -rf "$tmp"
  echo "$size"
}

sample_top_avg() {
  local namespace="$1" selector="$2" column="$3"
  local sum=0 count=0 current
  for _ in $(seq 1 10); do
    current="$(kubectl top pod -n "$namespace" -l "$selector" --no-headers 2>/dev/null | awk -v col="$column" '{gsub(/m|Mi/, "", $col); sum+=$col; n++} END{if(n) printf "%.0f", sum/n}')"
    if [[ -n "$current" ]]; then
      sum=$((sum + current))
      count=$((count + 1))
    fi
    sleep 2
  done
  if [[ "$count" -gt 0 ]]; then
    echo $((sum / count))
  else
    echo "N/A"
  fi
}

install_metrics_server() {
  kubectl apply -f https://github.com/kubernetes-sigs/metrics-server/releases/latest/download/components.yaml >/dev/null
  kubectl patch deployment metrics-server -n kube-system \
    --type=json \
    -p='[{"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--kubelet-insecure-tls"}]' >/dev/null || true
  kubectl rollout status deployment/metrics-server -n kube-system --timeout=5m >/dev/null || true
}

# 1. Binary size
info "Building Seriousum release binaries..."
cd "$REPO_ROOT"
cargo build --release --locked -q

SERIOUSUM_BIN_KB=$(( $(stat -c%s target/release/seriousum-daemon) / 1024 ))
CILIUM_BIN_KB="$(extract_upstream_binary_size_kb)"
IMAGE_TAG="seriousum-agent:bench"

success "Binary sizes: seriousum-agent=${SERIOUSUM_BIN_KB} KB upstream-cilium-agent=${CILIUM_BIN_KB} KB"

# 2. System benchmarks on kind
SERIOUSUM_STARTUP_S="N/A"
CILIUM_STARTUP_S="N/A"
SERIOUSUM_RSS_MB="N/A"
CILIUM_RSS_MB="N/A"
SERIOUSUM_CPU_MCORES="N/A"
CILIUM_CPU_MCORES="N/A"

if [[ "$SKIP_KIND" == "false" ]]; then
  if ! command -v kind >/dev/null 2>&1 || ! command -v kubectl >/dev/null 2>&1 || ! command -v helm >/dev/null 2>&1; then
    warn "Skipping kind benchmarks: kind/kubectl/helm missing"
  else
    ensure_helm_env
    helm repo add cilium https://helm.cilium.io/ >/dev/null 2>&1 || true
    helm repo update cilium >/dev/null 2>&1 || true

    info "Creating kind cluster '$CLUSTER_NAME'..."
    if kind create cluster --name "$CLUSTER_NAME" --image kindest/node:v1.33.1 --wait 90s >/dev/null; then
      export KUBECONFIG
      KUBECONFIG="$(kind get kubeconfig --name "$CLUSTER_NAME")"

      install_metrics_server

      info "Building Seriousum benchmark image..."
      docker build -f "$REPO_ROOT/images/agent.Dockerfile" -t "$IMAGE_TAG" "$REPO_ROOT" >/dev/null
      kind load docker-image "$IMAGE_TAG" --name "$CLUSTER_NAME"

      info "Measuring Seriousum startup..."
      T0=$(date +%s%3N)
      helm install cilium cilium/cilium \
        --namespace kube-system \
        --set image.repository=seriousum-agent \
        --set image.tag=bench \
        --set image.useDigest=false \
        --set image.pullPolicy=Never \
        --set operator.image.repository=quay.io/cilium/operator \
        --set operator.image.tag=latest \
        --set operator.image.useDigest=false \
        --set ipam.mode=kubernetes \
        --set kubeProxyReplacement=false \
        --wait --timeout 10m >/dev/null
      T1=$(date +%s%3N)
      SERIOUSUM_STARTUP_S="$(python3 - <<PY
print(f"{(${T1}-${T0})/1000:.1f}")
PY
)"
      sleep 60
      SERIOUSUM_RSS_MB="$(sample_top_avg kube-system 'k8s-app=cilium' 3)"
      SERIOUSUM_CPU_MCORES="$(sample_top_avg kube-system 'k8s-app=cilium' 2)"
      success "Seriousum: startup=${SERIOUSUM_STARTUP_S}s rss=${SERIOUSUM_RSS_MB}Mi cpu=${SERIOUSUM_CPU_MCORES}m"

      helm uninstall cilium -n kube-system >/dev/null || true
      kubectl wait --for=delete pod -n kube-system -l k8s-app=cilium --timeout=5m >/dev/null 2>&1 || true
      sleep 20

      info "Measuring upstream Cilium startup..."
      T0=$(date +%s%3N)
      helm install cilium cilium/cilium \
        --namespace kube-system \
        --set ipam.mode=kubernetes \
        --set kubeProxyReplacement=false \
        --wait --timeout 10m >/dev/null
      T1=$(date +%s%3N)
      CILIUM_STARTUP_S="$(python3 - <<PY
print(f"{(${T1}-${T0})/1000:.1f}")
PY
)"
      sleep 60
      CILIUM_RSS_MB="$(sample_top_avg kube-system 'k8s-app=cilium' 3)"
      CILIUM_CPU_MCORES="$(sample_top_avg kube-system 'k8s-app=cilium' 2)"
      success "Cilium: startup=${CILIUM_STARTUP_S}s rss=${CILIUM_RSS_MB}Mi cpu=${CILIUM_CPU_MCORES}m"

      helm uninstall cilium -n kube-system >/dev/null || true
      kubectl wait --for=delete pod -n kube-system -l k8s-app=cilium --timeout=5m >/dev/null 2>&1 || true
    else
      warn "Kind cluster creation failed on this host; publishing binary and micro-benchmark results only"
    fi
  fi
else
  warn "Skipping kind benchmarks (--skip-kind)"
fi

# 3. Micro-benchmarks
info "Running Criterion micro-benchmarks..."
rm -rf "$REPO_ROOT/target/criterion"
: > "$OUT_DIR/criterion-raw.txt"
cargo build --profile bench --benches >/dev/null
for bench_name in load_balancer policy_eval ipam; do
  bench_bin="$(find "$REPO_ROOT/target/release/deps" -maxdepth 1 -type f -name "${bench_name}-*" ! -name '*.d' | head -1)"
  if [[ -n "$bench_bin" ]]; then
    "$bench_bin" --bench >> "$OUT_DIR/criterion-raw.txt" 2>&1
  fi
done

LB_RR_8="$(parse_estimate "$REPO_ROOT/target/criterion/lb_round_robin/backends/8/new/estimates.json")"
LB_CH_8="$(parse_estimate "$REPO_ROOT/target/criterion/lb_consistent_hash/backends/8/new/estimates.json")"
POL_1="$(parse_estimate "$REPO_ROOT/target/criterion/policy_eval/policies/1/new/estimates.json")"
POL_100="$(parse_estimate "$REPO_ROOT/target/criterion/policy_eval/policies/100/new/estimates.json")"
SEL_HIT="$(parse_estimate "$REPO_ROOT/target/criterion/selector_match/match_hit/new/estimates.json")"
IPAM_1K="$(parse_estimate "$REPO_ROOT/target/criterion/ipam_alloc_release_1000/new/estimates.json")"

TIMESTAMP="$(date -u +"%Y-%m-%d %H:%M UTC")"
GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"

STARTUP_DELTA="$(percent_delta "$SERIOUSUM_STARTUP_S" "$CILIUM_STARTUP_S")"
RSS_DELTA="$(percent_delta "$SERIOUSUM_RSS_MB" "$CILIUM_RSS_MB")"
CPU_DELTA="$(percent_delta "$SERIOUSUM_CPU_MCORES" "$CILIUM_CPU_MCORES")"
BIN_DELTA="$(percent_delta "$SERIOUSUM_BIN_KB" "$CILIUM_BIN_KB")"

cat > "$OUT_DIR/results.json" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "git_sha": "$GIT_SHA",
  "cilium_image": "$CILIUM_IMAGE",
  "system": {
    "binary_size_kb": {"seriousum": "$SERIOUSUM_BIN_KB", "cilium": "$CILIUM_BIN_KB", "delta": "$BIN_DELTA"},
    "startup_seconds": {"seriousum": "$SERIOUSUM_STARTUP_S", "cilium": "$CILIUM_STARTUP_S", "delta": "$STARTUP_DELTA"},
    "idle_memory_mib": {"seriousum": "$SERIOUSUM_RSS_MB", "cilium": "$CILIUM_RSS_MB", "delta": "$RSS_DELTA"},
    "idle_cpu_mcores": {"seriousum": "$SERIOUSUM_CPU_MCORES", "cilium": "$CILIUM_CPU_MCORES", "delta": "$CPU_DELTA"}
  },
  "microbench_seriousum": {
    "lb_round_robin_8_backends": "$LB_RR_8",
    "lb_consistent_hash_8_backends": "$LB_CH_8",
    "policy_eval_1_policy": "$POL_1",
    "policy_eval_100_policies": "$POL_100",
    "selector_match_hit": "$SEL_HIT",
    "ipam_alloc_release_1000": "$IPAM_1K"
  }
}
EOF

cat > "$PUBLISH_DIR/benchmark-results.json" <<EOF
$(cat "$OUT_DIR/results.json")
EOF

cat > "$PUBLISH_DIR/BENCHMARKS.md" <<EOF
# Benchmark Comparison: Seriousum vs Cilium

_Last updated: $TIMESTAMP · commit \`$GIT_SHA\`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## Methodology

### System-level comparison
- Kubernetes: **kindest/node v1.33.1**
- Cluster size: **1 control-plane + 0 workers** (single-node kind default)
- Install path: **Helm**
- Seriousum image: locally built from \`images/agent.Dockerfile\`
- Cilium image: **$CILIUM_IMAGE**
- Operator for Seriousum run: upstream operator image (current project default)
- Metrics sampled after **60s** stabilization
- CPU and memory averaged over **10 samples** via \`kubectl top pod\`

### Binary comparison
- Seriousum: \`target/release/seriousum-daemon\`
- Cilium: \`/usr/bin/cilium-agent\` extracted from \`$CILIUM_IMAGE\`

### Micro-benchmarks
- Framework: **criterion**
- Scope: Seriousum internal hot paths
- Note: These are included for regression tracking. The direct Seriousum-vs-Cilium comparison currently focuses on deployable system metrics.

## Published Results

### System-level comparison

| Metric | Seriousum | Cilium | Delta vs Cilium |
|---|---:|---:|---:|
| Agent binary size | **${SERIOUSUM_BIN_KB} KB** | ${CILIUM_BIN_KB} KB | ${BIN_DELTA} |
| Startup time | **${SERIOUSUM_STARTUP_S} s** | ${CILIUM_STARTUP_S} s | ${STARTUP_DELTA} |
| Idle memory (RSS / pod) | **${SERIOUSUM_RSS_MB} MiB** | ${CILIUM_RSS_MB} MiB | ${RSS_DELTA} |
| Idle CPU | **${SERIOUSUM_CPU_MCORES} m** | ${CILIUM_CPU_MCORES} m | ${CPU_DELTA} |

### Seriousum micro-benchmarks

| Benchmark | Result |
|---|---:|
| Load balancer round-robin (8 backends) | ${LB_RR_8} |
| Load balancer consistent hash (8 backends) | ${LB_CH_8} |
| Policy evaluation (1 policy) | ${POL_1} |
| Policy evaluation (100 policies) | ${POL_100} |
| Selector match (hit) | ${SEL_HIT} |
| IPAM allocate + release ×1000 | ${IPAM_1K} |

## Reproduce locally

```bash
# Full comparison
./scripts/benchmark.sh

# Skip kind and run micro-benchmarks only
./scripts/benchmark.sh --skip-kind

# View raw machine-readable output
cat target/bench/results.json
```

## Notes

- Startup, memory, and CPU are the primary apples-to-apples comparison currently published.
- Seriousum still reuses upstream operator images during Helm-based compatibility runs.
- Future benchmark expansions can add direct upstream Go micro-benchmarks for policy and allocator internals.
EOF

cat > "$OUT_DIR/readme-bench.md" <<EOF
<!-- BENCHMARK_START -->
## 📊 Benchmarks

> Last run: **$TIMESTAMP** · commit \`$GIT_SHA\`
> Published comparison report: [docs/generated/BENCHMARKS.md](docs/generated/BENCHMARKS.md)

| Metric | Seriousum | Cilium | Delta vs Cilium |
|---|---:|---:|---:|
| Agent binary size | **${SERIOUSUM_BIN_KB} KB** | ${CILIUM_BIN_KB} KB | ${BIN_DELTA} |
| Startup time | **${SERIOUSUM_STARTUP_S} s** | ${CILIUM_STARTUP_S} s | ${STARTUP_DELTA} |
| Idle memory | **${SERIOUSUM_RSS_MB} MiB** | ${CILIUM_RSS_MB} MiB | ${RSS_DELTA} |
| Idle CPU | **${SERIOUSUM_CPU_MCORES} m** | ${CILIUM_CPU_MCORES} m | ${CPU_DELTA} |

### Seriousum micro-benchmarks

| Benchmark | Result |
|---|---:|
| LB round-robin (8 backends) | ${LB_RR_8} |
| LB consistent hash (8 backends) | ${LB_CH_8} |
| Policy eval (1 policy) | ${POL_1} |
| Policy eval (100 policies) | ${POL_100} |
| Selector match (hit) | ${SEL_HIT} |
| IPAM alloc + release ×1000 | ${IPAM_1K} |

<details>
<summary>Reproduce locally</summary>

```bash
./scripts/benchmark.sh
```

</details>
<!-- BENCHMARK_END -->
EOF

python3 - <<'PY'
import pathlib, re
repo = pathlib.Path("/var/home/james/dev/seriousum")
readme = repo / "README.md"
section = (repo / "target/bench/readme-bench.md").read_text().strip()
text = readme.read_text()
pattern = r"<!-- BENCHMARK_START -->.*?<!-- BENCHMARK_END -->"
if re.search(pattern, text, flags=re.S):
    text = re.sub(pattern, section, text, flags=re.S)
else:
    text = text.rstrip() + "\n\n" + section + "\n"
readme.write_text(text)
PY

success "Published benchmark artifacts:"
echo "  - $OUT_DIR/results.json"
echo "  - $PUBLISH_DIR/benchmark-results.json"
echo "  - $PUBLISH_DIR/BENCHMARKS.md"
echo "  - README.md benchmark section"
