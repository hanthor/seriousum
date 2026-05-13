# Cilium/Rust Integration Testing Justfile
# Simple recipes for building, testing, and managing the test environment

set shell := ["bash", "-c"]
set positional-arguments := true

# Default variables
IMAGE_PREFIX := "localhost:5000/seriousum"
IMAGE_TAG := "local"
KIND_CLUSTER := "kind"
CILIUM_REPO := "/var/home/james/dev/cilium"
TEST_TIMEOUT := "12m"

# Upstream Cilium CI images — used for benchmark comparisons against seriousum
CILIUM_CI_AGENT := "quay.io/cilium/cilium-ci"
CILIUM_CI_TAG := "latest"
CILIUM_CI_OPERATOR := "quay.io/cilium/operator-generic-ci:latest"
CILIUM_CI_HUBBLE_RELAY := "quay.io/cilium/hubble-relay-ci:latest"

# Colors for output
GREEN := '\033[0;32m'
BLUE := '\033[0;34m'
NC := '\033[0m' # No Color

# Show all recipes
@help:
    echo "{{GREEN}}Cilium/Rust Integration Testing Recipes{{NC}}"
    echo ""
    just --list

# Build all Rust images locally
@build-images:
    echo "{{BLUE}}Building Rust images...{{NC}}"
    ./images/build-cilium-images.sh
    echo "{{GREEN}}Images built!{{NC}}"

# Load images into kind cluster
@load-images:
    echo "{{BLUE}}Loading images into kind cluster {{KIND_CLUSTER}}...{{NC}}"
    ./scripts/run-cilium-kind-test.sh --load --skip-build --skip-dropin
    echo "{{GREEN}}Images loaded!{{NC}}"

# Build and load images (full pipeline)
@build-and-load: build-images load-images

# Create a fresh kind cluster
@cluster-create:
    echo "{{BLUE}}Creating kind cluster {{KIND_CLUSTER}}...{{NC}}"
    kind create cluster --name {{KIND_CLUSTER}} --kubeconfig ./target/cilium-kind/kind.kubeconfig
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    echo "{{GREEN}}Cluster created!{{NC}}"

# Delete the kind cluster
@cluster-delete:
    echo "{{BLUE}}Deleting kind cluster {{KIND_CLUSTER}}...{{NC}}"
    kind delete cluster --name {{KIND_CLUSTER}} || true
    echo "{{GREEN}}Cluster deleted!{{NC}}"

# Reset the kind cluster (delete + create)
@cluster-reset: cluster-delete cluster-create
    echo "{{GREEN}}Cluster reset!{{NC}}"

# Show cluster status
@cluster-status:
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    echo "{{BLUE}}Kind clusters:{{NC}}"
    kind get clusters || echo "No clusters"
    echo ""
    echo "{{BLUE}}Pods in kube-system:{{NC}}"
    kubectl get pods -n kube-system -o wide || echo "Cluster not ready"

# Run a focused ginkgo test
@test focus='' timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running test with focus: {{focus}}{{NC}}"
    if [ -z "{{focus}}" ]; then echo "Usage: just test <pattern>"; exit 1; fi
    ./scripts/run-cilium-kind-test.sh --load --focus "{{focus}}" --test-timeout "{{timeout}}"

# Run datapath services test
@test-services timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running K8sDatapathServicesTest{{NC}}"
    just test "K8sDatapathServicesTest" "{{timeout}}"

# Run network policies test
@test-policies timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running K8sNetworkPoliciesTest{{NC}}"
    just test "K8sNetworkPoliciesTest" "{{timeout}}"

# Run hubble test
@test-hubble timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running K8sHubbleTest{{NC}}"
    just test "K8sHubbleTest" "{{timeout}}"

# Run FQDN test
@test-fqdn timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running K8sFQDNTest{{NC}}"
    just test "K8sFQDNTest" "{{timeout}}"

