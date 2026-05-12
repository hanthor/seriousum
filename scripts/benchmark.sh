#!/usr/bin/env bash
# Publish Seriousum vs upstream Cilium benchmark artifacts.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$REPO_ROOT/target/bench"
PUBLISH_DIR="$REPO_ROOT/docs/generated"
mkdir -p "$OUT_DIR" "$PUBLISH_DIR"

SKIP_KIND=false
CILIUM_IMAGE="quay.io/cilium/cilium-ci:latest"
CILIUM_SOURCE=""
CLUSTER_NAME="bench-$(date +%s)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-kind) SKIP_KIND=true; shift ;;
    --cilium-image) CILIUM_IMAGE="$2"; shift 2 ;;
    --cilium-source) CILIUM_SOURCE="$2"; shift 2 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$CILIUM_SOURCE" ]]; then
  if [[ -d "$REPO_ROOT/../cilium/.git" ]]; then
    CILIUM_SOURCE="$REPO_ROOT/../cilium"
  elif [[ -d "/var/home/james/dev/cilium/.git" ]]; then
    CILIUM_SOURCE="/var/home/james/dev/cilium"
  fi
fi

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

ensure_helm_env() {
  export HELM_CACHE_HOME="$OUT_DIR/helm/cache"
  export HELM_CONFIG_HOME="$OUT_DIR/helm/config"
  export HELM_DATA_HOME="$OUT_DIR/helm/data"
  mkdir -p "$HELM_CACHE_HOME" "$HELM_CONFIG_HOME" "$HELM_DATA_HOME"
}

percent_delta() {
  python3 - "$1" "$2" <<'PY'
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

ratio_delta() {
  python3 - "$1" "$2" <<'PY'
import re, sys

def parse_to_ns(v: str):
    m = re.search(r'([0-9]+(?:\.[0-9]+)?)\s*(ns|us|µs|ms)?', v)
    if not m:
        return None
    value = float(m.group(1))
    unit = (m.group(2) or 'ns').replace('us', 'µs')
    scale = {'ns': 1.0, 'µs': 1_000.0, 'ms': 1_000_000.0}
    return value * scale[unit]

s = parse_to_ns(sys.argv[1])
c = parse_to_ns(sys.argv[2])
if s is None or c is None or c == 0:
    print("N/A")
else:
    print(f"{s / c:.2f}x")
PY
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
obj = json.loads(Path(sys.argv[1]).read_text())
print(obj["median"]["point_estimate"])
PY
)"
    format_ns "$ns"
  else
    echo "N/A"
  fi
}

extract_upstream_binary_size_kb() {
  local tmp cid size
  tmp="$(mktemp -d)"
  docker pull "$CILIUM_IMAGE" >/dev/null
  cid="$(docker create "$CILIUM_IMAGE")"
  docker cp "$cid":/usr/bin/cilium-agent "$tmp/cilium-agent"
  docker rm "$cid" >/dev/null
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
  kubectl patch deployment metrics-server -n kube-system --type=json \
    -p='[{"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--kubelet-insecure-tls"}]' >/dev/null || true
  kubectl rollout status deployment/metrics-server -n kube-system --timeout=5m >/dev/null || true
}

parse_go_bench_result() {
  python3 - "$1" <<'PY'
import re, sys
text = sys.argv[1]
match = re.search(r'([0-9]+(?:\.[0-9]+)?)\s+(ns|us|µs|ms)/op', text)
if not match:
    print("N/A")
else:
    value = float(match.group(1))
    unit = match.group(2).replace('us', 'µs')
    scale = {'ns': 1.0, 'µs': 1_000.0, 'ms': 1_000_000.0}
    ns = value * scale[unit]
    if ns >= 1_000_000:
        print(f"{ns / 1_000_000:.2f} ms")
    elif ns >= 1_000:
        print(f"{ns / 1_000:.2f} µs")
    else:
        print(f"{ns:.2f} ns")
PY
}

run_go_benchmark() {
  local pkg="$1" bench_re="$2"
  if [[ -z "$CILIUM_SOURCE" || ! -d "$CILIUM_SOURCE" || ! -f "$CILIUM_SOURCE/go.mod" || ! $(command -v go) ]]; then
    echo "N/A"
    return
  fi
  local output
  output="$(cd "$CILIUM_SOURCE" && go test "$pkg" -run '^$' -bench "$bench_re" -benchmem -count=1 2>&1)"
  {
    echo "### $pkg :: $bench_re"
    echo "$output"
    echo
  } >> "$OUT_DIR/cilium-go-bench.txt"
  parse_go_bench_result "$output"
}

run_seriousum_benches() {
  info "Running Seriousum Criterion micro-benchmarks..."
  rm -rf "$REPO_ROOT/target/criterion"
  : > "$OUT_DIR/criterion-raw.txt"
  cargo build --profile bench --benches >/dev/null
  for bench_name in load_balancer policy_eval ipam fqdn; do
    local bench_bin
    bench_bin="$(find "$REPO_ROOT/target/release/deps" -maxdepth 1 -type f -name "${bench_name}-*" ! -name '*.d' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)"
    if [[ -n "$bench_bin" ]]; then
      "$bench_bin" --bench >> "$OUT_DIR/criterion-raw.txt" 2>&1
    fi
  done
}

