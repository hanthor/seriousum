#!/bin/bash
# build-release.sh - Build multi-platform binaries for Seriousum

set -e

VERSION="${1:-v0.1.0-alpha}"
OUTPUT_DIR="releases"

echo "🏗️  Building Seriousum $VERSION for multiple platforms..."
mkdir -p "$OUTPUT_DIR"

# Detect host OS
OS=$(uname -s)
ARCH=$(uname -m)

# Function to build for a specific target
build_target() {
    local target=$1
    local os_name=$2
    local arch_name=$3
    local ext=${4:-.tar.gz}
    
    echo ""
    echo "📦 Building for $os_name $arch_name..."
    
    # Install target if needed
    rustup target add "$target" 2>/dev/null || true
    
    # Build
    cargo build --release --target "$target" --bins 2>&1 | grep -E "Compiling|Finished"
    
    # Create archive
    local release_dir="seriousum-${VERSION#v}-${os_name}-${arch_name}"
    mkdir -p "$release_dir"
    
    # Copy binaries
    local bin_path="target/$target/release"
    cp "$bin_path/seriousum-daemon" "$release_dir/" 2>/dev/null || true
    cp "$bin_path/seriousum-cli" "$release_dir/" 2>/dev/null || true
    cp "$bin_path/cilium-dbg" "$release_dir/" 2>/dev/null || true
    cp "$bin_path/seriousum-operator" "$release_dir/" 2>/dev/null || true
    
    # Create symlinks for compatibility
    cd "$release_dir"
    ln -sf seriousum-daemon cilium-agent 2>/dev/null || true
    ln -sf seriousum-cli cilium 2>/dev/null || true
    cd ..
    
    # Archive
    if [ "$ext" = ".zip" ]; then
        zip -r "$OUTPUT_DIR/${release_dir}.zip" "$release_dir" > /dev/null
        echo "✅ Created $OUTPUT_DIR/${release_dir}.zip"
    else
        tar czf "$OUTPUT_DIR/${release_dir}.tar.gz" "$release_dir"
        echo "✅ Created $OUTPUT_DIR/${release_dir}.tar.gz"
    fi
    
    rm -rf "$release_dir"
}

# Build for supported platforms
echo "🎯 Platform targets:"
echo "  • Linux x86_64 (glibc)"
echo "  • Linux ARM64 (glibc)"
echo "  • macOS x86_64"
echo "  • macOS ARM64 (Apple Silicon)"
echo "  • Windows x86_64"

# Linux x86_64
build_target "x86_64-unknown-linux-gnu" "linux" "x86_64"

# Linux ARM64
build_target "aarch64-unknown-linux-gnu" "linux" "arm64"

# macOS x86_64
if [ "$OS" = "Darwin" ]; then
    build_target "x86_64-apple-darwin" "darwin" "x86_64"
    build_target "aarch64-apple-darwin" "darwin" "arm64"
else
    echo "⚠️  Skipping macOS builds (not on macOS)"
fi

# Windows x86_64
build_target "x86_64-pc-windows-gnu" "windows" "x86_64" ".zip"

# Generate checksums
echo ""
echo "📝 Generating checksums..."
cd "$OUTPUT_DIR"
sha256sum * > SHA256SUMS
echo "✅ Created SHA256SUMS"

echo ""
echo "🎉 Build complete!"
echo "📁 Artifacts in: $OUTPUT_DIR/"
ls -lh
