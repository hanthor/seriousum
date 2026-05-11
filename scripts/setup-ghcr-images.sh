#!/usr/bin/env bash
# Set up GHCR images: pull if available, fall back to local

set -euo pipefail

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║             SETTING UP IMAGES (GHCR or Local)                ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

REGISTRY="ghcr.io"
OWNER="hanthor"
REPO="seriousum"
TAG="local"

IMAGES=(
  "operator-generic"
  "cilium-agent"
  "cilium-dbg"
  "hubble"
  "clustermesh-apiserver"
  "cilium"
  "cilium-cli"
)

echo "=== Checking image availability ==="
for img in "${IMAGES[@]}"; do
  ghcr_image="$REGISTRY/$OWNER/$REPO/$img:$TAG"
  local_image="localhost:5000/seriousum/$img:$TAG"
  
  echo ""
  echo "Image: $img"
  
  # Try GHCR first
  if docker pull "$ghcr_image" 2>/dev/null; then
    echo "  ✓ Pulled from GHCR: $ghcr_image"
    # Tag as local for consistency
    docker tag "$ghcr_image" "$local_image"
    echo "  ✓ Tagged as: $local_image"
  elif docker images --format "{{.Repository}}:{{.Tag}}" | grep -q "^${local_image}$"; then
    echo "  ✓ Using local: $local_image"
  else
    echo "  ✗ Image not found (need to build: just build-images)"
  fi
done

echo ""
echo "=== Setup complete ==="
echo ""
echo "All images are now available as localhost:5000/seriousum/*:local"
echo "Ready for: just run"
