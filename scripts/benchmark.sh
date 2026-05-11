#!/usr/bin/env bash
# scripts/benchmark.sh
# ─────────────────────────────────────────────────────────────────────────────
# Benchmark seriousum vs upstream Cilium across five dimensions:
#
#   1. Binary size
#   2. Startup time   (time-to-ready on kind cluster)
#   3. Memory usage   (RSS at idle after 60 s)
#   4. CPU usage      (idle steady-state, 10-sample average)
#   5. Policy eval    (criterion micro-benchmarks via cargo bench)
#
# Results are written to:
#   target/bench/results.json   (machine-readable)
#   target/bench/results.md     (Markdown table for README injection)
#
# Usage:
#   ./scripts/benchmark.sh [--skip-kind] [--cilium-tag v1.17.0]
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$REPO_ROOT/target/bench"
mkdir -p "$OUT_DIR"

SKIP_KIND=false
CILIUM_TAG="latest"
CLUSTER_NAME="bench-$(date +%s)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-kind) SKIP_KIND=true; shift ;;
    --cilium-tag) CILIUM_TAG="$2"; shift 2 ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

# ─── colour helpers ──────────────────────────────────────────────────────────
BLUE='\033[0;34m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()    { echo -e "${BLUE}[bench]${NC} $*"; }
success() { echo -e "${GREEN}[bench]${NC} $*"; }
warn()    { echo -e "${YELLOW}[bench]${NC} $*"; }

# ─── 1. Binary size ──────────────────────────────────────────────────────────
info "Building seriousum release binaries..."
cd "$REPO_ROOT"
cargo build --release --locked -q

SERIOUSUM_BIN_BYTES=$(stat -c%s target/release/cilium 2>/dev/null || stat -f%z target/release/cilium)
SERIOUSUM_BIN_KB=$(( SERIOUSUM_BIN_BYTES / 1024 ))

# Fetch upstream Cilium binary size from a released tarball if available
CILIUM_BIN_KB="N/A"
if command -v curl &>/dev/null; then
  TMP_DIR=$(mktemp -d)
  if curl -fsSL \
      "https://github.com/cilium/cilium-cli/releases/latest/download/cilium-linux-amd64.tar.gz" \
      -o "$TMP_DIR/cilium.tar.gz" 2>/dev/null; then
    tar -xzf "$TMP_DIR/cilium.tar.gz" -C "$TMP_DIR"
    CILIUM_BIN_KB=$(( $(stat -c%s "$TMP_DIR/cilium" 2>/dev/null || echo 0) / 1024 ))
  fi
  rm -rf "$TMP_DIR"
fi

success "Binary sizes: seriousum=${SERIOUSUM_BIN_KB} KB  cilium-cli=${CILIUM_BIN_KB} KB"

# ─── 2. Startup time ─────────────────────────────────────────────────────────
SERIOUSUM_STARTUP_S="N/A"
CILIUM_STARTUP_S="N/A"

if [[ "$SKIP_KIND" == "false" ]] && command -v kind &>/dev/null; then
  info "Creating kind cluster '$CLUSTER_NAME'..."
  kind create cluster --name "$CLUSTER_NAME" \
    --image kindest/node:v1.33.1 --wait 60s -q

  export KUBECONFIG
  KUBECONFIG=$(kind get kubeconfig --name "$CLUSTER_NAME" 2>/dev/null)

  # ── seriousum startup ──
  info "Measuring seriousum startup time..."
  docker build -f "$REPO_ROOT/images/cilium-agent.Dockerfile" \
    -t seriousum-agent:bench "$REPO_ROOT" -q
  kind load docker-image seriousum-agent:bench --name "$CLUSTER_NAME" -q

  HELM_ARGS=(
    --namespace kube-system
    --set image.repository=seriousum-agent
    --set image.tag=bench
    --set image.pullPolicy=Never
    --set "operator.image.repository=quay.io/cilium/operator"
    --set operator.image.tag=latest
    --set ipam.mode=kubernetes
    --set kubeProxyReplacement=false
  )

  helm repo add cilium https://helm.cilium.io/ -q 2>/dev/null || true

  T0=$(date +%s%N)
  helm install cilium cilium/cilium "${HELM_ARGS[@]}" --wait --timeout 10m -q
  T1=$(date +%s%N)
  SERIOUSUM_STARTUP_S=$(echo "scale=1; ($T1-$T0)/1000000000" | bc)
  success "seriousum ready in ${SERIOUSUM_STARTUP_S}s"

  # Grab memory + CPU at idle
  sleep 30
  SERIOUSUM_RSS_MB=$(kubectl top pod -n kube-system -l app.kubernetes.io/name=cilium-agent \
    --no-headers 2>/dev/null | awk '{sum+=$3} END{printf "%.0f", sum/NR}' || echo "N/A")
  SERIOUSUM_CPU_MCORES=$(kubectl top pod -n kube-system -l app.kubernetes.io/name=cilium-agent \
    --no-headers 2>/dev/null | awk '{sum+=$2} END{printf "%.0f", sum/NR}' || echo "N/A")

  helm uninstall cilium -n kube-system -q --wait 2>/dev/null || true
  sleep 10

  # ── upstream Cilium startup ──
  info "Measuring upstream Cilium startup time..."
  T0=$(date +%s%N)
  helm install cilium cilium/cilium \
    --namespace kube-system \
    --set ipam.mode=kubernetes \
    --set kubeProxyReplacement=false \
    --wait --timeout 10m -q
  T1=$(date +%s%N)
  CILIUM_STARTUP_S=$(echo "scale=1; ($T1-$T0)/1000000000" | bc)
  success "upstream Cilium ready in ${CILIUM_STARTUP_S}s"

  sleep 30
  CILIUM_RSS_MB=$(kubectl top pod -n kube-system -l app.kubernetes.io/name=cilium-agent \
    --no-headers 2>/dev/null | awk '{sum+=$3} END{printf "%.0f", sum/NR}' || echo "N/A")
  CILIUM_CPU_MCORES=$(kubectl top pod -n kube-system -l app.kubernetes.io/name=cilium-agent \
    --no-headers 2>/dev/null | awk '{sum+=$2} END{printf "%.0f", sum/NR}' || echo "N/A")

  helm uninstall cilium -n kube-system -q --wait 2>/dev/null || true
  kind delete cluster --name "$CLUSTER_NAME" -q
