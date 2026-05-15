#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
cd "$ROOT_DIR"

BUILD_SCRIPT="$ROOT_DIR/images/build-cilium-images.sh"
DROPIN_SCRIPT="$ROOT_DIR/scripts/build-cilium-dropin.sh"
PREPARE_KUBECTL_SCRIPT="$ROOT_DIR/scripts/prepare-cilium-kubectl.sh"
CILIUM_REPO=${CILIUM_REPO:-/var/home/james/dev/cilium}
IMAGE_PREFIX=${IMAGE_PREFIX:-localhost:5000/seriousum}
IMAGE_TAG=${IMAGE_TAG:-local}
AGENT_IMAGE_REPO=${AGENT_IMAGE_REPO:-$IMAGE_PREFIX/cilium-agent}
AGENT_IMAGE_TAG=${AGENT_IMAGE_TAG:-$IMAGE_TAG}
BIN_DIR=${BIN_DIR:-$ROOT_DIR/target/cilium-dropin}
KIND_CLUSTER=${KIND_CLUSTER:-kind}
FOCUS=${FOCUS:-Cilium}
# Default to loading images into kind (required for local operator image)
LOAD_INTO_KIND=${LOAD_INTO_KIND:-1}
BUILD_IMAGES=${BUILD_IMAGES:-1}
INSTALL_DROPIN=${INSTALL_DROPIN:-1}
KIND_BOOTSTRAP=${KIND_BOOTSTRAP:-1}
KIND_RECREATE_CLUSTER=${KIND_RECREATE_CLUSTER:-1}
KIND_CONTROLPLANES=${KIND_CONTROLPLANES:-1}
KIND_WORKERS=${KIND_WORKERS:-1}
KUBECONFIG_FILE=${KUBECONFIG_FILE:-}
KUBECTL_PATH=${KUBECTL_PATH:-}
TEST_TIMEOUT=${TEST_TIMEOUT:-2h}
HOLD_ENVIRONMENT=${HOLD_ENVIRONMENT:-false}
PREINSTALL_CILIUM_CRDS=${PREINSTALL_CILIUM_CRDS:-1}

usage() {
  cat <<'EOF'
Usage: scripts/run-cilium-kind-test.sh [options]

Build the Cilium-compatible Rust images, optionally load them into a kind
cluster, export the image overrides expected by the Cilium harness, and run a
focused ginkgo test.

Options:
  -f, --focus PATTERN        Focus pattern passed to ginkgo --focus ...
      --image-prefix PREFIX  Image prefix used by the build script
      --image-tag TAG        Image tag used by the build script
      --agent-image-repo REPO  Cilium agent image repository used for the live cluster
      --agent-image-tag TAG    Cilium agent image tag used for the live cluster
      --kind-cluster NAME    kind cluster name used for image loading
      --cilium-repo PATH   Path to the Cilium checkout that owns the test harness
      --load                 Load built images into the kind cluster
      --no-load              Do not load images into kind
      --skip-build           Reuse existing built images instead of rebuilding
      --skip-dropin          Reuse an existing host alias directory instead of reinstalling it
      --bootstrap-cluster     Create or recreate the kind cluster before running the harness
      --no-bootstrap-cluster  Skip kind cluster creation/recreation
      --kind-controlplanes N  Control-plane node count for bootstrap
      --kind-workers N        Worker node count for bootstrap
      --kubeconfig-file PATH  Kubeconfig path used by the harness
      --kubectl-path PATH     Base directory for version-specific kubectl shims
      --test-timeout DURATION Fail the test run after the given wall-clock duration
  -h, --help                 Show this help message

Environment variables:
  CILIUM_REPO, IMAGE_PREFIX, IMAGE_TAG, AGENT_IMAGE_REPO, AGENT_IMAGE_TAG, BIN_DIR, KIND_CLUSTER, FOCUS, LOAD_INTO_KIND, BUILD_IMAGES, INSTALL_DROPIN, KIND_BOOTSTRAP, KIND_RECREATE_CLUSTER, KIND_CONTROLPLANES, KIND_WORKERS, KUBECONFIG_FILE, KUBECTL_PATH, TEST_TIMEOUT, HOLD_ENVIRONMENT, PREINSTALL_CILIUM_CRDS
EOF
}