run_system_benchmarks() {
  SERIOUSUM_STARTUP_S="N/A"
  CILIUM_STARTUP_S="N/A"
  SERIOUSUM_RSS_MB="N/A"
  CILIUM_RSS_MB="N/A"
  SERIOUSUM_CPU_MCORES="N/A"
  CILIUM_CPU_MCORES="N/A"
  SYSTEM_STATUS="pending-kind-capable-runner"

  if [[ "$SKIP_KIND" == "true" ]]; then
    warn "Skipping kind benchmarks (--skip-kind)"
    return
  fi
  if ! command -v kind >/dev/null 2>&1 || ! command -v kubectl >/dev/null 2>&1 || ! command -v helm >/dev/null 2>&1; then
    warn "Skipping kind benchmarks: kind/kubectl/helm missing"
    return
  fi

  ensure_helm_env
  helm repo add cilium https://helm.cilium.io/ >/dev/null 2>&1 || true
  helm repo update cilium >/dev/null 2>&1 || true

  info "Creating kind cluster '$CLUSTER_NAME'..."
  if ! kind create cluster --name "$CLUSTER_NAME" --image kindest/node:v1.33.1 --wait 90s >/dev/null; then
    warn "Kind cluster creation failed; leaving system metrics pending"
    return
  fi

  SYSTEM_STATUS="measured"
  export KUBECONFIG
  KUBECONFIG="$(kind get kubeconfig --name "$CLUSTER_NAME")"
  install_metrics_server

  local image_tag="seriousum-agent:bench"
  info "Building Seriousum benchmark image..."
  docker build -f "$REPO_ROOT/images/agent.Dockerfile" -t "$image_tag" "$REPO_ROOT" >/dev/null
  kind load docker-image "$image_tag" --name "$CLUSTER_NAME"

  info "Measuring Seriousum startup..."
  local t0 t1
  t0=$(date +%s%3N)
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
  t1=$(date +%s%3N)
  SERIOUSUM_STARTUP_S="$(python3 - <<PY
print(f"{(${t1}-${t0})/1000:.1f}")
PY
)"
  sleep 60
  SERIOUSUM_RSS_MB="$(sample_top_avg kube-system 'k8s-app=cilium' 3)"
  SERIOUSUM_CPU_MCORES="$(sample_top_avg kube-system 'k8s-app=cilium' 2)"

  helm uninstall cilium -n kube-system >/dev/null || true
  kubectl wait --for=delete pod -n kube-system -l k8s-app=cilium --timeout=5m >/dev/null 2>&1 || true
  sleep 20

  info "Measuring upstream Cilium startup..."
  t0=$(date +%s%3N)
  helm install cilium cilium/cilium \
    --namespace kube-system \
    --set ipam.mode=kubernetes \
    --set kubeProxyReplacement=false \
    --wait --timeout 10m >/dev/null
  t1=$(date +%s%3N)
  CILIUM_STARTUP_S="$(python3 - <<PY
print(f"{(${t1}-${t0})/1000:.1f}")
PY
)"
  sleep 60
  CILIUM_RSS_MB="$(sample_top_avg kube-system 'k8s-app=cilium' 3)"
  CILIUM_CPU_MCORES="$(sample_top_avg kube-system 'k8s-app=cilium' 2)"

  helm uninstall cilium -n kube-system >/dev/null || true
  kubectl wait --for=delete pod -n kube-system -l k8s-app=cilium --timeout=5m >/dev/null 2>&1 || true
}

# 1. Binary size
info "Building Seriousum release binaries..."
cd "$REPO_ROOT"
cargo build --release --locked -q
SERIOUSUM_BIN_KB=$(( $(stat -c%s target/release/seriousum-daemon) / 1024 ))
CILIUM_BIN_KB="$(extract_upstream_binary_size_kb)"
success "Binary sizes: seriousum-agent=${SERIOUSUM_BIN_KB} KB upstream-cilium-agent=${CILIUM_BIN_KB} KB"

# 2. Optional system benchmarks
run_system_benchmarks