# Run parallel matrix of focused suites across separate clusters
@test-matrix timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running parallel test matrix (12 clusters){{NC}}"
    ./scripts/run-cilium-kind-matrix.sh \
        --load \
        --test-timeout "{{timeout}}"

# Hold environment for debugging (auto-loads images)
@test-debug focus='K8sDatapathServicesTest' timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running test in debug mode (environment held after failure){{NC}}"
    HOLD_ENVIRONMENT=true ./scripts/run-cilium-kind-test.sh --load --focus "{{focus}}" --test-timeout "{{timeout}}"

# Inspect pod logs
@logs pod namespace='kube-system' lines='50':
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    echo "{{BLUE}}Logs for {{pod}} in {{namespace}}{{NC}}"
    kubectl logs -n {{namespace}} {{pod}} --tail={{lines}} || kubectl logs -n {{namespace}} {{pod}} --tail={{lines}} --previous

# Get agent pod logs
@logs-agent lines='50':
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    POD=$(kubectl get pods -n kube-system -l k8s-app=cilium --field-selector=status.phase=Running -o jsonpath='{.items[0].metadata.name}' 2>/dev/null); if [ -z "$POD" ]; then echo "No running cilium agent pod found"; exit 1; fi
    just logs "$POD" kube-system {{lines}}

# Get operator pod logs
@logs-operator lines='50':
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    POD=$(kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator --field-selector=status.phase=Running -o jsonpath='{.items[0].metadata.name}' 2>/dev/null); if [ -z "$POD" ]; then echo "No running cilium operator pod found"; exit 1; fi
    just logs "$POD" kube-system {{lines}}

# Describe pod for debugging
@describe pod namespace='kube-system':
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    kubectl describe pod -n {{namespace}} {{pod}}

# Build Rust workspace and run tests
@check:
    echo "{{BLUE}}Checking Rust workspace...{{NC}}"
    cargo check --workspace --release
    echo "{{BLUE}}Running clippy...{{NC}}"
    cargo clippy --workspace --release
    echo "{{GREEN}}Workspace checks passed!{{NC}}"

# Build release binaries
@build:
    echo "{{BLUE}}Building release binaries...{{NC}}"
    cargo build --workspace --release
    echo "{{GREEN}}Build complete!{{NC}}"

# Build drop-in directory
@build-dropin:
    echo "{{BLUE}}Building drop-in directory...{{NC}}"
    ./scripts/build-cilium-dropin.sh
    echo "{{GREEN}}Drop-in directory created!{{NC}}"

# Run compliance check
@compliance:
    echo "{{BLUE}}Checking component porting compliance...{{NC}}"
    ./scripts/check-component-porting-compliance.sh
    echo "{{GREEN}}Compliance report updated!{{NC}}"

# Validate scripts
@validate:
    echo "{{BLUE}}Validating shell scripts...{{NC}}"
    bash -n ./scripts/run-cilium-kind-test.sh
    bash -n ./scripts/run-cilium-kind-matrix.sh
    bash -n ./images/build-cilium-images.sh
    bash -n ./scripts/build-cilium-dropin.sh
    echo "{{GREEN}}All scripts valid!{{NC}}"

