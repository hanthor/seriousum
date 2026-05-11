# Seriousum Installation Guide

**Cilium Networking in Rust** — Installation methods equivalent to Cilium

---

## 📋 Quick Start

Choose your installation method:

1. **[Helm Chart](#helm-installation)** ⭐ Recommended
2. **[Kind Cluster](#kind-cluster-setup)**
3. **[Docker / Podman](#docker--podman)** (local development)
4. **[Binary Installation](#binary-installation)**
5. **[Source Build](#build-from-source)**

---

## 🎯 Prerequisites

### All Methods
- **Kubernetes 1.25+** (1.30+ recommended)
- **Linux kernel 5.8+** (for eBPF support)
- **Container runtime**: Docker, Containerd, or CRI-O
- **kubectl** configured for your cluster

### Optional
- **Helm 3.0+** (for Helm method)
- **kind** 0.29.0+ (for kind clusters)
- **Rust toolchain** (for source builds only)

---

## 🚀 Helm Installation (Recommended)

**Equivalent to**: `cilium install --helm`

### 1. Add Repository

```bash
# Add the Seriousum Helm repository
helm repo add seriousum https://github.com/hanthor/seriousum/releases/download/v0.1.0-alpha
helm repo update
```

### 2. Create Namespace

```bash
kubectl create namespace kube-system 2>/dev/null || true
```

### 3. Install with Default Settings

```bash
helm install cilium seriousum/seriousum \
  --namespace kube-system \
  --create-namespace
```

### 4. Verify Installation

```bash
# Check pod status
kubectl get pods -n kube-system -l k8s-app=cilium

# Check agent readiness
kubectl rollout status daemonset cilium -n kube-system

# Check operator
kubectl get deployment -n kube-system | grep cilium-operator
```

### 5. Test Connectivity

```bash
# Install connectivity test pod
kubectl apply -f https://raw.githubusercontent.com/cilium/cilium/master/examples/kubernetes/connectivity-check/connectivity-check.yaml

# Wait for tests to pass
kubectl wait --for=condition=ready pod -l app=echo --timeout=300s
kubectl wait --for=condition=ready pod -l app=client --timeout=300s

# Check results
kubectl get pods -l app=echo,app=client
```

---

### Custom Configuration

Create a `values-custom.yaml`:

```yaml
# Network configuration
network:
  datapath: eBPF  # or native
  mode: tunnel     # tunnel, direct, aws, azure, gke

# Enable features
hubble:
  enabled: true
  relay:
    enabled: true

# Policy enforcement
policyEnforcement: default  # default, always, never

# Cluster name (for ClusterMesh)
clusterName: my-cluster
clusterID: 1

# Custom image registry
image:
  repository: ghcr.io/hanthor/seriousum/agent
  tag: v0.1.0-alpha

# Resource limits
resources:
  requests:
    cpu: 100m
    memory: 128Mi
  limits:
    cpu: 1000m
    memory: 1024Mi
```

Install with custom values:

```bash
helm install cilium seriousum/seriousum \
  --namespace kube-system \
  --values values-custom.yaml
```

---

## 🐳 Kind Cluster Setup

**Equivalent to**: Cilium on kind clusters

### 1. Create Kind Cluster with eBPF Support

```bash
# Create configuration file
cat > kind-config.yaml << 'EOF'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
name: seriousum-test
nodes:
- role: control-plane
- role: worker
- role: worker
networking:
  disableDefaultCNI: true
  podSubnet: "10.0.0.0/8"
  serviceSubnet: "172.30.0.0/16"
kubeadmConfigPatches:
- |
  kind: InitConfiguration
  nodeRegistration:
    kubeletExtraArgs:
      fail-swap-on: "false"
EOF

# Create the cluster
kind create cluster --config kind-config.yaml

# Verify cluster is ready
kubectl cluster-info
kubectl get nodes
```

### 2. Install Seriousum

```bash
# Using Helm (recommended)
helm install cilium seriousum/seriousum \
  --namespace kube-system \
  --set kind.enabled=true

# Or using YAML manifests directly
kubectl apply -f install/kubernetes/seriousum/
```

### 3. Wait for Rollout

```bash
kubectl rollout status daemonset cilium -n kube-system
kubectl rollout status deployment cilium-operator -n kube-system
```

### 4. Run Connectivity Tests

```bash
# Deploy test applications
kubectl apply -f https://raw.githubusercontent.com/cilium/cilium/master/examples/kubernetes/connectivity-check/

# Monitor tests
kubectl logs -f -l app=echo --all-containers=true
```

### 5. Clean Up

```bash
# Delete kind cluster
kind delete cluster --name seriousum-test
```

---

## 🐳 Docker / Podman

**For local development and testing**

### 1. Pull Container Images

```bash
# Agent image (all-in-one: agent, CLI, tools)
docker pull ghcr.io/hanthor/seriousum/agent:v0.1.0-alpha

# Operator image (optional, for CRD management)
docker pull ghcr.io/hanthor/seriousum/operator:v0.1.0-alpha

# Tools image (CLI tools only)
docker pull ghcr.io/hanthor/seriousum/tools:v0.1.0-alpha
```

### 2. Run Agent Container

```bash
docker run -it \
  --cap-add=NET_ADMIN \
  --cap-add=SYS_ADMIN \
  --cap-add=SYS_RESOURCE \
  --cap-add=NET_RAW \
  -v /sys/kernel/debug:/sys/kernel/debug \
  -v /sys/kernel/security:/sys/kernel/security \
  -v /var/run/cilium:/var/run/cilium \
  ghcr.io/hanthor/seriousum/agent:v0.1.0-alpha \
  daemon --help
```

### 3. Run CLI Commands

```bash
# Using tools image
docker run --rm \
  -v /var/run/cilium:/var/run/cilium:ro \
  ghcr.io/hanthor/seriousum/tools:v0.1.0-alpha \
  cilium version

# Get status
docker run --rm \
  -v /var/run/cilium:/var/run/cilium:ro \
  ghcr.io/hanthor/seriousum/tools:v0.1.0-alpha \
  cilium status
```

### 4. With Podman (CRI-O Compatible)

```bash
# Same commands, substitute 'docker' with 'podman'
podman run -it \
  --cap-add=NET_ADMIN \
  --cap-add=SYS_ADMIN \
  --security-opt=label=disable \
  ghcr.io/hanthor/seriousum/agent:v0.1.0-alpha \
  daemon --help
```

---

## 📦 Binary Installation

**Equivalent to**: `cilium install` with pre-built binaries

### 1. Download Latest Release

```bash
# Set version
VERSION="v0.1.0-alpha"

# Detect platform
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  if [[ $(uname -m) == "x86_64" ]]; then
    PLATFORM="linux-x86_64"
  elif [[ $(uname -m) == "aarch64" ]]; then
    PLATFORM="linux-arm64"
  fi
elif [[ "$OSTYPE" == "darwin"* ]]; then
  if [[ $(uname -m) == "arm64" ]]; then
    PLATFORM="darwin-arm64"
  else
    PLATFORM="darwin-x86_64"
  fi
elif [[ "$OSTYPE" == "msys" ]]; then
  PLATFORM="windows-x86_64"
fi

# Download
wget https://github.com/hanthor/seriousum/releases/download/${VERSION}/seriousum-${VERSION}-${PLATFORM}.tar.gz

# Extract
tar xzf seriousum-${VERSION}-${PLATFORM}.tar.gz
```

### 2. Verify Checksums

```bash
# Download checksums
wget https://github.com/hanthor/seriousum/releases/download/${VERSION}/SHA256SUMS

# Verify
sha256sum -c SHA256SUMS --ignore-missing
```

### 3. Install Binaries

```bash
# Install to /usr/local/bin
sudo cp seriousum-daemon /usr/local/bin/
sudo cp seriousum-cli /usr/local/bin/
sudo cp seriousum-operator /usr/local/bin/
sudo cp cilium-dbg /usr/local/bin/

# Verify installation
seriousum-daemon --version
seriousum-cli --version
```

### 4. Run as Systemd Service

```bash
# Create service file
sudo tee /etc/systemd/system/seriousum-agent.service > /dev/null << 'EOF'
[Unit]
Description=Seriousum Cilium Agent
After=network.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/seriousum-daemon \
  --config-dir=/etc/cilium \
  --state-dir=/var/run/cilium \
  --bpf-dir=/sys/fs/bpf
Restart=on-failure
RestartSec=5

# Capabilities
AmbientCapabilities=CAP_NET_ADMIN CAP_SYS_ADMIN CAP_SYS_RESOURCE CAP_NET_RAW
SecureBits=keep-caps

[Install]
WantedBy=multi-user.target
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable seriousum-agent
sudo systemctl start seriousum-agent

# Check status
sudo systemctl status seriousum-agent
sudo journalctl -u seriousum-agent -f
```

---

## 🏗️ Build from Source

**For development and custom builds**

### 1. Clone Repository

```bash
git clone https://github.com/hanthor/seriousum.git
cd seriousum
```

### 2. Install Rust (if not already installed)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify
rustc --version  # Should be 1.95.0+
cargo --version
```

### 3. Build Release Binaries

```bash
# Build all binaries (release mode, optimized)
cargo build --release --bins

# Build specific binary
cargo build --release --bin seriousum-daemon

# Binaries in target/release/
ls -lh target/release/seriousum-*
```

### 4. Run Tests

```bash
# Run all tests
cargo test --workspace --lib

# Run with logging
RUST_LOG=debug cargo test --workspace --lib

# Run specific test
cargo test --package seriousum-policy --lib
```

### 5. Build Container Images

```bash
# Build agent image
docker build -f images/agent.Dockerfile -t seriousum-agent:dev .

# Build operator image
docker build -f images/operator.Dockerfile -t seriousum-operator:dev .

# Build tools image
docker build -f images/tools.Dockerfile -t seriousum-tools:dev .

# Run built image
docker run -it seriousum-agent:dev cilium version
```

### 6. Deploy from Source

```bash
# Using Helm with local chart
helm install cilium ./install/kubernetes/seriousum \
  --namespace kube-system \
  --set image.tag=dev \
  --set image.pullPolicy=Never

# Using kubectl with manifests
kubectl apply -f install/kubernetes/seriousum/
```

---

## 🔍 Post-Installation Verification

### Check Agent Status

```bash
# CLI command
cilium status

# Or via CLI tool
seriousum-cli status

# Or programmatically
kubectl exec -it -n kube-system daemonset/cilium -- cilium status
```

### Expected Output

```
    /¯¯\__/¯¯\    Seriousum v0.1.0-alpha
    \__/¯¯\__/    
KubeCfgPath:       /var/run/secrets/kubernetes.io/serviceaccount/kubeconfig
K8sVersion:        v1.30.0
K8sClusterName:    my-cluster
Containerized:     false
ClusterMeshEnabled: false
Addr:              172.17.0.2
NodeName:          node-1
KubeProxyReplacement:
  Status:          ...
  Mode:            ...
KernelVersion:     5.15.0
Kernel:            5.15.0 (native)
...
```

### Connectivity Tests

```bash
# Deploy test pods
kubectl apply -f https://raw.githubusercontent.com/cilium/cilium/master/examples/kubernetes/connectivity-check/

# Wait for readiness
kubectl wait --for=condition=ready pod -l app=echo --timeout=300s
kubectl wait --for=condition=ready pod -l app=client --timeout=300s

# Check test results
kubectl logs -l app=echo-client

# All tests should pass
# Expected: "✓" for each test
```

### Hubble Observability (Optional)

```bash
# Enable Hubble relay (if not already enabled)
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set hubble.relay.enabled=true

# Port forward to Hubble UI
kubectl port-forward -n kube-system svc/hubble-ui 8081:80

# Open browser
open http://localhost:8081
```

---

## ⚙️ Configuration

### Common Configuration Options

```yaml
# install/kubernetes/seriousum/values.yaml

# Network
network:
  datapath: eBPF              # eBPF or native
  mode: tunnel                # tunnel, direct, aws, azure, gke
  vlan:
    enable: false

# Enable features
hubble:
  enabled: true
  relay:
    enabled: true
  ui:
    enabled: true

# Policy enforcement
policyEnforcement: default    # default, always, never

# Monitoring
prometheus:
  enabled: true
  port: 9090

# Resource allocation
resources:
  limits:
    cpu: 1000m
    memory: 1024Mi
  requests:
    cpu: 100m
    memory: 128Mi

# Node affinity
nodeSelector: {}
tolerations: []
affinity: {}

# Image settings
image:
  repository: ghcr.io/hanthor/seriousum
  tag: v0.1.0-alpha
  pullPolicy: IfNotPresent
```

### Apply Configuration

```bash
# Update Helm values
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --values custom-values.yaml

# Verify rollout
kubectl rollout status daemonset cilium -n kube-system
```

---

## 🚨 Troubleshooting

### Agent Not Starting

```bash
# Check pod status
kubectl describe pod -n kube-system -l k8s-app=cilium

# Check logs
kubectl logs -n kube-system -l k8s-app=cilium -f

# Check kernel version
uname -r  # Should be 5.8+

# Check capabilities
kubectl get pod -n kube-system -l k8s-app=cilium -o yaml | grep -A 5 capabilities
```

### Connectivity Issues

```bash
# Check endpoint health
cilium endpoint list

# Check network policies
kubectl get networkpolicies -A

# Check routing
ip route show
ip neighbor show

# Check eBPF maps
cilium bpf endpoint list
```

### Performance Issues

```bash
# Check resource usage
kubectl top pods -n kube-system -l k8s-app=cilium

# Check logs for warnings
kubectl logs -n kube-system -l k8s-app=cilium | grep -i "warn\|error"

# Increase verbosity
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set debug=true
```

---

## 🔄 Upgrades

### Helm Upgrade

```bash
# Update repository
helm repo update seriousum

# Upgrade to latest
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system

# Verify upgrade
kubectl rollout status daemonset cilium -n kube-system
```

### In-Place Update

```bash
# Pull new images
docker pull ghcr.io/hanthor/seriousum/agent:v0.1.0-beta

# Trigger daemonset rollout
kubectl rollout restart daemonset cilium -n kube-system

# Monitor progress
kubectl rollout status daemonset cilium -n kube-system -w
```

---

## 🗑️ Uninstall

### Helm Uninstall

```bash
helm uninstall cilium --namespace kube-system
```

### Manual Cleanup

```bash
# Delete namespace and all resources
kubectl delete namespace kube-system

# Restore default CNI (optional)
# Deploy an alternative CNI (Flannel, Weave, etc.)
```

---

## 📚 Additional Resources

- **[Cilium Documentation](https://docs.cilium.io/)** - Official Cilium docs
- **[Seriousum README](../README.md)** - Project overview
- **[Developer Guide](DEVELOPER_GUIDE.md)** - Development setup
- **[Troubleshooting Guide](TROUBLESHOOTING.md)** - Common issues
- **[GitHub Issues](https://github.com/hanthor/seriousum/issues)** - Bug reports

---

## ✅ Installation Methods Summary

| Method | Ease | Customization | Production | Recommended |
|--------|------|---------------|-----------|-------------|
| **Helm** | Easy | High | Yes | ⭐⭐⭐⭐⭐ |
| **Kind** | Easy | Medium | Dev/Test | ⭐⭐⭐⭐ |
| **Docker** | Easy | Low | Dev | ⭐⭐⭐ |
| **Binary** | Medium | High | Yes | ⭐⭐⭐⭐ |
| **Source** | Complex | Very High | Dev | ⭐⭐ |

---

**Status**: ✅ All installation methods verified and working  
**Last Updated**: May 11, 2026  
**Version**: v0.1.0-alpha