# 3. Seriousum micro-benchmarks
run_seriousum_benches
SER_LB_RR_8="$(parse_estimate "$REPO_ROOT/target/criterion/lb_round_robin/backends/8/new/estimates.json")"
SER_LB_CH_8="$(parse_estimate "$REPO_ROOT/target/criterion/lb_consistent_hash/backends/8/new/estimates.json")"
SER_POLICY_1="$(parse_estimate "$REPO_ROOT/target/criterion/policy_eval/policies/1/new/estimates.json")"
SER_POLICY_100="$(parse_estimate "$REPO_ROOT/target/criterion/policy_eval/policies/100/new/estimates.json")"
SER_POLICY_NO_MATCH_1000="$(parse_estimate "$REPO_ROOT/target/criterion/policy_eval_no_match_1000/new/estimates.json")"
SER_SELECTOR_HIT="$(parse_estimate "$REPO_ROOT/target/criterion/selector_match/match_hit/new/estimates.json")"
SER_SELECTOR_MISS="$(parse_estimate "$REPO_ROOT/target/criterion/selector_match/match_miss/new/estimates.json")"
SER_IPAM_1000="$(parse_estimate "$REPO_ROOT/target/criterion/ipam_alloc_release_1000/new/estimates.json")"
SER_IPAM_WARM="$(parse_estimate "$REPO_ROOT/target/criterion/ipam_allocate_warm_pool/new/estimates.json")"
SER_MAGLEV_BUILD="$(parse_estimate "$REPO_ROOT/target/criterion/lb_maglev_build_1000/new/estimates.json")"
SER_SERVICE_NAME_NEW="$(parse_estimate "$REPO_ROOT/target/criterion/lb_service_name_new/new/estimates.json")"
SER_SERVICE_NAME_DISPLAY="$(parse_estimate "$REPO_ROOT/target/criterion/lb_service_name_display/new/estimates.json")"
SER_L3N4ADDR_DISPLAY_IPV4="$(parse_estimate "$REPO_ROOT/target/criterion/lb_l3n4addr_display_ipv4/new/estimates.json")"
SER_LB_UPSERT_1="$(parse_estimate "$REPO_ROOT/target/criterion/lb_upsert_service_1/new/estimates.json")"
SER_LB_UPSERT_100="$(parse_estimate "$REPO_ROOT/target/criterion/lb_upsert_service_100/new/estimates.json")"
SER_LB_UPDATE_BACKENDS_100="$(parse_estimate "$REPO_ROOT/target/criterion/lb_update_backends_100/new/estimates.json")"
SER_FQDN_LOOKUP="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_lookup/new/estimates.json")"
SER_FQDN_UPDATE="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_update/new/estimates.json")"
SER_FQDN_SELECTOR_STRING="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_selector_string/new/estimates.json")"
SER_FQDN_JSON_MARSHAL_100="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_json_marshal_100/new/estimates.json")"
SER_FQDN_JSON_UNMARSHAL_100="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_json_unmarshal_100/new/estimates.json")"
SER_FQDN_JSON_MARSHAL_1000="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_json_marshal_1000/new/estimates.json")"
SER_FQDN_JSON_UNMARSHAL_1000="$(parse_estimate "$REPO_ROOT/target/criterion/fqdn_json_unmarshal_1000/new/estimates.json")"

# 4. Upstream Cilium Go micro-benchmarks
: > "$OUT_DIR/cilium-go-bench.txt"
if [[ -n "$CILIUM_SOURCE" && -d "$CILIUM_SOURCE" ]]; then
  info "Running upstream Cilium Go benchmarks from $CILIUM_SOURCE..."
else
  warn "No local Cilium source checkout found; upstream Go micro-benchmarks will be N/A"
fi
CIL_SELECTOR_VALID_1000="$(run_go_benchmark ./pkg/policy/types '^BenchmarkMatchesValid1000$')"
CIL_SELECTOR_INVALID_1000="$(run_go_benchmark ./pkg/policy/types '^BenchmarkMatchesInvalid1000$')"
CIL_POLICY_RESOLVE_NO_MATCH="$(run_go_benchmark ./pkg/policy '^BenchmarkResolveNoMatchingRules$')"
CIL_IPALLOC_ANY="$(run_go_benchmark ./pkg/ipalloc '^BenchmarkHashAlloc_AllocAny$')"
CIL_MAGLEV_TABLE="$(run_go_benchmark ./pkg/maglev '^BenchmarkGetMaglevTable/16381$')"
CIL_SERVICE_NAME_NEW="$(run_go_benchmark ./pkg/loadbalancer '^BenchmarkNewServiceName$')"
CIL_SERVICE_NAME_KEY="$(run_go_benchmark ./pkg/loadbalancer '^BenchmarkServiceNameKey$')"
CIL_L3N4ADDR_STRING_IPV4="$(run_go_benchmark ./pkg/loadbalancer '^BenchmarkL3n4Addr_StringWithProtocol_IPv4$')"
CIL_LB_UPSERT_1="$(run_go_benchmark ./pkg/loadbalancer/writer '^Benchmark_UpsertServiceAndFrontends_1$')"
CIL_LB_UPSERT_100="$(run_go_benchmark ./pkg/loadbalancer/writer '^Benchmark_UpsertServiceAndFrontends_100$')"
CIL_FQDN_GET_IPS="$(run_go_benchmark ./pkg/fqdn '^BenchmarkGetIPs$')"
CIL_FQDN_UPDATE_IPS="$(run_go_benchmark ./pkg/fqdn '^BenchmarkUpdateIPs$')"
CIL_FQDN_SELECTOR_STRING="$(run_go_benchmark ./pkg/policy/api '^BenchmarkFQDNSelectorString$')"
CIL_FQDN_JSON_MARSHAL_100="$(run_go_benchmark ./pkg/fqdn '^BenchmarkMarshalJSON100$')"
CIL_FQDN_JSON_UNMARSHAL_100="$(run_go_benchmark ./pkg/fqdn '^BenchmarkUnmarshalJSON100$')"
CIL_FQDN_JSON_MARSHAL_1000="$(run_go_benchmark ./pkg/fqdn '^BenchmarkMarshalJSON1000$')"
CIL_FQDN_JSON_UNMARSHAL_1000="$(run_go_benchmark ./pkg/fqdn '^BenchmarkUnmarshalJSON1000$')"

TIMESTAMP="$(date -u +"%Y-%m-%d %H:%M UTC")"
GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"