# Full pipeline: build images, create cluster, load, and run tests for a suite
# Usage: just run [suite] [timeout]
# Examples: just run           # Run K8sFQDNTest (fastest, 3 specs)
#           just run K8sDatapathServicesTest
#           just run K8sAgentPolicyTest 30m
@run suite='K8sFQDNTest' timeout=TEST_TIMEOUT:
    echo "{{GREEN}}Starting full build and test pipeline for {{suite}}{{NC}}"
    echo "Suite: {{suite}}"
    echo "Timeout: {{timeout}}"
    echo ""
    
    echo "{{BLUE}}[1/5] Building release binaries...{{NC}}"
    cargo build --workspace --release
    echo "{{GREEN}}✓ Binaries built{{NC}}"
    echo ""
    
    echo "{{BLUE}}[2/5] Building container images...{{NC}}"
    ./images/build-cilium-images.sh
    echo "{{GREEN}}✓ Images built{{NC}}"
    echo ""
    
    echo "{{BLUE}}[3/5] Resetting kind cluster...{{NC}}"
    kind delete cluster --name {{KIND_CLUSTER}} >/dev/null 2>&1 || true
    mkdir -p target/cilium-kind
    kind create cluster --name {{KIND_CLUSTER}} --kubeconfig ./target/cilium-kind/kind.kubeconfig
    echo "{{GREEN}}✓ Cluster ready{{NC}}"
    echo ""
    
    echo "{{BLUE}}[4/5] Loading images into cluster...{{NC}}"
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    kind load docker-image --name {{KIND_CLUSTER}} localhost:5000/seriousum/cilium-agent:local
    kind load docker-image --name {{KIND_CLUSTER}} localhost:5000/seriousum/cilium-dbg:local
    kind load docker-image --name {{KIND_CLUSTER}} localhost:5000/seriousum/operator-generic:local
    kind load docker-image --name {{KIND_CLUSTER}} localhost:5000/seriousum/hubble:local
    kind load docker-image --name {{KIND_CLUSTER}} localhost:5000/seriousum/clustermesh-apiserver:local
    echo "{{GREEN}}✓ Images loaded{{NC}}"
    echo ""
    
    echo "{{BLUE}}[5/5] Running {{suite}} tests...{{NC}}"
    export KUBECONFIG=./target/cilium-kind/kind.kubeconfig
    export PATH="$PWD/target/cilium-dropin:$PATH"
    ./scripts/run-cilium-kind-test.sh \
        --focus "{{suite}}" \
        --test-timeout "{{timeout}}" \
        --skip-build \
        --no-load

# Full setup: build everything, load images, create cluster
@setup: build build-images build-dropin cluster-reset load-images
    echo "{{GREEN}}Full setup complete! Run 'just test-services' to test.{{NC}}"

# Quick smoke test (fast focus, short timeout)
@smoke timeout='2m':
    echo "{{BLUE}}Running smoke test (quick validation){{NC}}"
    just test-services "{{timeout}}" 2>&1 | head -100 || true

# Show environment
@env:
    echo "{{BLUE}}Environment variables:{{NC}}"
    echo "IMAGE_PREFIX={{IMAGE_PREFIX}}"
    echo "IMAGE_TAG={{IMAGE_TAG}}"
    echo "KIND_CLUSTER={{KIND_CLUSTER}}"
    echo "CILIUM_REPO={{CILIUM_REPO}}"
    echo "TEST_TIMEOUT={{TEST_TIMEOUT}}"
    echo "KUBECONFIG=./target/cilium-kind/kind.kubeconfig"

# Clean build artifacts
@clean:
    echo "{{BLUE}}Cleaning build artifacts...{{NC}}"
    cargo clean
    rm -rf target/cilium-kind target/cilium-dropin
    echo "{{GREEN}}Cleaned!{{NC}}"

# Full clean (also deletes kind cluster)
@clean-all: cluster-delete clean
    echo "{{GREEN}}Full clean complete!{{NC}}"

# Aggressive cleanup: kills all kind clusters, removes docker networks, prunes containers
@clean-aggressive:
    echo "{{BLUE}}Performing aggressive cleanup...{{NC}}"
    echo "  Deleting all kind clusters..."
    kind get clusters 2>/dev/null | xargs -I {} kind delete cluster --name {} >/dev/null 2>&1 || true
    echo "  Removing kind-cilium network..."
    docker network rm kind-cilium >/dev/null 2>&1 || true
    docker network rm kind >/dev/null 2>&1 || true
    echo "  Pruning all docker containers..."
    docker container prune -f >/dev/null 2>&1 || true
    echo "  Pruning docker networks..."
    docker network prune -f >/dev/null 2>&1 || true
    echo "  Waiting for system to stabilize..."
    sleep 3
    echo "{{GREEN}}Aggressive cleanup complete!{{NC}}"