preinstall_cilium_crds() {
  if [ "$PREINSTALL_CILIUM_CRDS" != "1" ]; then
    return
  fi

  local crd_root="$CILIUM_REPO/pkg/k8s/apis/cilium.io/client/crds"
  if [ ! -d "$crd_root" ]; then
    printf 'missing Cilium CRD source directory: %s\n' "$crd_root" >&2
    exit 1
  fi

  printf '==> preinstalling Cilium CRDs from %s\n' "$crd_root"
  while IFS= read -r -d '' crd; do
    kubectl apply -f "$crd" >/dev/null
  done < <(find "$crd_root" -type f -name '*.yaml' -print0 | sort -z)
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -f|--focus)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      FOCUS=$2
      shift 2
      ;;
    --image-prefix)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      IMAGE_PREFIX=$2
      shift 2
      ;;
    --image-tag)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      IMAGE_TAG=$2
      shift 2
      ;;
    --agent-image-repo)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      AGENT_IMAGE_REPO=$2
      shift 2
      ;;
    --agent-image-tag)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      AGENT_IMAGE_TAG=$2
      shift 2
      ;;
    --kind-cluster)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      KIND_CLUSTER=$2
      shift 2
      ;;
    --cilium-repo)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      CILIUM_REPO=$2
      shift 2
      ;;
    --load)
      LOAD_INTO_KIND=1
      shift
      ;;
    --no-load)
      LOAD_INTO_KIND=0
      shift
      ;;
    --skip-build)
      BUILD_IMAGES=0
      shift
      ;;
    --skip-dropin)
      INSTALL_DROPIN=0
      shift
      ;;
    --bootstrap-cluster)
      KIND_BOOTSTRAP=1
      shift
      ;;
    --no-bootstrap-cluster)
      KIND_BOOTSTRAP=0
      shift
      ;;
    --kind-controlplanes)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      KIND_CONTROLPLANES=$2
      shift 2
      ;;
    --kind-workers)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      KIND_WORKERS=$2
      shift 2
      ;;
    --kubeconfig-file)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      KUBECONFIG_FILE=$2
      shift 2
      ;;
    --kubectl-path)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      KUBECTL_PATH=$2
      shift 2
      ;;
    --test-timeout)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      TEST_TIMEOUT=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$KUBECONFIG_FILE" ]; then
  KUBECONFIG_FILE="$ROOT_DIR/target/cilium-kind/$KIND_CLUSTER.kubeconfig"
fi

if [ -z "$KUBECTL_PATH" ]; then
  KUBECTL_PATH="$ROOT_DIR/target/cilium-kind/$KIND_CLUSTER.kubectl-cache"
fi

export IMAGE_PREFIX IMAGE_TAG
if [ "$BUILD_IMAGES" = "1" ]; then
  printf '==> building images with %s and %s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
  "$BUILD_SCRIPT"
fi

if [ "$INSTALL_DROPIN" = "1" ]; then
  printf '==> installing host aliases into %s\n' "$BIN_DIR"
  "$DROPIN_SCRIPT" "$BIN_DIR"
fi

if [ "$KIND_BOOTSTRAP" = "1" ]; then
  mkdir -p "$(dirname "$KUBECONFIG_FILE")"
  if [ "$KIND_RECREATE_CLUSTER" = "1" ]; then
    kind delete cluster --name "$KIND_CLUSTER" >/dev/null 2>&1 || true
  fi
  printf '==> bootstrapping kind cluster %s\n' "$KIND_CLUSTER"
  "$CILIUM_REPO/contrib/scripts/kind.sh" "$KIND_CONTROLPLANES" "$KIND_WORKERS" "$KIND_CLUSTER" "" "" "" "" "" "$KUBECONFIG_FILE"
fi

export KUBECONFIG="$KUBECONFIG_FILE"
export PATH="$BIN_DIR:$PATH"
mkdir -p "$KUBECTL_PATH"
K8S_VERSION=${K8S_VERSION:-$(kubectl version -o json | jq -r '.serverVersion | "\(.major).\(.minor)"')}
"$PREPARE_KUBECTL_SCRIPT" --kubectl-root "$KUBECTL_PATH" --k8s-version "$K8S_VERSION" >/dev/null
preinstall_cilium_crds

