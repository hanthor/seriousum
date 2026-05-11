#!/usr/bin/env bash
# Push Rust container images to GitHub Container Registry (GHCR)

set -euo pipefail

REGISTRY="ghcr.io"
OWNER="hanthor"
REPO="seriousum"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║         PUSHING IMAGES TO GHCR (GitHub Container Registry)   ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Check authentication
echo "=== Authenticating with GitHub ==="
if ! gh auth status >/dev/null 2>&1; then
  echo "✗ Not authenticated with GitHub"
  echo "Run: gh auth login"
  exit 1
fi
echo "✓ Authenticated with GitHub"
echo ""

# Get GitHub token for Docker login
echo "=== Setting up Docker authentication ==="
TOKEN=$(gh auth token)
echo "$TOKEN" | docker login "$REGISTRY" --username "$OWNER" --password-stdin
echo "✓ Docker authenticated with GHCR"
echo ""

# Images to push
IMAGES=(
  "localhost:5000/seriousum/operator-generic:local"
  "localhost:5000/seriousum/cilium-agent:local"
  "localhost:5000/seriousum/cilium-dbg:local"
  "localhost:5000/seriousum/hubble:local"
  "localhost:5000/seriousum/clustermesh-apiserver:local"
  "localhost:5000/seriousum/cilium:local"
  "localhost:5000/seriousum/cilium-cli:local"
)

echo "=== Pushing images to GHCR ==="
for img in "${IMAGES[@]}"; do
  # Extract image name and tag
  name="${img##*/}"
  base="${name%:*}"
  tag="${name##*:}"
  
  # Build GHCR image name
  ghcr_image="$REGISTRY/$OWNER/$REPO/$base:$tag"
  
  echo ""
  echo "Pushing: $img → $ghcr_image"
  
  # Tag the image
  docker tag "$img" "$ghcr_image"
  
  # Push to GHCR
  docker push "$ghcr_image"
  
  echo "✓ Pushed $base:$tag"
done

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    SUCCESS!                                   ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "Images pushed to GHCR:"
for img in "${IMAGES[@]}"; do
  name="${img##*/}"
  base="${name%:*}"
  tag="${name##*:}"
  ghcr_image="$REGISTRY/$OWNER/$REPO/$base:$tag"
  echo "  • $ghcr_image"
done
echo ""
echo "Next steps:"
echo "  1. Update scripts to use GHCR images"
echo "  2. Test with: just run K8sFQDNTest"
echo "  3. Commit changes"
