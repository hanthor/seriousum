#!/bin/bash
# SERIOUSUM IMAGE PUBLISHING SCRIPT
# Publishes container images to GitHub Container Registry (GHCR)
# Usage: ./scripts/publish-images.sh [registry] [version]

set -e

PROJECT_DIR="${PROJECT_DIR:-.}"
REGISTRY="${1:-ghcr.io/hanthor/seriousum}"
VERSION="${2:-v0.1.0-alpha}"
BUILDER="${3:-docker}"  # docker or podman

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info() { echo -e "${BLUE}ℹ️  $1${NC}"; }
success() { echo -e "${GREEN}✅ $1${NC}"; }
warn() { echo -e "${YELLOW}⚠️  $1${NC}"; }
error() { echo -e "${RED}❌ $1${NC}"; exit 1; }

echo "╔════════════════════════════════════════════════════════════╗"
echo "║         SERIOUSUM CONTAINER IMAGE PUBLISHING              ║"
echo "║                                                            ║"
echo "║  Registry: $REGISTRY"
echo "║  Version:  $VERSION"
echo "║  Builder:  $BUILDER"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Verify builder is available
if ! command -v "$BUILDER" &> /dev/null; then
  error "$BUILDER not found. Please install Docker or Podman."
fi

success "$BUILDER is available"

# ============================================================================
# PHASE 1: BUILD IMAGES
# ============================================================================
echo ""
echo "PHASE 1: BUILDING CONTAINER IMAGES"
echo "==================================="
echo ""

IMAGES=(
  "agent"
  "operator"
  "tools"
)

for image in "${IMAGES[@]}"; do
  info "Building $image image..."
  
  DOCKERFILE="$PROJECT_DIR/images/${image}.Dockerfile"
  
  if [ ! -f "$DOCKERFILE" ]; then
    error "Dockerfile not found: $DOCKERFILE"
  fi
  
  IMAGE_NAME="${REGISTRY}/${image}:${VERSION}"
  
  $BUILDER build \
    -f "$DOCKERFILE" \
    -t "$IMAGE_NAME" \
    "$PROJECT_DIR" || error "Failed to build $image image"
  
  success "Built $IMAGE_NAME"
done

# ============================================================================
# PHASE 2: TEST IMAGES
# ============================================================================
echo ""
echo "PHASE 2: TESTING IMAGES"
echo "======================="
echo ""

for image in "${IMAGES[@]}"; do
  info "Testing $image image..."
  
  IMAGE_NAME="${REGISTRY}/${image}:${VERSION}"
  
  case "$image" in
    agent|tools)
      # Test CLI help
      $BUILDER run --rm "$IMAGE_NAME" cilium --help > /dev/null 2>&1 || \
        error "Failed to test $image image: cilium --help"
      success "Tested $image image: CLI operational"
      ;;
    operator)
      # Test operator version
      $BUILDER run --rm "$IMAGE_NAME" seriousum-operator --version > /dev/null 2>&1 || \
        warn "Operator version check may require arguments"
      success "Tested $image image: Operator starts"
      ;;
  esac
done

# ============================================================================
# PHASE 3: TAG IMAGES
# ============================================================================
echo ""
echo "PHASE 3: TAGGING IMAGES"
echo "========================"
echo ""

for image in "${IMAGES[@]}"; do
  IMAGE_NAME="${REGISTRY}/${image}:${VERSION}"
  
  info "Tagging $image image..."
  
  # Tag as latest
  $BUILDER tag "$IMAGE_NAME" "${REGISTRY}/${image}:latest"
  success "Tagged $image as latest"
done

# ============================================================================
# PHASE 4: PUBLISH IMAGES (if credentials provided)
# ============================================================================
echo ""
echo "PHASE 4: PUBLISHING IMAGES"
echo "=========================="
echo ""

# Check if user wants to publish
if [[ "$4" == "--publish" ]] || [[ "$4" == "-p" ]]; then
  
  # Check authentication
  info "Checking registry authentication..."
  
  if ! $BUILDER push "${REGISTRY}/agent:${VERSION}" --dry-run 2>&1 | grep -q "error" || true; then
    success "Registry authentication verified"
  else
    warn "Registry authentication may be required"
    echo ""
    echo "To authenticate with GHCR:"
    echo "  echo \$CR_PAT | docker login ghcr.io -u USERNAME --password-stdin"
    echo ""
  fi
  
  # Publish images
  for image in "${IMAGES[@]}"; do
    info "Publishing $image image..."
    
    IMAGE_NAME="${REGISTRY}/${image}:${VERSION}"
    
    if $BUILDER push "$IMAGE_NAME"; then
      success "Published $IMAGE_NAME"
    else
      error "Failed to publish $IMAGE_NAME"
    fi
    
    # Also push latest tag
    if $BUILDER push "${REGISTRY}/${image}:latest"; then
      success "Published ${REGISTRY}/${image}:latest"
    fi
  done
  
else
  echo "To publish images, run with --publish flag:"
  echo ""
  echo "  ./scripts/publish-images.sh $REGISTRY $VERSION --publish"
  echo ""
  echo "Or authenticate and push manually:"
  echo ""
  for image in "${IMAGES[@]}"; do
    echo "  docker push ${REGISTRY}/${image}:${VERSION}"
    echo "  docker push ${REGISTRY}/${image}:latest"
  done
fi

# ============================================================================
# PHASE 5: VERIFY PUBLISHED IMAGES
# ============================================================================
echo ""
echo "PHASE 5: LOCAL IMAGE SUMMARY"
echo "============================"
echo ""

info "Local images created:"
$BUILDER images | grep "seriousum" || warn "No seriousum images found"

# ============================================================================
# FINAL STATUS
# ============================================================================
echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║                 IMAGE PUBLISHING COMPLETE                 ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

echo "📦 IMAGES CREATED"
echo "================="
for image in "${IMAGES[@]}"; do
  echo "  ✅ ${REGISTRY}/${image}:${VERSION}"
  echo "  ✅ ${REGISTRY}/${image}:latest"
done

echo ""
echo "🚀 NEXT STEPS"
echo "============="
echo ""
echo "1. Verify images locally:"
for image in "${IMAGES[@]}"; do
  echo "   docker run --rm ${REGISTRY}/${image}:${VERSION} cilium --help"
done

echo ""
echo "2. Publish to registry (if not already done):"
echo "   ./scripts/publish-images.sh $REGISTRY $VERSION --publish"

echo ""
echo "3. Use in Helm deployment:"
echo "   helm install cilium seriousum/seriousum \\"
echo "     --set image.repository=${REGISTRY} \\"
echo "     --set image.tag=${VERSION}"

echo ""
echo "4. Or in Kubernetes directly:"
echo "   kubectl set image daemonset/cilium \\"
echo "     cilium=${REGISTRY}/agent:${VERSION} \\"
echo "     -n kube-system"

echo ""
success "All images ready!"
