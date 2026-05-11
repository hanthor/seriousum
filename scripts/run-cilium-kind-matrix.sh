#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
RUNNER="$ROOT_DIR/scripts/run-cilium-kind-test.sh"
MATRIX_DIR=${MATRIX_DIR:-$ROOT_DIR/target/cilium-kind-matrix}
IMAGE_PREFIX=${IMAGE_PREFIX:-localhost:5000/seriousum}
IMAGE_TAG=${IMAGE_TAG:-local}
CILIUM_REPO=${CILIUM_REPO:-/var/home/james/dev/cilium}
LOAD_FLAG=${LOAD_FLAG:---load}
HOLD_ENVIRONMENT=${HOLD_ENVIRONMENT:-false}
TEST_TIMEOUT=${TEST_TIMEOUT:-2h}
SHARED_BIN_DIR=${SHARED_BIN_DIR:-$ROOT_DIR/target/cilium-kind-matrix/bin}

usage() {
  cat <<'EOF'
Usage: scripts/run-cilium-kind-matrix.sh [options]

Launch multiple focused Cilium kind tests in parallel, each against its own
kind cluster and result directory.

Options:
  --cluster NAME:FOCUS     Add one cluster/focus pair (repeatable)
  --matrix LIST            Comma-separated list of NAME:FOCUS entries
  --cilium-repo PATH       Path to the Cilium checkout to run against
  --image-prefix PREFIX    Image prefix passed to the image build helpers
  --image-tag TAG          Image tag passed to the image build helpers
  --matrix-dir DIR         Output directory for logs and shared artifacts
  --load                   Load images into each kind cluster before the run
  --no-load                Skip kind image loading
  --test-timeout DURATION  Fail each test run after the given wall-clock duration
  -h, --help               Show this help message
EOF
}

clusters=()
foci=()

add_entry() {
  local entry=$1
  local name focus
  case "$entry" in
    *:*)
      name=${entry%%:*}
      focus=${entry#*:}
      ;;
    *)
      printf 'invalid cluster entry (expected NAME:FOCUS): %s\n' "$entry" >&2
      exit 2
      ;;
  esac

  if [ -z "$name" ] || [ -z "$focus" ]; then
    printf 'invalid cluster entry (expected NAME:FOCUS): %s\n' "$entry" >&2
    exit 2
  fi

  clusters+=("$name")
  foci+=("$focus")
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --cluster)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      add_entry "$2"
      shift 2
      ;;
    --matrix)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      IFS=',' read -r -a entries <<<"$2"
      for entry in "${entries[@]}"; do
        add_entry "$entry"
      done
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
    --matrix-dir)
      if [ "$#" -lt 2 ]; then
        printf 'missing value for %s\n' "$1" >&2
        exit 2
      fi
      MATRIX_DIR=$2
      shift 2
      ;;
    --load)
      LOAD_FLAG=--load
      shift
      ;;
    --no-load)
      LOAD_FLAG=--no-load
      shift
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

if [ "${#clusters[@]}" -eq 0 ]; then
  usage >&2
  exit 2
fi

mkdir -p "$MATRIX_DIR"

printf '==> prebuilding shared images and host aliases\n'
"$ROOT_DIR/images/build-cilium-images.sh"
"$ROOT_DIR/scripts/build-cilium-dropin.sh" "$SHARED_BIN_DIR"

pids=()
labels=()
logs=()

for index in "${!clusters[@]}"; do
  cluster=${clusters[$index]}
  focus=${foci[$index]}
  run_dir="$MATRIX_DIR/$cluster"
  bin_dir="$SHARED_BIN_DIR"
  kubeconfig_file="$run_dir/kubeconfig"
  log_file="$run_dir/run.log"

  mkdir -p "$run_dir"
  labels+=("$cluster")
  logs+=("$log_file")

  printf '==> starting %s (%s)\n' "$cluster" "$focus"
  (
    CILIUM_REPO="$CILIUM_REPO" \
    IMAGE_PREFIX="$IMAGE_PREFIX" \
    IMAGE_TAG="$IMAGE_TAG" \
    BIN_DIR="$bin_dir" \
    KUBECONFIG_FILE="$kubeconfig_file" \
    BUILD_IMAGES=0 \
    INSTALL_DROPIN=0 \
    HOLD_ENVIRONMENT="$HOLD_ENVIRONMENT" \
    TEST_TIMEOUT="$TEST_TIMEOUT" \
    "$RUNNER" "$LOAD_FLAG" --kind-cluster "$cluster" --focus "$focus" --skip-build --skip-dropin --kubeconfig-file "$kubeconfig_file" --test-timeout "$TEST_TIMEOUT"
  ) >"$log_file" 2>&1 &
  pids+=("$!")
done

failures=0
for index in "${!pids[@]}"; do
  pid=${pids[$index]}
  label=${labels[$index]}
  log_file=${logs[$index]}
  if wait "$pid"; then
    printf 'PASS  %s (%s)\n' "$label" "$log_file"
  else
    printf 'FAIL  %s (%s)\n' "$label" "$log_file" >&2
    failures=$((failures + 1))
  fi
done

if [ "$failures" -ne 0 ]; then
  printf '%s parallel run(s) failed\n' "$failures" >&2
  exit 1
fi

printf 'all parallel kind runs passed\n'
