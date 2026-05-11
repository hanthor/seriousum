#!/bin/bash
# build-containers.sh - Build and push Seriousum container images

set -e

REGISTRY="${REGISTRY:-ghcr.io}"
USERNAME="${USERNAME:-hanthor}"
VERSION="${VERSION:-v0.1.0-alpha}"
PUSH="${PUSH:-false}"

echo "🐳 Building Seriousum container images..."
echo "Registry: $REGISTRY/$USERNAME/seriousum"
echo "Version: $VERSION"

# Function to build and optionally push image
build_image() {
    local name=$1
    local dockerfile=$2
    local description=$3
    
    echo ""
    echo "📦 Building $name image..."
    echo "   Description: $description"
    
    local image="$REGISTRY/$USERNAME/seriousum/$name:$VERSION"
    local latest="$REGISTRY/$USERNAME/seriousum/$name:latest"
    
    # Build image
    docker build \
        -f "$dockerfile" \
        -t "$image" \
        -t "$latest" \
        --label "org.opencontainers.image.version=$VERSION" \
        --label "org.opencontainers.image.source=https://github.com/$USERNAME/seriousum" \
        . 2>&1 | tail -5
    
    echo "✅ Built $image"
    
    # Push if requested
    if [ "$PUSH" = "true" ]; then
        echo "📤 Pushing $image..."
        docker push "$image"
        docker push "$latest"
        echo "✅ Pushed to $registry"
    else
        echo "⏭️  Skipping push (set PUSH=true to push)"
    fi
}

# Build images
build_image "agent" "images/agent.Dockerfile" \
    "Cilium agent (daemon + CLI) in Rust"

build_image "operator" "images/operator.Dockerfile" \
    "Kubernetes operator for Cilium in Rust"

build_image "tools" "images/tools.Dockerfile" \
    "Cilium diagnostic tools (cilium-cli, cilium-dbg)"

echo ""
echo "🎉 Container images built successfully!"
echo ""
echo "📊 Available images:"
docker images | grep "seriousum"
echo ""
echo "🚀 To push images to registry:"
echo "   PUSH=true bash scripts/build-containers.sh"
echo ""
echo "🐳 To run agent locally:"
echo "   docker run $REGISTRY/$USERNAME/seriousum/agent:$VERSION --help"