# Cleanup before fresh test run
@fresh-prep: clean-aggressive
    echo "{{BLUE}}Preparing for fresh test run...{{NC}}"
    mkdir -p target/cilium-kind target/cilium-dropin
    echo "{{GREEN}}Ready for fresh test!{{NC}}"

# Run multiple test suites sequentially on a single cluster (efficient)
@test-sequential timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running test suites sequentially{{NC}}"
    ./scripts/run-cilium-sequential-suites.sh --test-timeout "{{timeout}}"

# Run all major suites sequentially with default timeout
@test-all-sequential timeout=TEST_TIMEOUT:
    echo "{{BLUE}}Running all major suites sequentially{{NC}}"
    ./scripts/run-cilium-sequential-suites.sh \
        --suites "K8sAgentFQDNTest,K8sDatapathServicesTest,K8sAgentPolicyTest" \
        --test-timeout "{{timeout}}"

# Profile Cilium startup sequence to identify bottlenecks
@profile-startup:
    echo "{{BLUE}}Profiling Cilium startup sequence...{{NC}}"
    ./scripts/profile-cilium-startup.sh

# Push images to GitHub Container Registry (GHCR)
@push-ghcr:
    echo "{{BLUE}}Pushing images to GHCR...{{NC}}"
    ./scripts/push-images-to-ghcr.sh
    echo "{{GREEN}}Images pushed to GHCR!{{NC}}"

# Build, test, and push images to GHCR (full workflow)
@publish:
    echo "{{GREEN}}Complete publish workflow: build → test → push{{NC}}"
    just build
    just build-images
    echo "{{BLUE}}Running tests...{{NC}}"
    cargo test --workspace --release 2>&1 | grep -E "test result:|passed|failed" | tail -5
    echo "{{BLUE}}Pushing to GHCR...{{NC}}"
    just push-ghcr
    echo "{{GREEN}}✓ Publish complete!{{NC}}"

# Set up images from GHCR (with local fallback)
@setup-images:
    echo "{{BLUE}}Setting up images (GHCR with local fallback)...{{NC}}"
    ./scripts/setup-ghcr-images.sh
    echo "{{GREEN}}✓ Images ready{{NC}}"

# ============================================================================
# INTEGRATION TEST SETUP (learned steps)
# ============================================================================

# Build the ginkgo test.test binary in the upstream Cilium repo
@ginkgo-build:
    echo "{{BLUE}}Building ginkgo test binary in {{CILIUM_REPO}}/test...{{NC}}"
    cd {{CILIUM_REPO}}/test && ginkgo build .
    echo "{{GREEN}}✓ {{CILIUM_REPO}}/test/test.test built{{NC}}"

# Build the cilium-agent Docker image without BuildKit attestations (required for kind load)
@build-agent-compat tag=IMAGE_TAG:
    echo "{{BLUE}}Building cilium-agent image (kind-compatible, no attestations)...{{NC}}"
    DOCKER_BUILDKIT=0 docker build --platform linux/amd64 \
        -t localhost/seriousum/cilium-agent:{{tag}} \
        -f images/cilium-agent.Dockerfile .
    echo "{{GREEN}}✓ localhost/seriousum/cilium-agent:{{tag}} built{{NC}}"