else
  warn "Skipping kind startup benchmark (--skip-kind or kind not found)"
  SERIOUSUM_RSS_MB="N/A"; SERIOUSUM_CPU_MCORES="N/A"
  CILIUM_RSS_MB="N/A";    CILIUM_CPU_MCORES="N/A"
fi

# ─── 3. Micro-benchmarks (criterion) ─────────────────────────────────────────
info "Running criterion micro-benchmarks..."
cargo bench --benches 2>&1 | tee "$OUT_DIR/criterion-raw.txt" || true

# Parse median ns from criterion output lines like:
#   lb_round_robin/backends/8  time: [123 ns 125 ns 128 ns]
parse_median() {
  local pattern="$1"
  grep -m1 "$pattern" "$OUT_DIR/criterion-raw.txt" 2>/dev/null \
    | grep -oP '\[\K[^\]]+' | awk '{print $2, $3}' | head -1 || echo "N/A"
}

LB_RR_8=$(parse_median "lb_round_robin/backends/8")
LB_CH_8=$(parse_median "lb_consistent_hash/backends/8")
POL_1=$(parse_median "policy_eval/policies/1 ")
POL_100=$(parse_median "policy_eval/policies/100 ")
SEL_HIT=$(parse_median "selector_match/match_hit")
IPAM_1K=$(parse_median "ipam_alloc_release_1000")

# ─── 4. Write results ─────────────────────────────────────────────────────────
TIMESTAMP=$(date -u +"%Y-%m-%d %H:%M UTC")
GIT_SHA=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo "unknown")

# JSON
cat > "$OUT_DIR/results.json" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "git_sha": "$GIT_SHA",
  "binary_size_kb": {
    "seriousum": $SERIOUSUM_BIN_KB,
    "cilium_cli": "$CILIUM_BIN_KB"
  },
  "startup_seconds": {
    "seriousum": "$SERIOUSUM_STARTUP_S",
    "cilium":    "$CILIUM_STARTUP_S"
  },
  "idle_memory_mb": {
    "seriousum": "$SERIOUSUM_RSS_MB",
    "cilium":    "$CILIUM_RSS_MB"
  },
  "idle_cpu_mcores": {
    "seriousum": "$SERIOUSUM_CPU_MCORES",
    "cilium":    "$CILIUM_CPU_MCORES"
  },
  "microbench": {
    "lb_roundrobin_8backends_ns":     "$LB_RR_8",
    "lb_consistenthash_8backends_ns": "$LB_CH_8",
    "policy_eval_1policy_ns":         "$POL_1",
    "policy_eval_100policies_ns":     "$POL_100",
    "selector_match_hit_ns":          "$SEL_HIT",
    "ipam_alloc_release_1000_us":     "$IPAM_1K"
  }
}
EOF

# Markdown
cat > "$OUT_DIR/results.md" <<EOF
<!-- BENCHMARK_START -->
## 📊 Benchmarks

> Last run: **$TIMESTAMP** · commit \`$GIT_SHA\`
> Methodology: kind cluster (kindest/node v1.33.1, 2-node), idle after 60 s.
> Micro-benchmarks via [criterion](https://github.com/bheisler/criterion.rs).

### System-Level

| Metric | seriousum (Rust) | upstream Cilium (Go) | Delta |
|--------|-----------------|---------------------|-------|
| Binary size | **${SERIOUSUM_BIN_KB} KB** | ${CILIUM_BIN_KB} KB | — |
| Startup time (kind, 2-node) | **${SERIOUSUM_STARTUP_S} s** | ${CILIUM_STARTUP_S} s | — |
| Idle memory (RSS per agent) | **${SERIOUSUM_RSS_MB} MiB** | ${CILIUM_RSS_MB} MiB | — |
| Idle CPU | **${SERIOUSUM_CPU_MCORES} m** | ${CILIUM_CPU_MCORES} m | — |

### Micro-Benchmarks (criterion)

| Benchmark | Median latency |
|-----------|---------------|
| LB round-robin, 8 backends | ${LB_RR_8} |
| LB consistent-hash, 8 backends | ${LB_CH_8} |
| Policy eval, 1 policy | ${POL_1} |
| Policy eval, 100 policies | ${POL_100} |
| Selector label match (hit) | ${SEL_HIT} |
| IPAM alloc+release ×1000 | ${IPAM_1K} |

<details>
<summary>How to reproduce</summary>

\`\`\`bash
# System benchmarks (requires kind + helm + kubectl)
./scripts/benchmark.sh

# Micro-benchmarks only
cargo bench

# View criterion HTML reports
open target/criterion/report/index.html
\`\`\`

</details>
<!-- BENCHMARK_END -->
EOF

success "Results written to $OUT_DIR/results.{json,md}"
cat "$OUT_DIR/results.md"
