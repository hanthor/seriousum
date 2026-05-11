#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(git rev-parse --show-toplevel)
cd "$ROOT_DIR"

IMAGE_PREFIX=${IMAGE_PREFIX:-localhost:5000/seriousum}
IMAGE_TAG=${IMAGE_TAG:-local}
DOCKER=${DOCKER:-docker}
DRY_RUN=${DRY_RUN:-0}

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  case "$CARGO_TARGET_DIR" in
    /*) RELEASE_DIR="$CARGO_TARGET_DIR/release" ;;
    *) RELEASE_DIR="$ROOT_DIR/$CARGO_TARGET_DIR/release" ;;
  esac
else
  RELEASE_DIR="$ROOT_DIR/target/release"
fi

if [ "$DRY_RUN" != "1" ]; then
  cargo build --release --workspace --bins
fi

components=(
  cilium
  cilium-dbg
  cilium-agent
  cilium-cli
  operator
  hubble
  clustermesh-apiserver
)

build_image() {
  local component=$1
  local dockerfile="$ROOT_DIR/images/${component}.Dockerfile"
  local image_name=$component
  local image_ref

  if [ "$component" = "operator" ]; then
    image_name=operator-generic
  fi
  image_ref="$IMAGE_PREFIX/$image_name:$IMAGE_TAG"
  local build_context="$RELEASE_DIR"

  if [ ! -f "$dockerfile" ]; then
    printf 'missing Dockerfile: %s\n' "$dockerfile" >&2
    exit 1
  fi

  if [ "$component" != "cilium-agent" ] && [ "$DRY_RUN" != "1" ] && [ ! -x "$RELEASE_DIR/$component" ]; then
    printf 'missing built artifact: %s\n' "$RELEASE_DIR/$component" >&2
    exit 1
  fi

  if [ "$component" = "cilium-agent" ]; then
    build_context="$ROOT_DIR"
  fi

  if [ "$DRY_RUN" = "1" ]; then
    printf '%s build -f %q -t %q %q\n' "$DOCKER" "$dockerfile" "$image_ref" "$build_context"
  else
    "$DOCKER" build -f "$dockerfile" -t "$image_ref" "$build_context"
  fi
}

for component in "${components[@]}"; do
  build_image "$component"
done

printf 'Built local images tagged as %s/*:%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
printf 'Use these helm-style overrides as a starting point:\n'
printf '  --set image.repository=%s/cilium --set image.tag=%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
printf '  --set image.repository=%s/cilium-agent --set image.tag=%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
printf '  --set image.repository=%s/cilium-dbg --set image.tag=%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
printf '  --set operator.image.repository=%s/operator --set operator.image.tag=%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
printf '  --set hubble.image.repository=%s/hubble --set hubble.image.tag=%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
printf '  --set clustermesh.apiserver.image.repository=%s/clustermesh-apiserver --set clustermesh.apiserver.image.tag=%s\n' "$IMAGE_PREFIX" "$IMAGE_TAG"
