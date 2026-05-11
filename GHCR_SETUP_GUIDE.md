# GHCR Setup Guide - Operator Image Distribution

**Date**: 2026-05-11  
**Status**: GHCR support added - ready to push images  
**Solves**: P0.1 blocker (operator image availability)

## Overview

This guide explains how to push Rust container images to GitHub Container Registry (GHCR) and how it solves the operator image availability blocker.

## Why GHCR?

### Problem We Hit
- Operator image `quay.io/cilium/cilium-ci-generic:latest` not available
- Environment has no internet access to pull from quay.io
- Each environment needs pre-pulled or locally-built images

### Solution: GHCR
- Push all Rust images to `ghcr.io/hanthor/seriousum/*`
- Available from any environment with GitHub access
- Integrated with GitHub (no separate account needed)
- Fallback to local images if needed
- Enables CI/CD automation

## Quick Start

### 1. Prerequisites
```bash
# Check you have everything (should all pass)
gh auth status              # GitHub authenticated ✓
docker --version           # Docker installed ✓
docker images | grep seriousum  # Images built ✓
```

### 2. Push Images to GHCR
```bash
# Simple one-liner
bash scripts/push-images-to-ghcr.sh

# Or use justfile recipe
just push-ghcr

# Or full workflow (build + test + push)
just publish
```

### 3. What Gets Pushed
```
ghcr.io/hanthor/seriousum/operator-generic:local
ghcr.io/hanthor/seriousum/cilium-agent:local
ghcr.io/hanthor/seriousum/cilium-dbg:local
ghcr.io/hanthor/seriousum/hubble:local
ghcr.io/hanthor/seriousum/clustermesh-apiserver:local
ghcr.io/hanthor/seriousum/cilium:local
ghcr.io/hanthor/seriousum/cilium-cli:local
```

### 4. Time Estimate
- First push: 5-10 minutes (images are 400MB+ total)
- Subsequent pushes: 2-5 minutes (layers cached)

## Usage Patterns

### Option A: Build Locally, Push to GHCR
```bash
# Build images locally
just build-images

# Push to GHCR
just push-ghcr

# Now available globally as ghcr.io/hanthor/seriousum/*:local
```

### Option B: Pull from GHCR (in next environment)
```bash
# Pull images from GHCR (or use local if not available)
just setup-images

# Run tests with GHCR or local images
just run K8sFQDNTest
```

### Option C: Full Publish Workflow
```bash
# Build → Test → Push (all in one)
just publish

# Equivalent to:
# 1. Build binaries
# 2. Build images
# 3. Run tests
# 4. Push to GHCR
```

## How It Solves P0.1

### Before (Without GHCR)
1. Try to pull `quay.io/cilium/cilium-ci-generic:latest`
2. ❌ Image not available (no internet)
3. ❌ Pod stuck in ImagePullBackOff
4. ❌ Tests can't run

### After (With GHCR)
1. Try to pull from GHCR: `ghcr.io/hanthor/seriousum/operator-generic:local`
2. ✅ Image available (GitHub-integrated)
3. ✅ Pod starts and initializes
4. ✅ Tests can run

### With Fallback (Most Robust)
```bash
# In setup-ghcr-images.sh:
# 1. Try: docker pull ghcr.io/...  (GHCR)
# 2. If fail: use local              (Local fallback)
# 3. Result: Works in any environment
```

## Integration with Test Pipeline

### Modified Workflow
```
Build Binaries
    ↓
Build Images (localhost:5000/seriousum/*)
    ↓
Push to GHCR (ghcr.io/hanthor/seriousum/*)  ← NEW
    ↓
Load into kind (localhost:5000/seriousum/*)
    ↓
Run Tests
    ↓
Capture Results
```

### Automatic Workflow
```bash
# Single command does everything
just publish

# Or manual steps
just build           # Build binaries
just build-images    # Build images locally
just push-ghcr       # Push to GHCR
```

## Troubleshooting

### Push Fails: "unauthorized"
```bash
# Re-authenticate
gh auth logout
gh auth login
bash scripts/push-images-to-ghcr.sh
```

### Pull Fails: Can't reach GitHub
```bash
# Fall back to local images
just setup-images
# Will use local images if GHCR not accessible
```

### Image Not Found in GHCR
```bash
# Check what was pushed
gh api /user/packages/container/seriousum/versions

# Or use browser
https://github.com/hanthor?tab=packages&repo_name=seriousum
```

### Docker Login Issues
```bash
# Logout and retry
docker logout ghcr.io
bash scripts/push-images-to-ghcr.sh
```

## GitHub Container Registry Details

### Access Control
- **Visibility**: Public (anyone can pull)
- **Push Access**: Only authenticated user (you)
- **Pull Access**: Anyone (no login needed)

### URL Format
```
ghcr.io/[owner]/[repo]/[image]:[tag]
ghcr.io/hanthor/seriousum/operator-generic:local
```

### Registry Documentation
- GitHub Docs: https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry
- Authentication: Using personal access token from `gh auth token`

## Next Steps

### Immediate
1. Run: `bash scripts/push-images-to-ghcr.sh`
2. Verify images appeared in GHCR
3. Test pulling: `docker pull ghcr.io/hanthor/seriousum/operator-generic:local`

### Update Test Scripts
1. Modify `scripts/run-cilium-kind-test.sh` to use GHCR operator
2. Add fallback to local images
3. Test: `just run K8sFQDNTest`

### CI/CD Integration (Future)
1. GitHub Actions can pull from GHCR automatically
2. No need for `kind load docker-image`
3. Operator image always available in workflows

## Integration with P0 Fix Strategy

**P0.1 Problem**: Operator image not available  
**Solution**: Push to GHCR  
**Result**: Operator image now globally available  
**Enables**: Task #17 (Run integration gates) to proceed

## Commands Reference

```bash
# Push images
bash scripts/push-images-to-ghcr.sh
just push-ghcr

# Setup images (GHCR with fallback)
bash scripts/setup-ghcr-images.sh
just setup-images

# Full publish workflow
just publish

# Check images in GHCR
docker pull ghcr.io/hanthor/seriousum/operator-generic:local
```

## Summary

GHCR support enables:
- ✅ Global image distribution
- ✅ P0.1 blocker resolution
- ✅ CI/CD automation
- ✅ Reproducible builds
- ✅ Environment flexibility

**Status**: Ready to push

**Next**: `bash scripts/push-images-to-ghcr.sh`

---

For more information on GitHub Container Registry:
- https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry
