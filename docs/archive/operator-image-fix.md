# Operator Image Fix: Diagnostic Document

## Overview

This document explains the operator image naming convention and why explicit overrides are required when running Cilium with the Seriousum Rust-based operator in a kind cluster. It covers the interaction between chart-applied suffixes, helm overrides, and Kubernetes image pull policies.

---

## 1. Why the Chart Appends '-generic' to Operator Image Names

### The Cilium Design

The Cilium Helm chart applies a **cloud-specific suffix** to the operator image name. This allows a single repository and tag configuration to support multiple cloud environments without explicit overrides.

**Template Logic** (from `cilium-operator/_helpers.tpl`):

```
{{- printf "%s-%s%s%s%s" .Values.operator.image.repository $cloud .Values.operator.image.suffix $tag $imageDigest -}}
```

This constructs the image as: `{repository}-{cloud}{suffix}{tag}{@digest}`

### The '-generic' Suffix

- **Default cloud detection**: When no cloud provider is enabled (`eni.enabled`, `azure.enabled`, etc.), the chart defaults to `$cloud = "generic"`
- **Result**: For a local development setup, the chart automatically transforms:
  - **Input repository**: `localhost:5000/seriousum/operator`
  - **Final image**: `localhost:5000/seriousum/operator-generic:local`

The '-generic' suffix is **not** a suffix in the traditional sense—it's the cloud-provider variant name appended by the chart template.

### Why This Design?

Cilium builds separate operator images for each cloud provider (aws, azure, alibabacloud, generic):
- Each variant includes cloud-specific drivers and configuration
- The chart determines which variant to use based on enabled features
- This avoids requiring cloud-specific image repository overrides for most users

---

## 2. Why We Need Explicit operator.image.repository and operator.image.tag Overrides

### The Problem

When building custom Seriousum images:
1. **Build script creates**: `localhost:5000/seriousum/operator-generic:local`
2. **Chart default values**: `operator.image.repository: "quay.io/cilium/operator"`
3. **Without overrides**, the chart constructs: `quay.io/cilium/operator-generic:latest`
4. **Result**: kind cluster tries to pull the official Cilium operator, not your built image

### The Solution

The `run-cilium-kind-test.sh` script sets explicit overrides:

```bash
CILIUM_OPERATOR_IMAGE="$IMAGE_PREFIX/operator"          # e.g., localhost:5000/seriousum/operator
CILIUM_OPERATOR_TAG="$IMAGE_TAG"                        # e.g., local

# Passed to helm as:
operator.image.repository=$CILIUM_OPERATOR_IMAGE        # localhost:5000/seriousum/operator
operator.image.tag=$CILIUM_OPERATOR_TAG                 # local
```

With these overrides, the chart constructs:
- **Without suffix override**: `localhost:5000/seriousum/operator-generic:local` ✓ (matches built image)

### Key Constraints

To make this work, you **must also override the suffix**:

```bash
-cilium.operator-suffix=""
```