# Build ALL images without BuildKit attestations so kind load works
@build-all-compat tag=IMAGE_TAG:
    echo "{{BLUE}}Building all images (kind-compatible)...{{NC}}"
    DOCKER_BUILDKIT=0 docker build --platform linux/amd64 \
        -t localhost:5000/seriousum/cilium-agent:{{tag}} \
        -f images/cilium-agent.Dockerfile .
    DOCKER_BUILDKIT=0 docker build --platform linux/amd64 \
        -t localhost:5000/seriousum/operator-generic:{{tag}} \
        -f images/operator.Dockerfile target/release
    DOCKER_BUILDKIT=0 docker build --platform linux/amd64 \
        -t localhost:5000/seriousum/hubble:{{tag}} \
        -f images/hubble.Dockerfile target/release
    DOCKER_BUILDKIT=0 docker build --platform linux/amd64 \
        -t localhost:5000/seriousum/cilium-dbg:{{tag}} \
        -f images/cilium-dbg.Dockerfile target/release
    DOCKER_BUILDKIT=0 docker build --platform linux/amd64 \
        -t localhost:5000/seriousum/clustermesh-apiserver:{{tag}} \
        -f images/clustermesh-apiserver.Dockerfile target/release
    echo "{{GREEN}}✓ All images built as localhost:5000/seriousum/*:{{tag}}{{NC}}"

# Load ALL seriousum images into an existing kind cluster
@load-all cluster=KIND_CLUSTER tag=IMAGE_TAG:
    #!/usr/bin/env bash
    set -euo pipefail
    for img in cilium-agent operator-generic hubble cilium-dbg clustermesh-apiserver; do
        echo "Loading localhost:5000/seriousum/$img:{{tag}} into {{cluster}}..."
        kind load docker-image "localhost:5000/seriousum/$img:{{tag}}" --name {{cluster}} 2>&1 || \
            echo "  (skipped $img — image may not exist locally)"
    done
    echo "Done loading images into {{cluster}}"

# Load a pre-built agent image into an existing kind cluster (no cluster recreation)
@load-agent cluster=KIND_CLUSTER tag=IMAGE_TAG:
    echo "{{BLUE}}Loading cilium-agent:{{tag}} into kind cluster '{{cluster}}'...{{NC}}"
    kind load docker-image localhost/seriousum/cilium-agent:{{tag}} --name {{cluster}}
    echo "{{GREEN}}✓ Image loaded into {{cluster}}{{NC}}"

# Build dropin aliases and ginkgo binary — one-time setup before running tests
@test-setup:
    echo "{{BLUE}}Setting up integration test prerequisites...{{NC}}"
    just ginkgo-build
    just build
    ./scripts/build-cilium-dropin.sh target/cilium-dropin
    echo "{{GREEN}}✓ test-setup complete (ginkgo binary + dropin ready){{NC}}"

