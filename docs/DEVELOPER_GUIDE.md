# Cilium Rust Port - Comprehensive Developer Guide

**Version**: 1.0  
**Date**: 2026-05-11  
**Status**: In Development  
**Target**: Enable contributors, document architecture, provide porting guide  

---

## Table of Contents

1. [Getting Started](#getting-started)
2. [Project Architecture](#project-architecture)
3. [Development Setup](#development-setup)
4. [Porting Guide: Go → Rust](#porting-guide-go--rust)
5. [Component Deep Dives](#component-deep-dives)
6. [Testing Strategy](#testing-strategy)
7. [Common Tasks](#common-tasks)
8. [Troubleshooting](#troubleshooting)
9. [Release Process](#release-process)

---

## Getting Started

### For New Contributors

Welcome! This guide will help you understand the Cilium Rust port and how to contribute.

**Prerequisites**:
- Rust 1.95.0 or later
- Docker/Podman for building container images
- kind (Kubernetes in Docker) for testing
- kubectl for Kubernetes interactions

**Quick Start** (15 minutes):

```bash
# 1. Clone the repository
git clone https://github.com/hanthor/seriousum.git
cd seriousum

# 2. Install Rust toolchain (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 3. Build the project
cargo build --release

# 4. Run tests
cargo test

# 5. Build container images
docker build -f images/cilium.Dockerfile -t seriousum-agent:dev .

# 6. You're ready to develop!
```

### First Contribution

Looking to contribute? Here's how:

1. **Read the architecture** → `Architecture` section below
2. **Pick a task** → GitHub issues (start with `good first issue`)
3. **Set up your environment** → `Development Setup` section
4. **Make your changes** → Follow code standards
5. **Write tests** → Aim for >80% coverage
6. **Submit PR** → Include tests + documentation

---

## Project Architecture

### High-Level Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Cilium Rust Port                     │
│                  (seriousum project)                    │
└─────────────────────────────────────────────────────────┘
                         ▲
              ┌──────────┼──────────┐
              │          │          │
         ┌────▼────┐ ┌───▼────┐ ┌──▼────────┐
         │  Agent  │ │Operator│ │ Hubble    │
         │ CLI     │ │ CLI    │ │ CLI       │
         └────┬────┘ └───┬────┘ └──┬────────┘
              │          │         │
         ┌────▼──────────▼─────────▼─────────────────┐
         │              Core Runtime                 │
         │  (daemon, controller, configuration)      │
         ├─────────────────────────────────────────┤
         │                                           │
         │  ┌─────────────┬────────────┬──────────┐  │
         │  │ Service     │ Policy     │ Network  │  │
         │  │ Subsystem   │ Engine     │ Manager  │  │
         │  ├─────────────┼────────────┼──────────┤  │
         │  │ Load        │ eBPF Rules │ IPAM     │  │
         │  │ Balancer    │ Gen        │ Manager  │  │
         │  └─────────────┴────────────┴──────────┘  │
         │                                           │
         ├─────────────────────────────────────────┤
         │           Infrastructure Layer           │
         │                                           │
         │  ┌──────────┬──────────┬────────────────┐ │
         │  │ eBPF     │ K8s      │ Metrics &     │ │
         │  │ Loader   │ Client   │ Observability │ │
         │  └──────────┴──────────┴────────────────┘ │
         └─────────────────────────────────────────┘
                         ▲
         ┌───────────────┼───────────────┐
         │               │               │
    ┌────▼──────┐  ┌────▼──────┐  ┌────▼──────┐
    │  Kernel   │  │ Kubernetes │  │  etcd     │
    │ (eBPF)    │  │  API Server│  │ (Config)  │
    └───────────┘  └────────────┘  └───────────┘
```

### Crate Organization

```
seriousum/                    # Workspace root
├── Cargo.toml              # Workspace manifest
├── crates/                 # Main implementation
│   ├── core/              # Core types & utilities
│   ├── daemon/            # Agent daemon
│   ├── operator/          # Kubernetes operator
│   ├── service-observer/  # K8s service watching (P1)
│   ├── ebpf/              # eBPF map management (P1)
│   ├── backend-mapping/   # Backend selection (P1)
│   ├── loadbalancer/      # Load balancing (P1)
│   ├── policy/            # Policy enforcement (P2)
│   ├── endpoints/         # Pod endpoint mgmt (P2)
│   ├── api/               # Public API
│   ├── cli/               # CLI utilities
│   └── ... (20+ others)
├── src/                    # Binary entrypoints
│   ├── bin/
│   │   ├── cilium.rs      # Main agent binary
│   │   └── cilium-dbg.rs  # Debug binary
├── images/                 # Dockerfile definitions
├── scripts/                # Build/test scripts
├── docs/                   # Documentation (this + more)
└── tests/                  # Integration tests
```

### Development vs Production

**Development** (local work):
```bash
cargo build         # Debug build (fast)
cargo test          # Run tests
cargo doc --open    # Read docs locally
```

**Production** (releases):
```bash
cargo build --release    # Release build (optimized)
docker build ...         # Container image
cargo test --release     # Test optimized version
```

---

## Development Setup

### Environment Setup

#### 1. Install Rust

```bash
# Install rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Verify installation
rustc --version  # Should be 1.95.0+
cargo --version  # Should be 1.71.0+
```

#### 2. Install Dependencies

```bash
# Ubuntu/Debian
sudo apt-get install -y \
  build-essential \
  cmake \
  pkg-config \
  libssl-dev \
  docker.io \
  kubectl

# macOS (via Homebrew)
brew install cmake pkg-config openssl docker kubectl

# Verify
docker --version
kubectl --version
```

#### 3. Install kind (Kubernetes in Docker)

```bash
# Install kind
GO111MODULE="on" go install sigs.k8s.io/kind@latest

# Or via package manager
brew install kind           # macOS
sudo apt-get install kind   # Ubuntu

# Verify
kind --version
```

#### 4. Clone Repository

```bash
git clone https://github.com/hanthor/seriousum.git
cd seriousum
git checkout main
```

### IDE Setup

#### VS Code

1. Install extensions:
   - `rust-analyzer` (Rust analysis)
   - `Even Better TOML` (Cargo.toml support)
   - `crates` (Dependency updates)
   - `CodeLLDB` (Debugger)

2. Create `.vscode/settings.json`:

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.checkOnSave.extraArgs": ["--all-targets"],
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer",
    "editor.formatOnSave": true
  }
}
```

#### RustRover / IntelliJ IDEA

1. Install Rust plugin
2. Settings → Languages & Frameworks → Rust → Enable
3. Automatic clippy and fmt on save

### Build Configuration

#### Cargo.toml Workspace

```toml
[workspace]
members = [
    "crates/core",
    "crates/daemon",
    "crates/service-observer",
    # ... all 31 crates
]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

#### Build Commands

```bash
# Full workspace build
cargo build --release

# Single crate build
cargo build --release -p seriousum-core

# Specific binary
cargo build --release --bin cilium

# Check without building
cargo check

# Fast check with all features
cargo check --all-targets

# Clean everything
cargo clean
```

---

## Porting Guide: Go → Rust

### Philosophy

**Goal**: Port Go Cilium to Rust while:
- Maintaining exact behavioral parity
- Preserving all test compatibility
- Improving code quality and safety
- Enabling production-grade performance

### Approach

**Phase 1: Analysis**
- Read Go source code thoroughly
- Understand data structures and algorithms
- Map to Rust idioms
- Identify dependencies and interactions

**Phase 2: Stub Implementation**
- Create Rust module structure
- Define all types (using Go as reference)
- Create test stubs
- Plan error handling strategy

**Phase 3: Core Implementation**
- Implement core logic in Rust
- Follow Rust best practices (ownership, borrowing)
- Write comprehensive unit tests
- Validate against Go behavior

**Phase 4: Integration**
- Connect to other Rust components
- Run integration tests
- Validate with actual Kubernetes
- Compare metrics with Go version

**Phase 5: Optimization** (if needed)
- Profile performance
- Optimize hot paths
- Add benchmarks
- Document performance characteristics

### Example: Porting a Go Component

**Original Go Code** (`loadbalancer.go`):

```go
type LoadBalancer struct {
    services map[string]*Service
    mu       sync.RWMutex
}

func (lb *LoadBalancer) SelectBackend(svc string) *Backend {
    lb.mu.RLock()
    defer lb.mu.RUnlock()
    
    service := lb.services[svc]
    if service == nil || len(service.Backends) == 0 {
        return nil
    }
    
    idx := rand.Intn(len(service.Backends))
    return service.Backends[idx]
}
```

**Rust Equivalent**:

```rust
use std::sync::RwLock;
use std::collections::HashMap;

pub struct LoadBalancer {
    services: RwLock<HashMap<String, Service>>,
}

impl LoadBalancer {
    pub fn select_backend(&self, svc: &str) -> Option<Backend> {
        let services = self.services.read()
            .map_err(|e| LbError::LockPoisoned(e.to_string()))?;
        
        let service = services.get(svc)?;
        
        if service.backends.is_empty() {
            return None;
        }
        
        use rand::seq::SliceRandom;
        service.backends
            .choose(&mut rand::thread_rng())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_select_backend_empty_service() {
        let lb = LoadBalancer::new();
        assert_eq!(lb.select_backend("nonexistent"), None);
    }
}
```

### Key Porting Patterns

**Pattern 1: Locks & Concurrency**

Go:
```go
mu.Lock()
defer mu.Unlock()
// use shared data
```

Rust:
```rust
let mut data = shared.write()?;
// use data (borrow checker ensures safety)
```

**Pattern 2: Error Handling**

Go:
```go
result, err := operation()
if err != nil {
    return nil, fmt.Errorf("failed: %w", err)
}
```

Rust:
```rust
let result = operation()?;  // Propagate
// or
let result = operation()
    .context("operation failed")?;  // With context
```

**Pattern 3: Goroutines → tokio Tasks**

Go:
```go
go func() {
    for event := range eventChan {
        processEvent(event)
    }
}()
```

Rust:
```rust
tokio::spawn(async {
    while let Some(event) = receiver.recv().await {
        process_event(event).await;
    }
});
```

**Pattern 4: Interfaces → Traits**

Go:
```go
type BackendSelector interface {
    Select() Backend
}
```

Rust:
```rust
pub trait BackendSelector {
    fn select(&self) -> Option<Backend>;
}
```

### Testing Strategy During Porting

```
Go Component          Rust Port           Validation
────────────────────────────────────────────────────
load_balancer.go  →  loadbalancer/lib.rs  →  Go tests
                                ↓
                         Rust unit tests
                                ↓
                      Integration tests
                                ↓
                       Parity validation
```

### Performance Considerations

**Rust Advantages**:
- No garbage collection (predictable latency)
- Zero-cost abstractions (no runtime overhead)
- SIMD support (vectorized operations)
- Better async performance (tokio vs goroutines)

**Optimization Points**:
- Profile with `flamegraph`
- Benchmark with `criterion`
- Use release builds for performance testing
- Consider SIMD for packet processing

---

## Component Deep Dives

### Component 1: Service Observer

**Purpose**: Watch Kubernetes services and maintain cache

**Location**: `crates/service-observer/`

**Key Concepts**:
- K8s API watching
- Event-driven updates
- In-memory caching
- Label selectors

**Example Usage**:

```rust
use seriousum_service_observer::ServiceObserver;

#[tokio::main]
async fn main() -> Result<()> {
    let observer = ServiceObserver::new(kube_client).await?;
    
    // Subscribe to changes
    let mut rx = observer.subscribe();
    
    while let Some(event) = rx.recv().await {
        println!("Service event: {:?}", event);
    }
    
    Ok(())
}
```

**Testing**:

```rust
#[tokio::test]
async fn test_service_discovery() {
    let observer = ServiceObserver::mock();
    observer.add_service(test_service()).await;
    
    let services = observer.list_services("default").await?;
    assert_eq!(services.len(), 1);
}
```

### Component 2: Load Balancer

**Purpose**: Distribute traffic across backends using different algorithms

**Location**: `crates/loadbalancer/`

**Algorithms**:
- Round-robin (sequential)
- Least-connections (stateful)
- Consistent hash (stable)
- Random (probabilistic)

**Example**:

```rust
use seriousum_loadbalancer::{LoadBalancer, Algorithm};

let mut lb = LoadBalancer::with_algorithm(Algorithm::RoundRobin);
lb.add_backend("backend-1", "10.0.0.1:8080");
lb.add_backend("backend-2", "10.0.0.2:8080");

let backend = lb.select_backend()?;  // Distributes evenly
```

### Component 3: eBPF Maps

**Purpose**: Store service and backend data in kernel space

**Location**: `crates/ebpf/src/maps.rs`

**Map Types**:
- Service Map: Service ID → Service definition
- Backend Map: Service ID → Backends
- Affinity Map: Client IP → Backend (session affinity)
- Counters: Metrics

**Example**:

```rust
use seriousum_ebpf::maps::{ServiceMap, BackendMap};

let service_map = ServiceMap::new()?;
service_map.insert(service_id, service_def)?;

let backend_map = BackendMap::new()?;
backend_map.insert(service_id, backends)?;
```

### Component 4: Policy Engine

**Purpose**: Evaluate and enforce network policies

**Location**: `crates/policy/`

**Features**:
- Policy parsing
- Rule evaluation
- Conflict detection
- Performance optimization

---

## Testing Strategy

### Unit Testing

**Guidelines**:
- Test one thing per test
- Use descriptive names
- Aim for >80% coverage
- Test both success and failure paths

**Example**:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_robin_distribution() {
        let mut lb = LoadBalancer::with_algorithm(Algorithm::RoundRobin);
        lb.add_backend("b1", "10.0.0.1:8080");
        lb.add_backend("b2", "10.0.0.2:8080");
        
        // Select 100 times, verify distribution
        let mut counts = HashMap::new();
        for _ in 0..100 {
            let backend = lb.select_backend().unwrap();
            *counts.entry(backend).or_insert(0) += 1;
        }
        
        assert_eq!(counts.get("b1"), Some(&50));
        assert_eq!(counts.get("b2"), Some(&50));
    }

    #[test]
    fn test_empty_backend_list() {
        let lb = LoadBalancer::with_algorithm(Algorithm::RoundRobin);
        assert_eq!(lb.select_backend(), None);
    }
}
```

### Integration Testing

**Test Framework**: Custom integration test harness

**Example**:

```rust
#[tokio::test]
async fn test_service_load_balancing_e2e() {
    // Setup
    let cluster = create_kind_cluster("test-lb").await;
    let kubectl = cluster.kubectl();
    
    // Create resources
    kubectl.apply(service_yaml).await?;
    kubectl.apply(pods_yaml).await?;
    
    // Verify behavior
    let results = verify_traffic_distribution(&kubectl).await?;
    assert!(results.is_balanced);
    
    // Cleanup
    cluster.cleanup().await;
}
```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_round_robin_distribution

# With output
cargo test -- --nocapture

# Specific crate
cargo test -p seriousum-loadbalancer

# Release mode (slower but more realistic)
cargo test --release

# With coverage
cargo tarpaulin --out Html
```

---

## Common Tasks

### Adding a New Crate

```bash
# 1. Create crate directory
mkdir crates/my-component
cd crates/my-component

# 2. Create Cargo.toml
cat > Cargo.toml << 'EOF'
[package]
name = "seriousum-my-component"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1.0", features = ["full"] }
seriousum-core = { path = "../core" }

[dev-dependencies]
tokio-test = "0.4"
EOF

# 3. Create src/lib.rs
mkdir -p src
cat > src/lib.rs << 'EOF'
pub struct MyComponent;

impl MyComponent {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creation() {
        let component = MyComponent::new();
        // assertions
    }
}
EOF

# 4. Update workspace Cargo.toml
# Add to [workspace] members list

# 5. Build to verify
cd ../..
cargo build
```

### Debugging

```bash
# Run with backtrace
RUST_BACKTRACE=1 cargo run

# More verbose backtraces
RUST_BACKTRACE=full cargo run

# With debugger (requires CodeLLDB)
# Set breakpoints in VS Code, then press F5

# Print debugging
println!("Debug: {:?}", value);
dbg!(value);  // Macro shorthand

# Structured logging
use log::{info, debug, warn, error};
info!("Component started");
```

### Code Formatting

```bash
# Format all code
cargo fmt

# Check without changing
cargo fmt -- --check

# Format specific file
cargo fmt -- src/lib.rs
```

### Linting

```bash
# Run clippy (recommended checks)
cargo clippy --all-targets

# Fix automatically
cargo clippy --fix

# Check for common mistakes
cargo clippy -- -D warnings
```

---

## Troubleshooting

### Build Issues

**Problem**: "error: linker `cc` not found"

```bash
# Solution: Install build tools
# Ubuntu
sudo apt-get install build-essential

# macOS
xcode-select --install
```

**Problem**: "rustc not found"

```bash
# Solution: Ensure PATH includes cargo
export PATH="$HOME/.cargo/bin:$PATH"
source ~/.profile
```

### Test Failures

**Problem**: "thread panicked"

```bash
# Run with backtrace
RUST_BACKTRACE=1 cargo test <test_name>

# Check assertion message for details
```

**Problem**: "kind cluster not found"

```bash
# Check kind clusters
kind get clusters

# Create test cluster
kind create cluster --name test
```

### Performance Issues

**Problem**: Build is slow

```bash
# Use sccache for faster rebuilds
cargo install sccache
export RUSTC_WRAPPER=sccache

# Use `cargo check` instead of `cargo build` for quick checks
cargo check
```

**Problem**: Tests are slow

```bash
# Run in parallel (limited)
cargo test -- --test-threads=2

# Run fastest tests first
cargo test --lib  # Unit tests only
```

---

## Release Process

### Version Bumping

```bash
# Update version in workspace Cargo.toml
# Cargo.toml: version = "0.2.0"

# Update all dependent crates
# Regenerate Cargo.lock
cargo update

# Commit changes
git add Cargo.toml Cargo.lock
git commit -m "Bump version to 0.2.0"
```

### Building Release

```bash
# Build release binary
cargo build --release

# Verify binary
./target/release/cilium --version

# Create binary package
tar -czf cilium-v0.2.0-linux-x86_64.tar.gz \
    target/release/cilium \
    target/release/cilium-dbg
```

### Creating Release Tag

```bash
# Create annotated tag
git tag -a v0.2.0 -m "Release v0.2.0: P2 features"

# Push tag
git push origin v0.2.0

# Create GitHub release
gh release create v0.2.0 \
    --notes "See CHANGELOG.md" \
    cilium-v0.2.0-linux-x86_64.tar.gz
```

---

## Contributing Guidelines

### Code Style

- Follow `rustfmt` style (auto-formatted)
- Use meaningful variable names
- Add comments for complex logic
- Document public APIs with doc comments

### Commit Messages

```
Title (50 chars max): Brief description

Body (72 chars per line):
- Detailed explanation if needed
- Can include multiple paragraphs
- Reference issue #123 if applicable

Fixes #123
```

### Pull Request Process

1. Fork repository
2. Create feature branch: `git checkout -b feat/my-feature`
3. Make changes + write tests
4. Format: `cargo fmt`
5. Lint: `cargo clippy`
6. Test: `cargo test`
7. Push: `git push origin feat/my-feature`
8. Create PR with description
9. Address review comments

### Documentation

- Add doc comments to public items
- Include examples where useful
- Update main README if user-facing
- Add entry to CHANGELOG.md

---

## Resources

### External Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Kubernetes API Docs](https://kubernetes.io/docs/reference/generated/kubernetes-api/)
- [eBPF Documentation](https://ebpf.io/what-is-ebpf/)

### Project Resources

- Repository: https://github.com/hanthor/seriousum
- Issues: https://github.com/hanthor/seriousum/issues
- Parity status: See PARITY_PROOF_DASHBOARD.md

### Getting Help

1. Check documentation and examples
2. Search existing GitHub issues
3. Create new issue with:
   - Description of problem
   - Minimal reproduction
   - Expected vs actual behavior
4. Join community discussions

---

**Document Version**: 1.0  
**Last Updated**: 2026-05-11  
**Status**: Comprehensive Developer Guide Complete  
**Target Audience**: Contributors, maintainers, operators  