Without this, the chart would construct:
- With default suffix `-ci`: `localhost:5000/seriousum/operator-ci:local` ✗ (image doesn't exist)

**Why override the suffix?**
- The upstream Cilium default is `-ci` (for CI builds)
- Seriousum builds use `-generic` suffix at the Docker image build level
- Setting suffix to empty string allows the chart to use the repository name as-is
- The build script already names the image with the cloud variant suffix, so the chart shouldn't add another

---

## 3. CILIUM_OPERATOR_IMAGE (Base) vs. The '-generic' Suffix

### Two Different Components

#### CILIUM_OPERATOR_IMAGE: The Repository Base
```bash
CILIUM_OPERATOR_IMAGE="$IMAGE_PREFIX/operator"
# Example: localhost:5000/seriousum/operator
```
- This is the **repository name** without a tag
- It's used as the helm value: `operator.image.repository`
- The chart appends the cloud suffix to this base

#### The '-generic' Suffix: The Cloud Variant
```bash
kind load docker-image --name "$KIND_CLUSTER" "$IMAGE_PREFIX/operator-generic:$IMAGE_TAG"
# Example: localhost:5000/seriousum/operator-generic:local
```
- This is the **actual Docker image** that was built
- It's what the build script creates: `images/build-cilium-images.sh` names it `operator-generic`
- The chart adds `-generic` suffix to `$IMAGE_PREFIX/operator` to match this actual image name

### The Workflow

1. **Build produces**: `localhost:5000/seriousum/operator-generic:local`
2. **Environment variable set**: `CILIUM_OPERATOR_IMAGE=localhost:5000/seriousum/operator`
3. **Helm receives**: `operator.image.repository=localhost:5000/seriousum/operator`
4. **Chart calculates**: `localhost:5000/seriousum/operator` + `-generic` + `:local` = **`localhost:5000/seriousum/operator-generic:local`**
5. **Match**: Kubernetes can find and run the image

### Why Build as 'operator-generic'?

The `build-cilium-images.sh` script does this intentionally:

```bash
if [ "$component" = "operator" ]; then
  image_name=operator-generic
fi
```

This mimics the Cilium upstream convention where the buildroot/Dockerfile produces `operator-generic` directly (as part of the make system). By building as `-generic`, we align with both:
- Cilium's naming scheme
- What the chart expects after applying its own cloud-variant logic

---

## 4. How 'kind load docker-image' and IfNotPresent pullPolicy Work Together

### Understanding the Interaction

#### 'kind load docker-image': Local Import

```bash
kind load docker-image --name "$KIND_CLUSTER" "$IMAGE_PREFIX/operator-generic:$IMAGE_TAG"
# Example: kind load docker-image --name kind localhost:5000/seriousum/operator-generic:local
```

**What it does:**
- Imports the Docker image from your local Docker daemon into the kind cluster's node(s)
- Makes the image available as if it were pulled from a registry, but it's resident in the kind node
- Useful for local development without needing a registry or internet access

**Why use it:**
- Speeds up testing: no network round-trip to fetch images
- Works with local registries or offline scenarios
- Allows testing built images before pushing to a registry

#### IfNotPresent pullPolicy

```bash
operator.image.pullPolicy=IfNotPresent
```

**What it does:**
- Instructs the kubelet to use a locally cached image if available
- Only pulls from the registry if the image:tag is not found locally
- Avoids unnecessary network calls for images already on the node

**Helm override in run script:**

```bash
CLUSTERMESH_INSTALL_OVERRIDES="...operator.image.pullPolicy=IfNotPresent..."
```

### The Complete Flow

1. **Build phase**:
   ```bash
   docker build -t localhost:5000/seriousum/operator-generic:local ...
   ```
   Image exists in local Docker daemon.

2. **kind load phase**:
   ```bash
   kind load docker-image --name kind localhost:5000/seriousum/operator-generic:local
   ```
   Image is transferred from local Docker daemon → kind cluster node(s).

3. **Deploy phase** (Helm install):
   - Helm chart generates Kubernetes manifests with:
     - `image: localhost:5000/seriousum/operator-generic:local`
     - `imagePullPolicy: IfNotPresent`

4. **Runtime phase** (kubelet on kind node):
   - kubelet sees `imagePullPolicy: IfNotPresent`
   - Checks if `localhost:5000/seriousum/operator-generic:local` exists locally
   - **Found** (from step 2): Uses the cached image, **no pull occurs**
   - Container starts immediately without network access

### Why This Matters

**Without IfNotPresent (default is 'Always'):**
- kubelet attempts to pull `localhost:5000/seriousum/operator-generic:local` from registry
- Local registry must be running and accessible
- Fails if the image isn't in the registry (e.g., only local Docker daemon)

**With IfNotPresent + kind load:**
- No registry access needed
- Even works with `localhost:5000` as long as `kind load docker-image` succeeded
- Fast iteration: change code → build → load → deploy → test

### Practical Example

Without the IfNotPresent override:
```
kubelet error: failed to pull image "localhost:5000/seriousum/operator-generic:local": 
  rpc error: code = Unknown desc = failed to pull and unpack image:
  failed to resolve reference: failed to do request: Head "http://localhost:5000/...": 
  connection refused
```

With IfNotPresent override:
```
Pod starts successfully using locally cached image
```

---

## Complete Integration Example

### Full Command Flow

```bash
# 1. Build images
IMAGE_PREFIX=localhost:5000/seriousum IMAGE_TAG=local ./images/build-cilium-images.sh
# Produces: localhost:5000/seriousum/operator-generic:local (in local Docker daemon)

# 2. Create/prepare kind cluster
kind create cluster --name my-test

# 3. Load image into kind
kind load docker-image --name my-test localhost:5000/seriousum/operator-generic:local
# Transfers to kind node

# 4. Install Cilium with helm overrides
helm install cilium cilium/cilium --namespace kube-system \
  --set operator.image.repository=localhost:5000/seriousum/operator \
  --set operator.image.tag=local \
  --set operator.image.pullPolicy=IfNotPresent \
  --set operator.image.suffix="" \
  --set operator.image.useDigest=false

# 5. Cilium operator pod starts
# - Chart renders image: localhost:5000/seriousum/operator-generic:local
# - kubelet with IfNotPresent finds locally cached image
# - Pod runs immediately
```

### What the Chart Sees

The helm values result in this Deployment image spec:

```yaml
spec:
  containers:
  - name: cilium-operator
    image: localhost:5000/seriousum/operator-generic:local
    imagePullPolicy: IfNotPresent
```

The Cilium chart constructs this by:
1. Taking `operator.image.repository` = `localhost:5000/seriousum/operator`
2. Appending cloud variant `-generic`
3. Appending tag `:local`
4. Result: `localhost:5000/seriousum/operator-generic:local`

---

## Summary Table

| Component | Value | Purpose |
|-----------|-------|---------|
| **Build artifact** | `localhost:5000/seriousum/operator-generic:local` | Docker image created by build script |
| **CILIUM_OPERATOR_IMAGE** | `localhost:5000/seriousum/operator` | Helm override for repository base (no cloud suffix, no tag) |
| **operator.image.suffix** | `""` | Override to prevent chart from adding `-ci` or other suffix |
| **operator.image.tag** | `local` | Helm override for image tag |
| **kind load docker-image** | `localhost:5000/seriousum/operator-generic:local` | Command to import image into kind node cache |
| **imagePullPolicy** | `IfNotPresent` | Tell kubelet to use cached image, don't pull from registry |
| **Rendered image spec** | `localhost:5000/seriousum/operator-generic:local` (IfNotPresent) | Final image used by container runtime |

---

## References

- Build script: `images/build-cilium-images.sh`
- Test runner: `scripts/run-cilium-kind-test.sh`
- Chart template: `cilium/templates/cilium-operator/_helpers.tpl`
- Cilium values: `cilium/values.yaml`