# Run ginkgo against an EXISTING cluster using pre-loaded images (no build, no cluster recreation)
# Images must already be loaded into the cluster via `just load-all` or `just build-all-compat`+`just load-all`
# Usage: just run-existing cilium-ginkgo K8sAgentChaosTest
run-existing cluster=KIND_CLUSTER focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT:
    #!/usr/bin/env bash
    set -euo pipefail
    ROOT="$(git rev-parse --show-toplevel)"
    IMGPREFIX="localhost:5000/seriousum"
    TAG="{{IMAGE_TAG}}"
    mkdir -p "$ROOT/target/cilium-kind"
    KUBECONF="$ROOT/target/cilium-kind/{{cluster}}.kubeconfig"
    kind get kubeconfig --name {{cluster}} > "$KUBECONF"
    export KUBECONFIG="$KUBECONF"
    export PATH="$ROOT/target/cilium-dropin:$PATH"
    export CNI_INTEGRATION=kind
    export INTEGRATION_TESTS=true
    # Mirror CI's fresh-cluster assumption: remove orphaned non-Helm objects that
    # can survive interrupted local runs and block chart re-install ownership checks.
    kubectl -n kube-system delete serviceaccount cilium-envoy --ignore-not-found >/dev/null 2>&1 || true
    kubectl -n kube-system delete configmap cilium-envoy-config --ignore-not-found >/dev/null 2>&1 || true
    K8S_VER=$(kubectl version -o json 2>/dev/null \
        | python3 -c "import sys,json; v=json.load(sys.stdin)['serverVersion']; print(v['major']+'.'+v['minor'])" \
        2>/dev/null || echo "1.33")
    export K8S_VERSION="$K8S_VER"
    # Put all image settings into HELM_OVERRIDES so they go through cliOverrideOptions
    # (applied last via maps.Copy — always wins over defaultHelmOptions and kindHelmOverrides).
    # This avoids the env-var→defaultHelmOptions double-suffix bug when env vars from prior
    # runs are still exported in the shell.
    # operator.image.override bypasses the chart's "repository-cloud+suffix" logic.
    # The chart appends "-generic" (cloud) automatically, which would double the suffix.
    HELM_OVERRIDES="image.repository=$IMGPREFIX/cilium-agent,image.tag=$TAG,image.useDigest=false,image.pullPolicy=Never,operator.image.override=$IMGPREFIX/operator-generic:$TAG,operator.image.pullPolicy=Never,hubble.relay.image.repository=$IMGPREFIX/hubble,hubble.relay.image.tag=$TAG,hubble.relay.image.useDigest=false,hubble.relay.image.pullPolicy=Never,clustermesh.apiserver.image.repository=$IMGPREFIX/clustermesh-apiserver,clustermesh.apiserver.image.tag=$TAG,clustermesh.apiserver.image.useDigest=false,clustermesh.apiserver.image.pullPolicy=Never"
    echo "Running focus='{{focus}}' against cluster='{{cluster}}' (k8s $K8S_VER)"
    cd {{CILIUM_REPO}}/test
    timeout --preserve-status --kill-after=5m {{timeout}} \
        ./test.test \
            --ginkgo.focus="{{focus}}" \
            --ginkgo.v \
            -- \
            -cilium.testScope=k8s \
            -cilium.kubeconfig="$KUBECONF" \
            -cilium.install-helm-overrides="$HELM_OVERRIDES" \
            -cilium.holdEnvironment=false

# Recreate cluster + load local seriousum images + run ginkgo focus.
# Mirrors upstream CI behavior (fresh kind cluster per run), avoiding stale-resource
# ownership conflicts between repeated local test iterations.
# Usage: just run-existing-fresh cilium-ginkgo K8sAgentChaosTest
run-existing-fresh cluster='cilium-ginkgo' focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT agent_port_prefix='234' operator_port_prefix='235':
    just ginkgo-cluster {{cluster}} {{agent_port_prefix}} {{operator_port_prefix}}
    just load-all {{cluster}}
    just run-existing {{cluster}} {{focus}} {{timeout}}

# ============================================================================
# PARALLEL TESTING & IMPLEMENTATION WORKFLOWS
# ============================================================================
# These recipes enable running multiple test suites and implementation tasks
# in parallel for faster iteration and earlier feedback

# Run 3 test suites in parallel on separate kind clusters
@test-parallel timeout=TEST_TIMEOUT:
    echo "{{GREEN}}Starting 3 test suites in parallel...{{NC}}"
    bash scripts/run-parallel-test-suites.sh
    echo ""
    echo "{{GREEN}}Parallel tests completed! Collecting results...{{NC}}"
    bash scripts/collect-parallel-results.sh

# Collect and aggregate results from parallel tests
@test-parallel-results:
    echo "{{BLUE}}Aggregating parallel test results...{{NC}}"
    bash scripts/collect-parallel-results.sh
    echo "{{GREEN}}✓ Results aggregated{{NC}}"

# Clean up all parallel test clusters and temp files
@test-parallel-cleanup:
    #!/bin/bash
    echo -e "\033[0;34mCleaning up parallel test resources...\033[0m"
    bash scripts/cleanup-parallel.sh