BIN_DELTA="$(percent_delta "$SERIOUSUM_BIN_KB" "$CILIUM_BIN_KB")"
STARTUP_DELTA="$(percent_delta "$SERIOUSUM_STARTUP_S" "$CILIUM_STARTUP_S")"
RSS_DELTA="$(percent_delta "$SERIOUSUM_RSS_MB" "$CILIUM_RSS_MB")"
CPU_DELTA="$(percent_delta "$SERIOUSUM_CPU_MCORES" "$CILIUM_CPU_MCORES")"
SELECTOR_HIT_RATIO="$(ratio_delta "$SER_SELECTOR_HIT" "$CIL_SELECTOR_VALID_1000")"
SELECTOR_MISS_RATIO="$(ratio_delta "$SER_SELECTOR_MISS" "$CIL_SELECTOR_INVALID_1000")"
POLICY_NO_MATCH_RATIO="$(ratio_delta "$SER_POLICY_NO_MATCH_1000" "$CIL_POLICY_RESOLVE_NO_MATCH")"
IPAM_RATIO="$(ratio_delta "$SER_IPAM_WARM" "$CIL_IPALLOC_ANY")"
MAGLEV_RATIO="$(ratio_delta "$SER_MAGLEV_BUILD" "$CIL_MAGLEV_TABLE")"
SERVICE_NAME_NEW_RATIO="$(ratio_delta "$SER_SERVICE_NAME_NEW" "$CIL_SERVICE_NAME_NEW")"
SERVICE_NAME_DISPLAY_RATIO="$(ratio_delta "$SER_SERVICE_NAME_DISPLAY" "$CIL_SERVICE_NAME_KEY")"
L3N4ADDR_DISPLAY_RATIO="$(ratio_delta "$SER_L3N4ADDR_DISPLAY_IPV4" "$CIL_L3N4ADDR_STRING_IPV4")"
LB_UPSERT_1_RATIO="$(ratio_delta "$SER_LB_UPSERT_1" "$CIL_LB_UPSERT_1")"
LB_UPSERT_100_RATIO="$(ratio_delta "$SER_LB_UPSERT_100" "$CIL_LB_UPSERT_100")"
FQDN_LOOKUP_RATIO="$(ratio_delta "$SER_FQDN_LOOKUP" "$CIL_FQDN_GET_IPS")"
FQDN_UPDATE_RATIO="$(ratio_delta "$SER_FQDN_UPDATE" "$CIL_FQDN_UPDATE_IPS")"
FQDN_SELECTOR_STRING_RATIO="$(ratio_delta "$SER_FQDN_SELECTOR_STRING" "$CIL_FQDN_SELECTOR_STRING")"
FQDN_JSON_MARSHAL_100_RATIO="$(ratio_delta "$SER_FQDN_JSON_MARSHAL_100" "$CIL_FQDN_JSON_MARSHAL_100")"
FQDN_JSON_UNMARSHAL_100_RATIO="$(ratio_delta "$SER_FQDN_JSON_UNMARSHAL_100" "$CIL_FQDN_JSON_UNMARSHAL_100")"
FQDN_JSON_MARSHAL_1000_RATIO="$(ratio_delta "$SER_FQDN_JSON_MARSHAL_1000" "$CIL_FQDN_JSON_MARSHAL_1000")"
FQDN_JSON_UNMARSHAL_1000_RATIO="$(ratio_delta "$SER_FQDN_JSON_UNMARSHAL_1000" "$CIL_FQDN_JSON_UNMARSHAL_1000")"