CILIUM_IMAGE="$AGENT_IMAGE_REPO"
CILIUM_TAG="$AGENT_IMAGE_TAG"
# Use local Rust operator image (fallback to upstream if needed)
# Use operator.image.override to bypass the helm template's cloud-suffix logic.
# The built image is tagged as operator-generic to match the binary name Helm invokes.
CILIUM_OPERATOR_IMAGE="${OPERATOR_IMAGE_REPO:-$IMAGE_PREFIX/operator-generic}"
CILIUM_OPERATOR_TAG="${OPERATOR_IMAGE_TAG:-$IMAGE_TAG}"
HUBBLE_RELAY_IMAGE="$IMAGE_PREFIX/hubble"
HUBBLE_RELAY_TAG="$IMAGE_TAG"
CLUSTERMESH_INSTALL_OVERRIDES="image.useDigest=false,image.pullPolicy=IfNotPresent,preflight.image.pullPolicy=IfNotPresent,operator.image.useDigest=false,hubble.relay.image.useDigest=false,clustermesh.apiserver.image.useDigest=false,clustermesh.apiserver.image.repository=$IMAGE_PREFIX/clustermesh-apiserver,clustermesh.apiserver.image.tag=$IMAGE_TAG,clustermesh.apiserver.image.pullPolicy=IfNotPresent,operator.image.pullPolicy=IfNotPresent,hubble.relay.image.pullPolicy=IfNotPresent,kubeProxyReplacement=false,operator.image.override=$CILIUM_OPERATOR_IMAGE:$CILIUM_OPERATOR_TAG"

export CILIUM_IMAGE
export CILIUM_TAG
export CILIUM_OPERATOR_IMAGE
export CILIUM_OPERATOR_TAG
export HUBBLE_RELAY_IMAGE
export HUBBLE_RELAY_TAG
export CLUSTERMESH_INSTALL_OVERRIDES

if [ "$LOAD_INTO_KIND" = "1" ] && [ "$KIND_BOOTSTRAP" = "1" ]; then
  printf '==> loading images into kind cluster %s\n' "$KIND_CLUSTER"
  kind load docker-image --name "$KIND_CLUSTER" "$CILIUM_IMAGE:$CILIUM_TAG" 2>/dev/null || true
  kind load docker-image --name "$KIND_CLUSTER" "$IMAGE_PREFIX/cilium-dbg:$IMAGE_TAG" 2>/dev/null || true
  kind load docker-image --name "$KIND_CLUSTER" "$CILIUM_OPERATOR_IMAGE:$CILIUM_OPERATOR_TAG" 2>/dev/null || true
  kind load docker-image --name "$KIND_CLUSTER" "$HUBBLE_RELAY_IMAGE:$HUBBLE_RELAY_TAG" 2>/dev/null || true
  kind load docker-image --name "$KIND_CLUSTER" "$IMAGE_PREFIX/clustermesh-apiserver:$IMAGE_TAG" 2>/dev/null || true
  printf 'Images loaded into kind cluster\n'
fi

cd "$CILIUM_REPO/test"
printf '==> running ginkgo --focus %s\n' "$FOCUS"
CNI_INTEGRATION=kind \
K8S_VERSION="$K8S_VERSION" \
NETNEXT="${NETNEXT:-false}" \
KUBEPROXY="${KUBEPROXY:-1}" \
NO_CILIUM_ON_NODES="${NO_CILIUM_ON_NODES:-}" \
INTEGRATION_TESTS=true \
timeout --preserve-status --kill-after=5m "$TEST_TIMEOUT" ginkgo --focus "$FOCUS" -v -- \
  -cilium.testScope=k8s \
  -cilium.kubeconfig="$KUBECONFIG_FILE" \
  -cilium.kubectl-path="$KUBECTL_PATH" \
  -cilium.passCLIEnvironment=true \
  -cilium.image="$CILIUM_IMAGE" \
  -cilium.tag="$CILIUM_TAG" \
  -cilium.operator-image="$CILIUM_OPERATOR_IMAGE" \
  -cilium.operator-tag="$CILIUM_OPERATOR_TAG" \
  -cilium.operator-suffix="" \
  -cilium.hubble-relay-image="$HUBBLE_RELAY_IMAGE" \
  -cilium.hubble-relay-tag="$HUBBLE_RELAY_TAG" \
  -cilium.install-helm-overrides="$CLUSTERMESH_INSTALL_OVERRIDES" \
  -cilium.holdEnvironment="$HOLD_ENVIRONMENT"