# Show parallel test results
@test-parallel-report:
    echo "{{BLUE}}Parallel Test Results{{NC}}"
    bash -c 'if [ -f target/parallel-test-results/AGGREGATED_RESULTS.md ]; then cat target/parallel-test-results/AGGREGATED_RESULTS.md; else echo "No parallel test results found. Run just test-parallel first."; fi'

# ============================================================================
# BENCHMARK COMPARISON: upstream Cilium vs seriousum (Rust)
# ============================================================================

# Create a fresh kind cluster ready for ginkgo tests using the proper CNI setup script.
# Deletes any existing cluster with the same name first.
# agent_port_prefix/operator_port_prefix avoid debug-port collisions when multiple clusters exist.
# Usage: just ginkgo-cluster cilium-ginkgo
# Usage: just ginkgo-cluster cilium-upstream 236 237
ginkgo-cluster cluster='cilium-ginkgo' agent_port_prefix='234' operator_port_prefix='235':
    #!/usr/bin/env bash
    set -euo pipefail
    ROOT="$(git rev-parse --show-toplevel)"
    mkdir -p "$ROOT/target/cilium-kind"
    KUBECONF="$ROOT/target/cilium-kind/{{cluster}}.kubeconfig"
    kind delete cluster --name {{cluster}} 2>/dev/null || true
    AGENTPORTPREFIX={{agent_port_prefix}} OPERATORPORTPREFIX={{operator_port_prefix}} \
        bash "{{CILIUM_REPO}}/contrib/scripts/kind.sh" 1 1 {{cluster}} kindest/node:v1.33.1 iptables dual "" "" "$KUBECONF"
    echo "Cluster {{cluster}} ready — kubeconfig: $KUBECONF"

# Pull and load upstream Cilium CI images into an existing kind cluster.
# These are the reference images used for benchmark comparisons.
# NOTE: Skips hubble-relay-ci which is small enough to pull live (IfNotPresent).
# Usage: just load-upstream cilium-ginkgo
load-upstream cluster=KIND_CLUSTER:
    #!/usr/bin/env bash
    set -euo pipefail
    # kind load docker-image fails on multi-arch BuildKit images; use docker save|ctr import instead.
    for img in "{{CILIUM_CI_AGENT}}:{{CILIUM_CI_TAG}}" "{{CILIUM_CI_OPERATOR}}"; do
        echo "Pulling $img (linux/amd64)..."
        DOCKER_BUILDKIT=0 docker pull --platform linux/amd64 "$img"
        for node in $(kind get nodes --name {{cluster}}); do
            echo "  importing $img into $node..."
            docker save "$img" | docker exec -i "$node" ctr --namespace=k8s.io images import -
        done
    done
    echo "Done — upstream CI images loaded into {{cluster}}"