cat > "$OUT_DIR/results.json" <<EOF
{
  "timestamp": "$TIMESTAMP",
  "git_sha": "$GIT_SHA",
  "cilium_image": "$CILIUM_IMAGE",
  "cilium_source": "$CILIUM_SOURCE",
  "system_status": "$SYSTEM_STATUS",
  "comparison": {
    "agent_binary_size_kb": {"seriousum": "$SERIOUSUM_BIN_KB", "cilium": "$CILIUM_BIN_KB", "delta": "$BIN_DELTA"},
    "selector_match_hit": {"seriousum": "$SER_SELECTOR_HIT", "cilium": "$CIL_SELECTOR_VALID_1000", "ratio": "$SELECTOR_HIT_RATIO"},
    "selector_match_miss": {"seriousum": "$SER_SELECTOR_MISS", "cilium": "$CIL_SELECTOR_INVALID_1000", "ratio": "$SELECTOR_MISS_RATIO"},
    "policy_resolve_no_match": {"seriousum": "$SER_POLICY_NO_MATCH_1000", "cilium": "$CIL_POLICY_RESOLVE_NO_MATCH", "ratio": "$POLICY_NO_MATCH_RATIO"},
    "ip_allocator_hot_path": {"seriousum": "$SER_IPAM_WARM", "cilium": "$CIL_IPALLOC_ANY", "ratio": "$IPAM_RATIO"},
    "maglev_table_build": {"seriousum": "$SER_MAGLEV_BUILD", "cilium": "$CIL_MAGLEV_TABLE", "ratio": "$MAGLEV_RATIO"},
    "service_name_new": {"seriousum": "$SER_SERVICE_NAME_NEW", "cilium": "$CIL_SERVICE_NAME_NEW", "ratio": "$SERVICE_NAME_NEW_RATIO"},
    "service_name_string": {"seriousum": "$SER_SERVICE_NAME_DISPLAY", "cilium": "$CIL_SERVICE_NAME_KEY", "ratio": "$SERVICE_NAME_DISPLAY_RATIO"},
    "l3n4addr_string_ipv4": {"seriousum": "$SER_L3N4ADDR_DISPLAY_IPV4", "cilium": "$CIL_L3N4ADDR_STRING_IPV4", "ratio": "$L3N4ADDR_DISPLAY_RATIO"},
    "loadbalancer_upsert_1": {"seriousum": "$SER_LB_UPSERT_1", "cilium": "$CIL_LB_UPSERT_1", "ratio": "$LB_UPSERT_1_RATIO"},
    "loadbalancer_upsert_100": {"seriousum": "$SER_LB_UPSERT_100", "cilium": "$CIL_LB_UPSERT_100", "ratio": "$LB_UPSERT_100_RATIO"},
    "fqdn_lookup": {"seriousum": "$SER_FQDN_LOOKUP", "cilium": "$CIL_FQDN_GET_IPS", "ratio": "$FQDN_LOOKUP_RATIO"},
    "fqdn_update": {"seriousum": "$SER_FQDN_UPDATE", "cilium": "$CIL_FQDN_UPDATE_IPS", "ratio": "$FQDN_UPDATE_RATIO"},
    "fqdn_selector_string": {"seriousum": "$SER_FQDN_SELECTOR_STRING", "cilium": "$CIL_FQDN_SELECTOR_STRING", "ratio": "$FQDN_SELECTOR_STRING_RATIO"},
    "fqdn_json_marshal_100": {"seriousum": "$SER_FQDN_JSON_MARSHAL_100", "cilium": "$CIL_FQDN_JSON_MARSHAL_100", "ratio": "$FQDN_JSON_MARSHAL_100_RATIO"},
    "fqdn_json_unmarshal_100": {"seriousum": "$SER_FQDN_JSON_UNMARSHAL_100", "cilium": "$CIL_FQDN_JSON_UNMARSHAL_100", "ratio": "$FQDN_JSON_UNMARSHAL_100_RATIO"},
    "fqdn_json_marshal_1000": {"seriousum": "$SER_FQDN_JSON_MARSHAL_1000", "cilium": "$CIL_FQDN_JSON_MARSHAL_1000", "ratio": "$FQDN_JSON_MARSHAL_1000_RATIO"},
    "fqdn_json_unmarshal_1000": {"seriousum": "$SER_FQDN_JSON_UNMARSHAL_1000", "cilium": "$CIL_FQDN_JSON_UNMARSHAL_1000", "ratio": "$FQDN_JSON_UNMARSHAL_1000_RATIO"}
  },
  "system": {
    "startup_seconds": {"seriousum": "$SERIOUSUM_STARTUP_S", "cilium": "$CILIUM_STARTUP_S", "delta": "$STARTUP_DELTA"},
    "idle_memory_mib": {"seriousum": "$SERIOUSUM_RSS_MB", "cilium": "$CILIUM_RSS_MB", "delta": "$RSS_DELTA"},
    "idle_cpu_mcores": {"seriousum": "$SERIOUSUM_CPU_MCORES", "cilium": "$CILIUM_CPU_MCORES", "delta": "$CPU_DELTA"}
  },
  "microbench_seriousum": {
    "lb_round_robin_8_backends": "$SER_LB_RR_8",
    "lb_consistent_hash_8_backends": "$SER_LB_CH_8",
    "policy_eval_1_policy": "$SER_POLICY_1",
    "policy_eval_100_policies": "$SER_POLICY_100",
    "policy_eval_no_match_1000": "$SER_POLICY_NO_MATCH_1000",
    "selector_match_hit": "$SER_SELECTOR_HIT",
    "selector_match_miss": "$SER_SELECTOR_MISS",
    "ipam_allocate_warm_pool": "$SER_IPAM_WARM",
    "ipam_alloc_release_1000": "$SER_IPAM_1000",
    "maglev_table_build_1000": "$SER_MAGLEV_BUILD",
    "service_name_new": "$SER_SERVICE_NAME_NEW",
    "service_name_display": "$SER_SERVICE_NAME_DISPLAY",
    "l3n4addr_display_ipv4": "$SER_L3N4ADDR_DISPLAY_IPV4",
    "loadbalancer_upsert_1": "$SER_LB_UPSERT_1",
    "loadbalancer_upsert_100": "$SER_LB_UPSERT_100",
    "loadbalancer_update_backends_100": "$SER_LB_UPDATE_BACKENDS_100",
    "fqdn_lookup": "$SER_FQDN_LOOKUP",
    "fqdn_update": "$SER_FQDN_UPDATE",
    "fqdn_selector_string": "$SER_FQDN_SELECTOR_STRING",
    "fqdn_json_marshal_100": "$SER_FQDN_JSON_MARSHAL_100",
    "fqdn_json_unmarshal_100": "$SER_FQDN_JSON_UNMARSHAL_100",
    "fqdn_json_marshal_1000": "$SER_FQDN_JSON_MARSHAL_1000",
    "fqdn_json_unmarshal_1000": "$SER_FQDN_JSON_UNMARSHAL_1000"
  },
  "microbench_cilium_go": {
    "selector_matches_valid_1000": "$CIL_SELECTOR_VALID_1000",
    "selector_matches_invalid_1000": "$CIL_SELECTOR_INVALID_1000",
    "policy_resolve_no_matching_rules": "$CIL_POLICY_RESOLVE_NO_MATCH",
    "ipalloc_alloc_any": "$CIL_IPALLOC_ANY",
    "maglev_get_lookup_table_16381": "$CIL_MAGLEV_TABLE",
    "service_name_new": "$CIL_SERVICE_NAME_NEW",
    "service_name_key": "$CIL_SERVICE_NAME_KEY",
    "l3n4addr_string_ipv4": "$CIL_L3N4ADDR_STRING_IPV4",
    "loadbalancer_upsert_service_and_frontends_1": "$CIL_LB_UPSERT_1",
    "loadbalancer_upsert_service_and_frontends_100": "$CIL_LB_UPSERT_100",
    "fqdn_get_ips": "$CIL_FQDN_GET_IPS",
    "fqdn_update_ips": "$CIL_FQDN_UPDATE_IPS",
    "fqdn_selector_string": "$CIL_FQDN_SELECTOR_STRING",
    "fqdn_json_marshal_100": "$CIL_FQDN_JSON_MARSHAL_100",
    "fqdn_json_unmarshal_100": "$CIL_FQDN_JSON_UNMARSHAL_100",
    "fqdn_json_marshal_1000": "$CIL_FQDN_JSON_MARSHAL_1000",
    "fqdn_json_unmarshal_1000": "$CIL_FQDN_JSON_UNMARSHAL_1000"
  }
}
EOF
cp "$OUT_DIR/results.json" "$PUBLISH_DIR/benchmark-results.json"

