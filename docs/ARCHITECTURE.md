# Seriousum Architecture

**System design and component overview for Seriousum Cilium**

---

## 🏗️ High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     KUBERNETES CLUSTER                          │
└─────────────────────────────────────────────────────────────────┘
         │                    │                    │
         ├─── CONTROL PLANE ──┼─── WORKER NODE 1 ─┤
         │                    │                    │
    ┌────▼─────────┐    ┌─────▼─────────┐    ┌────▼──────────┐
    │ Operator Pod │    │  Cilium Agent │    │  Cilium Agent│
    │ (DaemonSet)  │    │  (DaemonSet)  │    │ (DaemonSet)   │
    │              │    │               │    │               │
    │ • CRD Mgmt   │    │ • eBPF Loader │    │ • eBPF Loader │
    │ • Policy Sync│    │ • Policy Eval │    │ • Policy Eval │
    │ • Endpoint   │    │ • Datapath    │    │ • Datapath    │
    │   Tracking   │    │ • Observability     │ • Observability
    └──────────────┘    │ • Load Balancer    │ • Load Balancer
                        └────────────────┘    └───────────────┘
         │                    │                    │
         └────────┬───────────┴────────┬───────────┘
                  │                    │
              ┌───▼──────────────────────▼────┐
              │   HOST NETWORK INTERFACE      │
              │ • eBPF Programs (kernel)      │
              │ • BPF Maps (shared memory)    │
              │ • Ring Buffer (events)        │
              └────────────────────────────────┘
                          │
              ┌───────────▼────────────┐
              │    LINUX KERNEL        │
              │ • Netfilter Hooks      │
              │ • BPF VM               │
              │ • eBPF Programs        │
              │ • Syscall Interface    │
              └────────────────────────┘
```

---

## 📦 Component Structure

### 1. Daemon (seriousum-daemon)

**Purpose**: Main Cilium agent running on each node

```
seriousum-daemon
├── Configuration
│   ├── Load from ConfigMap
│   ├── Load from CLI args
│   └── Load from environment
├── eBPF Management
│   ├── Program loading
│   ├── Map management
│   └── Program updates
├── Control Plane
│   ├── Kubernetes integration
│   ├── Policy evaluation
│   ├── Endpoint tracking
│   └── Identity management
├── Datapath
│   ├── Packet forwarding
│   ├── Service load balancing
│   ├── Policy enforcement
│   └── Encryption (WireGuard/IPsec)
└── Observability
    ├── Metrics (Prometheus)
    ├── Events (Hubble)
    └── Logging
```

### 2. Operator (seriousum-operator)

**Purpose**: Kubernetes-native lifecycle management

```
seriousum-operator
├── CRD Management
│   ├── CiliumClusterConfig
│   ├── CiliumNode
│   ├── CiliumEndpoint
│   └── CiliumIdentity
├── Reconciliation
│   ├── Watch for changes
│   ├── Compute desired state
│   ├── Apply changes
│   └── Verify convergence
└── Administration
    ├── Cluster initialization
    ├── Node onboarding
    └── Resource cleanup
```

### 3. CLI Tools

#### seriousum-cli (cilium)

**Purpose**: User-facing cluster management tool

```
seriousum-cli
├── Status Commands
│   ├── status
│   ├── endpoint list
│   └── policy list
├── Debug Commands
│   ├── config
│   ├── bpf
│   └── monitor
└── Admin Commands
    ├── identity
    ├── kvstore
    └── clustermesh
```

#### seriousum-dbg (cilium-dbg)

**Purpose**: Advanced debugging and diagnostics

```
seriousum-dbg
├── Kernel Inspection
│   ├── eBPF inspection
│   ├── BPF map dumps
│   └── Ring buffer events
├── Policy Analysis
│   ├── Policy rules
│   ├── Label matching
│   └── Decision tree
└── Network Inspection
    ├── Endpoint details
    ├── Routing tables
    └── Traffic analysis
```

---

## 🔄 Request Flow

### Incoming Traffic

```
[Packet arrives on NIC]
         │
         ▼
[eBPF XDP/TC Program]
         │
    ┌────┴──────┐
    │            │
 [DROP]    [FORWARD]
             │
             ▼
[Policy Evaluation]
             │
        ┌────┴────┐
        │          │
    [ALLOW]    [DROP]
        │
        ▼
[Endpoint Resolution]
        │
        ▼
[Load Balancer (if service)]
        │
        ▼