# Run ginkgo against an EXISTING cluster using upstream Cilium CI images.
# This is the benchmark baseline — compare results against `just run-existing`
# which runs the same suite against the seriousum Rust agent.
# Usage: just run-upstream cilium-ginkgo K8sAgentChaosTest
run-upstream cluster=KIND_CLUSTER focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT:
    #!/usr/bin/env bash
    set -euo pipefail
    ROOT="$(git rev-parse --show-toplevel)"
    AGENT_IMG="{{CILIUM_CI_AGENT}}"
    AGENT_TAG="{{CILIUM_CI_TAG}}"
    OPERATOR_IMG="{{CILIUM_CI_OPERATOR}}"
    HUBBLE_IMG="{{CILIUM_CI_HUBBLE_RELAY}}"
    HUBBLE_REPO="${HUBBLE_IMG%:*}"
    HUBBLE_TAG="${HUBBLE_IMG##*:}"
    mkdir -p "$ROOT/target/cilium-kind"
    KUBECONF="$ROOT/target/cilium-kind/{{cluster}}.kubeconfig"
    kind get kubeconfig --name {{cluster}} > "$KUBECONF"
    export KUBECONFIG="$KUBECONF"
    export PATH="$ROOT/target/cilium-dropin:$PATH"
    export CNI_INTEGRATION=kind
    export INTEGRATION_TESTS=true
    # Remove orphaned non-Helm envoy objects from interrupted local runs; this
    # keeps baseline installs aligned with CI's clean-cluster behavior.
    kubectl -n kube-system delete serviceaccount cilium-envoy --ignore-not-found >/dev/null 2>&1 || true
    kubectl -n kube-system delete configmap cilium-envoy-config --ignore-not-found >/dev/null 2>&1 || true
    K8S_VER=$(kubectl version -o json 2>/dev/null \
        | python3 -c "import sys,json; v=json.load(sys.stdin)['serverVersion']; print(v['major']+'.'+v['minor'])" \
        2>/dev/null || echo "1.33")
    export K8S_VERSION="$K8S_VER"
    # operator.image.override bypasses the Helm chart cloud-suffix logic (same as run-existing).
    # Agent + operator use Never (pre-loaded); hubble-relay uses IfNotPresent (pulls from quay.io).
    HELM_OVERRIDES="image.repository=$AGENT_IMG,image.tag=$AGENT_TAG,image.useDigest=false,image.pullPolicy=Never,operator.image.override=$OPERATOR_IMG,operator.image.pullPolicy=Never,hubble.relay.image.repository=$HUBBLE_REPO,hubble.relay.image.tag=$HUBBLE_TAG,hubble.relay.image.useDigest=false,hubble.relay.image.pullPolicy=IfNotPresent"
    echo "Running UPSTREAM CILIUM focus='{{focus}}' against cluster='{{cluster}}' (k8s $K8S_VER)"
    echo "  Agent:    $AGENT_IMG:$AGENT_TAG"
    echo "  Operator: $OPERATOR_IMG"
    cd {{CILIUM_REPO}}/test
    timeout --preserve-status --kill-after=5m {{timeout}} \
        ./test.test \
            --ginkgo.focus="{{focus}}" \
            --ginkgo.v \
            -- \
            -cilium.testScope=k8s \
            -cilium.kubeconfig="$KUBECONF" \
            -cilium.install-helm-overrides="$HELM_OVERRIDES" \
            -cilium.holdEnvironment=false

# Recreate cluster + load upstream CI images + run baseline focus.
# Usage: just run-upstream-fresh cilium-upstream K8sAgentChaosTest
run-upstream-fresh cluster='cilium-upstream' focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT agent_port_prefix='236' operator_port_prefix='237':
    just ginkgo-cluster {{cluster}} {{agent_port_prefix}} {{operator_port_prefix}}
    just load-upstream {{cluster}}
    just run-upstream {{cluster}} {{focus}} {{timeout}}

# Run a test in the background in a detached session (immune to SIGHUP / terminal close).
# Logs go to target/bg-<focus>-<timestamp>.log
# Usage: just run-bg cilium-ginkgo K8sAgentChaosTest
# Usage: just run-bg cilium-ginkgo K8sAgentChaosTest 30m upstream
run-bg cluster=KIND_CLUSTER focus='K8sAgentChaosTest' timeout=TEST_TIMEOUT variant='seriousum':
    #!/usr/bin/env bash
    set -euo pipefail
    ROOT="$(git rev-parse --show-toplevel)"
    TS=$(date +%Y%m%d-%H%M%S)
    LOGFILE="$ROOT/target/bg-{{variant}}-{{focus}}-$TS.log"
    mkdir -p "$ROOT/target"
    RECIPE="run-existing"
    if [ "{{variant}}" = "upstream" ]; then RECIPE="run-upstream"; fi
    echo "Launching $RECIPE {{focus}} in background (setsid)..."
    echo "Log: $LOGFILE"
    setsid just "$RECIPE" {{cluster}} {{focus}} {{timeout}} > "$LOGFILE" 2>&1 &
    echo "PID: $! — tail -f $LOGFILE"