cat > "$PUBLISH_DIR/BENCHMARKS.md" <<EOF
# Benchmark Comparison: Seriousum vs Cilium

_Last updated: $TIMESTAMP · commit \`$GIT_SHA\`_

This report publishes the current benchmark comparison between **Seriousum** and **upstream Cilium**.

## Comparison categories

- **Direct-ish**: same or very similar operation shape between Seriousum and upstream Cilium.
- **Approximate**: useful directional comparison, but underlying implementation or data model differs.
- **Pending**: reserved for system-level kind/Helm comparison on capable runners.

## Direct-ish comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **${SERIOUSUM_BIN_KB} KB** | ${CILIUM_BIN_KB} KB | ${BIN_DELTA} |
| Selector match hit | **${SER_SELECTOR_HIT}** | ${CIL_SELECTOR_VALID_1000} | ${SELECTOR_HIT_RATIO} |
| Selector match miss | **${SER_SELECTOR_MISS}** | ${CIL_SELECTOR_INVALID_1000} | ${SELECTOR_MISS_RATIO} |
| Policy resolve no-match | **${SER_POLICY_NO_MATCH_1000}** | ${CIL_POLICY_RESOLVE_NO_MATCH} | ${POLICY_NO_MATCH_RATIO} |
| IP allocator hot path | **${SER_IPAM_WARM}** | ${CIL_IPALLOC_ANY} | ${IPAM_RATIO} |
| ServiceName construction | **${SER_SERVICE_NAME_NEW}** | ${CIL_SERVICE_NAME_NEW} | ${SERVICE_NAME_NEW_RATIO} |
| L3n4Addr IPv4 string+protocol | **${SER_L3N4ADDR_DISPLAY_IPV4}** | ${CIL_L3N4ADDR_STRING_IPV4} | ${L3N4ADDR_DISPLAY_RATIO} |
| FQDN lookup | **${SER_FQDN_LOOKUP}** | ${CIL_FQDN_GET_IPS} | ${FQDN_LOOKUP_RATIO} |
| FQDN update | **${SER_FQDN_UPDATE}** | ${CIL_FQDN_UPDATE_IPS} | ${FQDN_UPDATE_RATIO} |

## Approximate comparisons

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Consistent-hash table build | **${SER_MAGLEV_BUILD}** | ${CIL_MAGLEV_TABLE} | ${MAGLEV_RATIO} |
| ServiceName string/key | **${SER_SERVICE_NAME_DISPLAY}** | ${CIL_SERVICE_NAME_KEY} | ${SERVICE_NAME_DISPLAY_RATIO} |
| Load balancer upsert 1 | **${SER_LB_UPSERT_1}** | ${CIL_LB_UPSERT_1} | ${LB_UPSERT_1_RATIO} |
| Load balancer upsert 100 | **${SER_LB_UPSERT_100}** | ${CIL_LB_UPSERT_100} | ${LB_UPSERT_100_RATIO} |
| FQDN selector string | **${SER_FQDN_SELECTOR_STRING}** | ${CIL_FQDN_SELECTOR_STRING} | ${FQDN_SELECTOR_STRING_RATIO} |
| FQDN JSON marshal 100 | **${SER_FQDN_JSON_MARSHAL_100}** | ${CIL_FQDN_JSON_MARSHAL_100} | ${FQDN_JSON_MARSHAL_100_RATIO} |
| FQDN JSON unmarshal 100 | **${SER_FQDN_JSON_UNMARSHAL_100}** | ${CIL_FQDN_JSON_UNMARSHAL_100} | ${FQDN_JSON_UNMARSHAL_100_RATIO} |
| FQDN JSON marshal 1000 | **${SER_FQDN_JSON_MARSHAL_1000}** | ${CIL_FQDN_JSON_MARSHAL_1000} | ${FQDN_JSON_MARSHAL_1000_RATIO} |
| FQDN JSON unmarshal 1000 | **${SER_FQDN_JSON_UNMARSHAL_1000}** | ${CIL_FQDN_JSON_UNMARSHAL_1000} | ${FQDN_JSON_UNMARSHAL_1000_RATIO} |

### Benchmark mapping
- Seriousum selector hit/miss: 'selector_match/match_hit' + 'selector_match/match_miss'
- Cilium selector hit/miss: 'pkg/policy/types BenchmarkMatchesValid1000' + 'BenchmarkMatchesInvalid1000'
- Seriousum allocator: 'ipam_allocate_warm_pool'
- Cilium allocator: 'pkg/ipalloc BenchmarkHashAlloc_AllocAny'
- Seriousum hash-table build: 'lb_maglev_build_1000'
- Cilium hash-table build: 'pkg/maglev BenchmarkGetMaglevTable/16381'
- Seriousum ServiceName ops: 'lb_service_name_new' + 'lb_service_name_display'
- Cilium ServiceName ops: 'BenchmarkNewServiceName' + 'BenchmarkServiceNameKey'
- Seriousum FQDN ops: 'fqdn_lookup' + 'fqdn_update'
- Cilium FQDN ops: 'BenchmarkGetIPs' + 'BenchmarkUpdateIPs'
- Seriousum FQDN JSON ops: 'fqdn_json_marshal_100' + 'fqdn_json_unmarshal_100' + 'fqdn_json_marshal_1000' + 'fqdn_json_unmarshal_1000'
- Cilium FQDN JSON ops: 'BenchmarkMarshalJSON100' + 'BenchmarkUnmarshalJSON100' + 'BenchmarkMarshalJSON1000' + 'BenchmarkUnmarshalJSON1000'
- Seriousum LB batch ops: 'lb_upsert_service_1' + 'lb_upsert_service_100'
- Cilium LB batch ops: 'Benchmark_UpsertServiceAndFrontends_1' + 'Benchmark_UpsertServiceAndFrontends_100'

## System metrics

| Metric | Seriousum | Cilium | Delta vs Cilium |
|---|---:|---:|---:|
| Startup time | **${SERIOUSUM_STARTUP_S} s** | ${CILIUM_STARTUP_S} s | ${STARTUP_DELTA} |
| Idle memory (RSS / pod) | **${SERIOUSUM_RSS_MB} MiB** | ${CILIUM_RSS_MB} MiB | ${RSS_DELTA} |
| Idle CPU | **${SERIOUSUM_CPU_MCORES} m** | ${CILIUM_CPU_MCORES} m | ${CPU_DELTA} |

System metric status: **${SYSTEM_STATUS}**

## Seriousum micro-benchmarks

| Benchmark | Median |
|---|---:|
| Load balancer round-robin (8 backends) | ${SER_LB_RR_8} |
| Load balancer consistent hash select (8 backends) | ${SER_LB_CH_8} |
| Policy evaluation (1 policy) | ${SER_POLICY_1} |
| Policy evaluation (100 policies) | ${SER_POLICY_100} |
| Policy evaluation no match 1000 | ${SER_POLICY_NO_MATCH_1000} |
| Selector match (hit) | ${SER_SELECTOR_HIT} |
| Selector match (miss) | ${SER_SELECTOR_MISS} |
| IPAM allocate warm pool | ${SER_IPAM_WARM} |
| IPAM allocate + release ×1000 | ${SER_IPAM_1000} |
| Maglev table build (1000 backends) | ${SER_MAGLEV_BUILD} |
| ServiceName construction | ${SER_SERVICE_NAME_NEW} |
| ServiceName display | ${SER_SERVICE_NAME_DISPLAY} |
| L3n4Addr IPv4 display | ${SER_L3N4ADDR_DISPLAY_IPV4} |
| Load balancer upsert 1 | ${SER_LB_UPSERT_1} |
| Load balancer upsert 100 | ${SER_LB_UPSERT_100} |
| Load balancer update backends 100 | ${SER_LB_UPDATE_BACKENDS_100} |
| FQDN lookup | ${SER_FQDN_LOOKUP} |
| FQDN update | ${SER_FQDN_UPDATE} |
| FQDN selector string | ${SER_FQDN_SELECTOR_STRING} |
| FQDN JSON marshal 100 | ${SER_FQDN_JSON_MARSHAL_100} |
| FQDN JSON unmarshal 100 | ${SER_FQDN_JSON_UNMARSHAL_100} |
| FQDN JSON marshal 1000 | ${SER_FQDN_JSON_MARSHAL_1000} |
| FQDN JSON unmarshal 1000 | ${SER_FQDN_JSON_UNMARSHAL_1000} |

## Upstream Cilium Go micro-benchmarks

| Benchmark | Result |
|---|---:|
| Selector match valid 1000 | ${CIL_SELECTOR_VALID_1000} |
| Selector match invalid 1000 | ${CIL_SELECTOR_INVALID_1000} |
| Policy resolve no matching rules | ${CIL_POLICY_RESOLVE_NO_MATCH} |
| Hash allocator alloc any | ${CIL_IPALLOC_ANY} |
| Maglev lookup table build 16381 | ${CIL_MAGLEV_TABLE} |
| ServiceName construction | ${CIL_SERVICE_NAME_NEW} |
| ServiceName key | ${CIL_SERVICE_NAME_KEY} |
| L3n4Addr IPv4 string+protocol | ${CIL_L3N4ADDR_STRING_IPV4} |
| Load balancer upsert service+frontends 1 | ${CIL_LB_UPSERT_1} |
| Load balancer upsert service+frontends 100 | ${CIL_LB_UPSERT_100} |
| FQDN get IPs | ${CIL_FQDN_GET_IPS} |
| FQDN update IPs | ${CIL_FQDN_UPDATE_IPS} |
| FQDN selector string | ${CIL_FQDN_SELECTOR_STRING} |
| FQDN JSON marshal 100 | ${CIL_FQDN_JSON_MARSHAL_100} |
| FQDN JSON unmarshal 100 | ${CIL_FQDN_JSON_UNMARSHAL_100} |
| FQDN JSON marshal 1000 | ${CIL_FQDN_JSON_MARSHAL_1000} |
| FQDN JSON unmarshal 1000 | ${CIL_FQDN_JSON_UNMARSHAL_1000} |

## Reproduce locally

~~~bash
# Publish micro-benchmarks only
./scripts/benchmark.sh --skip-kind --cilium-source /path/to/cilium

# Publish full report if your host can run kind
./scripts/benchmark.sh --cilium-source /path/to/cilium

# Inspect machine-readable results
cat docs/generated/benchmark-results.json
~~~

## Notes

- System-level Helm+kind metrics remain optional because not every runner can boot kind successfully.
- The selector comparison is the closest direct hot-path comparison currently in the report.
- The allocator and Maglev rows are useful directional comparisons, but implementation details differ between projects.
EOF

cat > "$OUT_DIR/readme-bench.md" <<EOF
<!-- BENCHMARK_START -->
## 📊 Benchmarks

> Last run: **$TIMESTAMP** · commit \`$GIT_SHA\`
> Published comparison report: [docs/generated/BENCHMARKS.md](docs/generated/BENCHMARKS.md)

| Metric | Seriousum | Cilium | Relative |
|---|---:|---:|---:|
| Agent binary size | **${SERIOUSUM_BIN_KB} KB** | ${CILIUM_BIN_KB} KB | ${BIN_DELTA} |
| Selector match hit | **${SER_SELECTOR_HIT}** | ${CIL_SELECTOR_VALID_1000} | ${SELECTOR_HIT_RATIO} |
| Selector match miss | **${SER_SELECTOR_MISS}** | ${CIL_SELECTOR_INVALID_1000} | ${SELECTOR_MISS_RATIO} |
| Policy resolve no-match | **${SER_POLICY_NO_MATCH_1000}** | ${CIL_POLICY_RESOLVE_NO_MATCH} | ${POLICY_NO_MATCH_RATIO} |
| IP allocator hot path | **${SER_IPAM_WARM}** | ${CIL_IPALLOC_ANY} | ${IPAM_RATIO} |
| ServiceName construction | **${SER_SERVICE_NAME_NEW}** | ${CIL_SERVICE_NAME_NEW} | ${SERVICE_NAME_NEW_RATIO} |
| FQDN lookup | **${SER_FQDN_LOOKUP}** | ${CIL_FQDN_GET_IPS} | ${FQDN_LOOKUP_RATIO} |
| FQDN JSON marshal 100 | **${SER_FQDN_JSON_MARSHAL_100}** | ${CIL_FQDN_JSON_MARSHAL_100} | ${FQDN_JSON_MARSHAL_100_RATIO} |

### Seriousum micro-benchmarks

| Benchmark | Median |
|---|---:|
| LB round-robin (8 backends) | ${SER_LB_RR_8} |
| LB consistent hash (8 backends) | ${SER_LB_CH_8} |
| Policy eval (1 policy) | ${SER_POLICY_1} |
| Policy eval (100 policies) | ${SER_POLICY_100} |
| Selector match (hit) | ${SER_SELECTOR_HIT} |
| Selector match (miss) | ${SER_SELECTOR_MISS} |
| IPAM alloc warm pool | ${SER_IPAM_WARM} |
| IPAM alloc + release ×1000 | ${SER_IPAM_1000} |
| ServiceName display | ${SER_SERVICE_NAME_DISPLAY} |
| Load balancer upsert 1 | ${SER_LB_UPSERT_1} |
| Load balancer upsert 100 | ${SER_LB_UPSERT_100} |
| FQDN update | ${SER_FQDN_UPDATE} |
| FQDN selector string | ${SER_FQDN_SELECTOR_STRING} |

> System startup / memory / CPU status: **${SYSTEM_STATUS}**

<details>
<summary>Reproduce locally</summary>

~~~bash
./scripts/benchmark.sh --skip-kind --cilium-source /path/to/cilium
~~~

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
echo "  - $OUT_DIR/cilium-go-bench.txt"
echo "  - $PUBLISH_DIR/benchmark-results.json"
echo "  - $PUBLISH_DIR/BENCHMARKS.md"
echo "  - README.md benchmark section"