[Encryption (if enabled)]
        │
        ▼
[Destination Pod]
```

### Policy Evaluation

```
[Packet]
    │
    ▼
[Extract Headers]
    │
    ▼
[Source Identity Lookup]
    │
    ▼
[Destination Identity Lookup]
    │
    ▼
[Policy Rule Match]
    │
    ├─ [Exact Match] → [Decision]
    │
    ├─ [Wildcard Match] → [Decision]
    │
    └─ [No Match] → [Default (Allow/Deny)]
        │
        ▼
    [Decision]
        │
    ┌───┴───┐
    │       │
[ALLOW] [DROP]
```

---

## 💾 Data Flow

### State Synchronization

```
[Kubernetes API]
        │
        ▼
[Agent Informers]
├─ Pod watcher
├─ Service watcher
├─ NetworkPolicy watcher
├─ Endpoint watcher
└─ Node watcher
        │
        ▼
[Local Cache]
├─ Endpoint state
├─ Policy rules
├─ Identity mappings
└─ Service state
        │
        ▼
[BPF Map Updates]
├─ Policy map
├─ Endpoint map
├─ Service map
└─ Identity map
        │
        ▼
[eBPF Program]
├─ Load policies
├─ Resolve endpoints
├─ Apply routing
└─ Enforce policy
```

### Identity Management

```
[Pod Workload]
        │
        ▼
[Label Selection]
├─ Pod labels
├─ Namespace labels
├─ Cluster labels
└─ Custom labels
        │
        ▼
[Identity Allocation]
├─ Calculate hash
├─ Assign numeric ID
└─ Store in etcd (if clustermesh)
        │
        ▼
[Identity Cache]
├─ Map ID → Labels
├─ Map Labels → ID
└─ Distribute to nodes
        │
        ▼
[eBPF Identity Map]
├─ 16-bit identity
├─ Fast lookup (O(1))
└─ Kernel access
```

---

## 🔌 Integration Points

### With Kubernetes

```
Cilium ←→ Kubernetes
    ├─ API Server (watch resources)
    ├─ etcd (cluster state)
    ├─ kubelet (node status)
    └─ CNI (pod network setup)
```

### With Linux Kernel

```
Cilium ←→ Kernel
    ├─ eBPF subsystem
    ├─ netfilter hooks
    ├─ socket syscalls
    ├─ BPF filesystem
    └─ debugfs/tracefs
```

### With External Services

```
Cilium ←→ External
    ├─ Envoy (L7 policies)
    ├─ Prometheus (metrics)
    ├─ Hubble (observability)
    ├─ etcd (ClusterMesh)
    └─ DNS (FQDN policies)
```

---

## 📊 Data Structures

### Core Maps (eBPF)

```
Policy Map
├─ Key: (src_identity, dst_identity, port, proto)
├─ Value: allow/deny/log
└─ Size: ~100MB (typical)

Endpoint Map
├─ Key: IP address
├─ Value: endpoint_id, flags
└─ Size: ~10MB (typical)

Service Map
├─ Key: (service_ip, port)
├─ Value: backend IPs, LB algorithm
└─ Size: ~5MB (typical)

Identity Map
├─ Key: numeric identity
├─ Value: security labels (compressed)
└─ Size: ~2MB (typical)

Ring Buffer (Events)
├─ Flow events
├─ Policy violations
├─ Connection tracking
└─ Size: ~100MB (circular)
```

---

## 🔐 Security Architecture

### Defense Layers

```
Layer 1: Admission
├─ Label validation
├─ Resource quotas
└─ RBAC

Layer 2: Network Policy
├─ Ingress rules
├─ Egress rules
└─ L4/L7 rules

Layer 3: Encryption
├─ WireGuard tunnels
├─ IPsec
└─ TLS (for APIs)

Layer 4: Isolation
├─ Network namespaces
├─ Pod sandboxing
└─ Resource limits
```

### Identity Model

```
Security Identity
├─ Reserved (1-99)
│  ├─ 1 = world
│  ├─ 2 = unmanaged
│  └─ ...
├─ Cluster-local (100-65535)
│  ├─ Service identities
│  ├─ Pod identities
│  └─ Host identities
└─ Global (65536+)
   └─ Multi-cluster (if enabled)
```

---

## ⚡ Performance Characteristics

### eBPF Program Performance

```
Typical latency per packet:
├─ Policy lookup: 100-500ns (BPF map)
├─ Endpoint resolution: 100-200ns (hash map)
├─ Load balancer selection: 200-500ns (hash algorithm)
└─ Total: <2μs for fast path

Throughput:
├─ Per-core: 1-10 Gbps (depends on policy complexity)
├─ With encryption: 500Mbps-2Gbps
└─ Scalable: Linear with CPU cores
```

### Memory Usage

```
Per-node typical:
├─ Agent process: 50-200MB
├─ eBPF maps: 100-500MB (policy dependent)
├─ Operator (if running): 50-100MB
└─ Total: 200-800MB per node

Scales with:
├─ Number of pods
├─ Number of policies
├─ ClusterMesh scope
└─ Observability enabled
```

---

## 🔄 Concurrency Model

### Agent Threading

```
Main Thread
├─ Configuration loading
├─ Component initialization
└─ Event loop

Worker Threads
├─ Kubernetes watch handlers
├─ Policy updates
├─ Endpoint synchronization
├─ API server
└─ Metrics collection

eBPF Programs (Kernel)
├─ XDP ingress (parallel per NIC)
├─ TC egress (parallel per NIC)
└─ Kprobes (parallel per core)
```

### Synchronization

```
Policy Updates
├─ Lock: DashMap (concurrent hashmap)
├─ Write: Policy engine updates
├─ Read: eBPF programs (lock-free on kernel side)
└─ Latency: <100ms propagation

Endpoint Tracking
├─ Lock: DashMap
├─ Event-driven updates
├─ K8s watchers trigger updates
└─ Latency: <1s to eBPF

Identity Resolution
├─ Lock: RwLock
├─ High write frequency
├─ Lock-free reads (copy-on-write)
└─ Latency: <10ms
```

---

## 🚀 Startup Sequence

```
1. Binary Start (seriousum-daemon)
   └─ Load configuration
   └─ Check kernel version
   └─ Verify capabilities

2. eBPF Initialization
   └─ Compile/load BPF programs
   └─ Create BPF maps
   └─ Attach to network interfaces

3. Kubernetes Connection
   └─ Authenticate to API server
   └─ List existing resources
   └─ Start watchers

4. State Recovery
   └─ Load existing endpoints
   └─ Recover policies
   └─ Rebuild maps

5. Service Readiness
   └─ Start REST API
   └─ Start metrics server
   └─ Accept traffic

Total time: 30-60 seconds typical
```

---

## 📈 Scaling Considerations

### Horizontal Scaling

```
Scales to: 100+ nodes
├─ One agent per node (DaemonSet)
├─ Central etcd for state
├─ Distributed policy evaluation
└─ Per-node eBPF programs
```

### Policy Complexity

```
Scales to: 1000+ policies
├─ Efficient policy lookup (O(1) eBPF maps)
├─ Hierarchical labels
├─ Policy grouping
└─ Incremental updates
```

### ClusterMesh

```
Scales to: 10+ clusters
├─ Distributed identity allocation
├─ Cross-cluster policy rules
├─ Global service discovery
└─ Encrypted cluster links
```

---

## 🔍 Observability Architecture

### Metrics

```
Prometheus Integration
├─ Agent metrics
│  ├─ Policy decisions (allow/deny)
│  ├─ Endpoint count
│  ├─ API latency
│  └─ Resource usage
├─ eBPF metrics
│  ├─ Packets processed
│  ├─ Policy evaluations
│  └─ Drop counts
└─ Kubernetes metrics
   ├─ Pod count
   ├─ Policy count
   └─ Cluster size
```

### Flow Observability (Hubble)

```
Flow Events
├─ Source: eBPF ring buffer
├─ Format: Connection metadata
├─ Storage: In-memory + optional DB
└─ Access: Hubble UI or API

Information per flow:
├─ Source/destination identity
├─ Protocol and port
├─ Allowed or denied
├─ Verdict reason
└─ Timestamp and duration
```

---

## 🎯 Design Principles

1. **eBPF-First**: Kernel-level enforcement for performance
2. **Kubernetes-Native**: Tight API server integration
3. **Zero-Trust**: Identity-based, not IP-based
4. **Observable**: Deep visibility into all traffic
5. **Scalable**: Horizontal and policy complexity scaling
6. **Secure**: Defense-in-depth approach
7. **Maintainable**: Clean code architecture

---

**Architecture Version**: v0.1.0-alpha  
**Last Updated**: May 11, 2026
