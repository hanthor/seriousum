# Full Rust Operator Implementation - Specification & Plan

**Status**: Implementation Planning (Updated to use kube-rs)  
**Date**: 2026-05-11  
**GitHub Issue**: #57  
**Target**: Replace upstream Cilium operator with Rust implementation  
**Framework**: kube-rs (3.1.0+) with custom reconcilers  

---

## Overview

Transform the current operator scaffold into a production-grade Kubernetes operator that:
- Manages agent DaemonSets and reconciliation
- Watches network policies and distributes configurations
- Maintains operator-agent communication
- Handles cluster state and lifecycle events
- Reports comprehensive metrics and status

**Current State**: Basic scaffold (345 LOC, health reporting)  
**Target State**: Full K8s operator (1,500-2,000 LOC, feature complete)  
**Timeline**: 3-4 weeks (Phase 1: 1 week core + 2-3 weeks features)  

---

## Architecture

### Operator Components

```
┌────────────────────────────────────────────────────┐
│          Cilium Rust Operator                      │
├────────────────────────────────────────────────────┤
│                                                    │
│  ┌──────────────────────────────────────────┐   │
│  │   Kubernetes Resource Watchers           │   │
│  │  ├─ CiliumNetworkPolicy                  │   │
│  │  ├─ DaemonSet (agent)                    │   │
│  │  ├─ Pod (monitoring)                     │   │
│  │  └─ Service                              │   │
│  └──────────────────────────────────────────┘   │
│                                                    │
│  ┌──────────────────────────────────────────┐   │
│  │   Reconciliation Engine                  │   │
│  │  ├─ Agent reconciliation                 │   │
│  │  ├─ Configuration distribution           │   │
│  │  ├─ Policy compilation                   │   │
│  │  └─ State synchronization                │   │
│  └──────────────────────────────────────────┘   │
│                                                    │
│  ┌──────────────────────────────────────────┐   │
│  │   Controller Components                  │   │
│  │  ├─ Policy controller                    │   │
│  │  ├─ Agent lifecycle controller           │   │
│  │  ├─ Configuration controller             │   │
│  │  └─ Status report controller             │   │
│  └──────────────────────────────────────────┘   │
│                                                    │
│  ┌──────────────────────────────────────────┐   │
│  │   Storage & Querying                     │   │
│  │  ├─ Policy cache                         │   │
│  │  ├─ Agent state cache                    │   │
│  │  ├─ Configuration cache                  │   │
│  │  └─ Metrics storage                      │   │
│  └──────────────────────────────────────────┘   │
│                                                    │
│  ┌──────────────────────────────────────────┐   │
│  │   API Endpoints                          │   │
│  │  ├─ /healthz (health check)              │   │
│  │  ├─ /v1/config (agent config)            │   │
│  │  ├─ /v1/policies (policy data)           │   │
│  │  ├─ /v1/status (operator status)         │   │
│  │  └─ /metrics (prometheus metrics)        │   │
│  └──────────────────────────────────────────┘   │
│                                                    │
└────────────────────────────────────────────────────┘
         ▲                           ▲
         │                           │
    ┌────┴─────────┬────────────────┴────┐
    │              │                     │
  Agents      Kubernetes API         etcd
(DaemonSet)   (watchers)           (config)
```

---

## Framework Choice: kube-rs

**Why kube-rs?**
- ✅ Production-grade Kubernetes client library (3.1.0)
- ✅ Type-safe API with derive macros for CRDs
- ✅ Efficient async watchers (tokio-based)
- ✅ Built-in error handling and retry logic
- ✅ Active maintenance and great documentation
- ✅ Used in many production operators (sealed-secrets, etc.)

**Dependencies to add to Cargo.toml**:
```toml
[dependencies]
kube = { version = "0.95", features = ["runtime", "derive"] }
tokio = { version = "1", features = ["full"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
prometheus = "0.13"
```

**Example kube-rs usage**:
```rust
use kube::{Api, Client, ResourceExt};
use kube::api::ListParams;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::try_default().await?;
    let api = Api::<Pod>::all(client);
    
    // Watch pods
    let params = ListParams::default().limit(1);
    let mut stream = api.watch(&params, "0").await?;
    
    while let Some(event) = stream.try_next().await? {
        println!("Pod event: {:?}", event);
    }
    Ok(())
}
```

---

## Implementation Phases

### Phase 1: Core Operator Framework (1 week)

**Goal**: Basic operator that can run in Kubernetes and serve health/status

**Components**:

1. **Operator Server** (HTTP/REST)
   ```rust
   pub struct OperatorServer {
       config: OperatorConfig,
       state: Arc<OperatorState>,
   }
   
   impl OperatorServer {
       pub async fn run(&self) -> Result<()>;
       pub async fn handle_healthz(&self) -> HealthResponse;
       pub async fn handle_config(&self) -> ConfigResponse;
   }
   ```

2. **Kubernetes Client Integration (via kube-rs)**
   ```rust
   use kube::{Api, Client, ResourceExt};
   
   pub struct KubernetesClient {
       client: Client,
       namespace: String,
   }
   
   impl KubernetesClient {
       pub async fn new(namespace: &str) -> Result<Self> {
           let client = Client::try_default().await?;
           Ok(Self {
               client,
               namespace: namespace.to_string(),
           })
       }
       
       pub async fn list_agents(&self) -> Result<Vec<Pod>> {
           let api = Api::<Pod>::namespaced(
               self.client.clone(),
               &self.namespace
           );
           api.list(&Default::default()).await.map(|list| list.items)
       }
       
       pub async fn watch_policies(&self) -> Result<impl Stream<Item = WatchEvent>> {
           let api = Api::<CiliumNetworkPolicy>::namespaced(
               self.client.clone(),
               &self.namespace
           );
           api.watch(&Default::default(), "0").await
       }
   }
   ```

3. **State Management**
   ```rust
   pub struct OperatorState {
       agents: RwLock<Vec<AgentInfo>>,
       policies: RwLock<Vec<NetworkPolicy>>,
       config: RwLock<OperatorConfig>,
   }
   ```

4. **Basic HTTP Endpoints**
   - `GET /healthz` → health status
   - `GET /v1/status` → operator status
   - `GET /v1/config` → operator configuration

**Deliverables**:
- Operator binary can start and serve HTTP
- Health endpoints working
- Configuration endpoints working
- Kubernetes authentication working

**Tests**:
- Health endpoint returns 200
- Config endpoint returns JSON
- Operator restarts gracefully
- State persistence across restarts

---

### Phase 2: Kubernetes Integration (1 week)

**Goal**: Operator actively watches K8s resources and maintains state

**Components**:

1. **Policy Watcher (using kube-rs)**
   ```rust
   use kube::runtime::watcher;
   use kube::runtime::WatchStreamExt;
   
   pub struct PolicyWatcher {
       client: Client,
       state: Arc<OperatorState>,
   }
   
   impl PolicyWatcher {
       pub async fn watch_policies(&self) -> Result<()> {
           let api = Api::<CiliumNetworkPolicy>::all(self.client.clone());
           let mut stream = watcher(api, Default::default()).applied_objects();
           
           while let Some(policy) = stream.try_next().await? {
               self.on_policy_add(&policy).await;
           }
           Ok(())
       }
       
       pub async fn on_policy_add(&self, policy: &CiliumNetworkPolicy) {
           // Handle policy addition
       }
   }
   ```

2. **Agent Watcher (using kube-rs)**
   ```rust
   use kube::runtime::watcher;
   
   pub struct AgentWatcher {
       client: Client,
       state: Arc<OperatorState>,
   }
   
   impl AgentWatcher {
       pub async fn watch_agents(&self) -> Result<()> {
           let api = Api::<Pod>::namespaced(self.client.clone(), "cilium");
           let selector = ListParams::default()
               .labels("app=cilium-agent");
           let mut stream = watcher(api, selector).applied_objects();
           
           while let Some(pod) = stream.try_next().await? {
               self.on_agent_join(&pod).await;
           }
           Ok(())
       }
   }
   ```

3. **Event System**
   ```rust
   pub enum Event {
       PolicyAdded(String),
       PolicyUpdated(String),
       PolicyRemoved(String),
       AgentJoined(String),
       AgentLeft(String),
       ConfigChanged,
   }
   
   pub struct EventBus {
       subscribers: Vec<mpsc::Sender<Event>>,
   }
   ```

**Deliverables**:
- Operator watches CiliumNetworkPolicy resources
- Operator watches agent pods
- State updated in real-time
- Events propagated to handlers

**Tests**:
- Policy watch detects additions
- Policy watch detects updates
- Policy watch detects deletions
- Agent discovery works
- State consistency verified

---

### Phase 3: Policy Distribution (1 week)

**Goal**: Operator compiles policies and distributes to agents

**Components**:

1. **Policy Compiler**
   ```rust
   pub struct PolicyCompiler {
       policies: Vec<NetworkPolicy>,
   }
   
   impl PolicyCompiler {
       pub fn compile_policies(&self) -> Result<CompiledPolicies>;
       pub fn detect_conflicts(&self) -> Result<Vec<PolicyConflict>>;
       pub fn validate_syntax(&self) -> Result<()>;
   }
   ```

2. **Policy Distributor**
   ```rust
   pub struct PolicyDistributor {
       k8s: KubernetesClient,
       policies: Arc<RwLock<Vec<NetworkPolicy>>>,
   }
   
   impl PolicyDistributor {
       pub async fn distribute_policies(&self) -> Result<()>;
       pub async fn update_agent_configmap(&self, agent: &str, policies: &[Policy]) -> Result<()>;
       pub async fn notify_agents(&self) -> Result<()>;
   }
   ```

3. **API Endpoint for Policy Distribution**
   ```rust
   // GET /v1/policies?node=node-1
   pub async fn get_policies_for_node(
       &self,
       node: &str,
   ) -> Result<Vec<CompiledPolicy>>;
   ```

**Deliverables**:
- Policies compiled when added/updated
- Conflicts detected and reported
- ConfigMaps created for policy distribution
- Agents can fetch policies via API

**Tests**:
- Policy compilation works
- Conflict detection works
- ConfigMap creation verified
- API endpoints return correct policies

---

### Phase 4: Agent Lifecycle Management (1 week)

**Goal**: Operator manages agent DaemonSet and monitors health

**Components**:

1. **DaemonSet Manager**
   ```rust
   pub struct DaemonSetManager {
       k8s: KubernetesClient,
   }
   
   impl DaemonSetManager {
       pub async fn create_agent_daemonset(&self) -> Result<()>;
       pub async fn update_agent_daemonset(&self, image: &str) -> Result<()>;
       pub async fn get_daemonset_status(&self) -> Result<DaemonSetStatus>;
   }
   ```

2. **Agent Health Monitor**
   ```rust
   pub struct AgentHealthMonitor {
       k8s: KubernetesClient,
       state: Arc<OperatorState>,
   }
   
   impl AgentHealthMonitor {
       pub async fn monitor_agents(&self) -> Result<()>;
       pub async fn check_agent_health(&self, pod: &str) -> Result<HealthStatus>;
       pub async fn restart_unhealthy_agents(&self) -> Result<()>;
   }
   ```

3. **Status Reporting**
   ```rust
   pub struct StatusReporter {
       k8s: KubernetesClient,
   }
   
   impl StatusReporter {
       pub async fn report_operator_status(&self) -> Result<()>;
       pub async fn report_cluster_status(&self) -> Result<()>;
       pub async fn update_status_crd(&self) -> Result<()>;
   }
   ```

**Deliverables**:
- Agent DaemonSet created and managed
- Agent health monitored
- Unhealthy agents restarted
- Status reported via CRD
- Metrics exported for monitoring

**Tests**:
- DaemonSet creation works
- Agent health check works
- Restart logic works
- Status reporting works

---

### Phase 5: Metrics & Observability (1 week)

**Goal**: Operator exports comprehensive metrics and logs

**Components**:

1. **Metrics Collector**
   ```rust
   pub struct MetricsCollector {
       registry: prometheus::Registry,
   }
   
   impl MetricsCollector {
       pub fn agent_count(&self) -> Gauge;
       pub fn policy_count(&self) -> Gauge;
       pub fn config_updates(&self) -> Counter;
       pub fn reconciliation_duration(&self) -> Histogram;
   }
   ```

2. **Metrics Exporter**
   ```rust
   pub struct MetricsExporter {
       collector: Arc<MetricsCollector>,
   }
   
   impl MetricsExporter {
       pub async fn export_prometheus(&self) -> Result<String>;
       pub async fn serve_metrics(&self, addr: &str) -> Result<()>;
   }
   ```

3. **Logging**
   ```rust
   // Structured logging via tracing
   trace!("Policy added: {}", policy_name);
   info!("Agent joined: {}", agent_id);
   warn!("Policy conflict detected");
   error!("Agent health check failed: {}", reason);
   ```

**Deliverables**:
- Prometheus metrics endpoint (`/metrics`)
- Key metrics: agent count, policy count, etc.
- Structured logs for all operations
- OpenTelemetry integration ready

**Tests**:
- Metrics endpoint accessible
- Metrics have correct values
- Logs capture all events
- No log panics

---

## API Specification

### Endpoints

#### Health & Status

```
GET /healthz
  Response: { "status": "healthy", "message": "...", "version": {...} }
  
GET /v1/status
  Response: {
    "status": "ready",
    "agents_ready": 3,
    "policies_deployed": 5,
    "last_reconciliation": "2026-05-11T10:30:00Z"
  }
```

#### Configuration

```
GET /v1/config
  Response: { "operator_name": "...", "cluster_name": "...", ... }
  
POST /v1/config
  Body: { configuration update }
  Response: { "status": "updated" }
```

#### Policies

```
GET /v1/policies?node=node-1
  Response: [
    { "id": "policy-1", "rules": [...], "version": 1 },
    { "id": "policy-2", "rules": [...], "version": 1 }
  ]
  
GET /v1/policies/{policy-id}
  Response: { "id": "policy-1", "rules": [...], "version": 1 }
```

#### Agents

```
GET /v1/agents
  Response: [
    { "id": "node-1", "status": "ready", "version": "v0.1.0" },
    { "id": "node-2", "status": "ready", "version": "v0.1.0" }
  ]
  
GET /v1/agents/{agent-id}
  Response: { "id": "node-1", "status": "ready", ... }
```

#### Metrics

```
GET /metrics
  Response: Prometheus metrics (text format)
  
cilium_operator_agent_count{status="ready"} 3
cilium_operator_policies_deployed 5
cilium_operator_reconciliation_duration_seconds_bucket{le="1.0"} 10
```

---

## Database/Storage

### In-Memory State

```rust
pub struct OperatorState {
    // Agents
    agents: RwLock<HashMap<String, AgentInfo>>,
    
    // Policies
    policies: RwLock<HashMap<String, NetworkPolicy>>,
    
    // Configuration
    config: RwLock<OperatorConfig>,
    
    // Metrics
    metrics: Arc<MetricsCollector>,
    
    // Cache
    policy_cache: RwLock<PolicyCache>,
}
```

### Kubernetes Storage

- **ConfigMaps**: Store compiled policies per node
- **Secrets**: Store operator credentials
- **CRDs**: Store operator status and agent status
- **Events**: Record significant events

---

## Error Handling

### Error Types

```rust
pub enum OperatorError {
    KubernetesError(String),
    ConfigError(String),
    PolicyError(String),
    StateError(String),
    NetworkError(String),
    TimeoutError,
}
```

### Retry Strategy

```rust
pub struct RetryConfig {
    max_retries: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
}

// Exponential backoff with jitter
pub async fn retry_with_backoff<F, R>(
    f: F,
    config: &RetryConfig,
) -> Result<R>
where
    F: Fn() -> BoxFuture<'static, Result<R>>,
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kube_fake_client::DynamicClient; // Fake client for testing

    #[test]
    fn test_operator_creation() {
        let operator = Operator::new(test_config());
        assert_eq!(operator.status(), OperatorStatus::Ready);
    }

    #[tokio::test]
    async fn test_policy_watcher() {
        let (fake_client, _) = kube_fake_client::new::<CiliumNetworkPolicy>();
        let watcher = PolicyWatcher::new(fake_client);
        
        // Add fake policy
        // Verify watcher processes it
    }
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_operator_with_kind_cluster() {
    let cluster = KindCluster::create("test").await.unwrap();
    let operator = Operator::with_k8s(cluster.client()).await.unwrap();
    
    // Deploy agent DaemonSet
    operator.deploy_agents().await.unwrap();
    
    // Verify agents running
    let agents = operator.list_agents().await.unwrap();
    assert!(!agents.is_empty());
}
```

### E2E Tests

- Deploy operator to real K8s cluster
- Add policies
- Verify agents enforce policies
- Check metrics
- Restart operator and verify state recovery

---

## Deployment

### Docker Image

```dockerfile
FROM rust:1.95 as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p seriousum-operator

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/seriousum-operator /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/seriousum-operator"]
```

### Kubernetes Manifest (ServiceAccount with proper RBAC)

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: cilium-operator
  namespace: cilium
---
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: cilium-operator
rules:
  - apiGroups: [""]
    resources: ["pods", "services", "configmaps"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["cilium.io"]
    resources: ["ciliumnetworkpolicies"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["daemonsets"]
    verbs: ["create", "get", "update", "patch"]
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: cilium-operator
  namespace: cilium
spec:
  replicas: 2
  template:
    spec:
      containers:
      - name: operator
        image: ghcr.io/hanthor/seriousum-operator:v0.2.0
        ports:
        - containerPort: 8080
        env:
        - name: CILIUM_OPERATOR_NAMESPACE
          value: cilium
        resources:
          requests:
            cpu: 100m
            memory: 128Mi
          limits:
            cpu: 500m
            memory: 512Mi
```

---

## Timeline

```
Week 1: Core Framework (Phase 1)
  - HTTP server + endpoints
  - K8s client setup
  - Basic state management
  - Unit tests

Week 2: K8s Integration (Phase 2)
  - Policy watcher
  - Agent watcher
  - Event system
  - Integration tests

Week 3: Policy Distribution (Phase 3)
  - Policy compiler
  - Policy distributor
  - API endpoints
  - Conflict detection

Week 4: Agent Lifecycle (Phase 4)
  - DaemonSet management
  - Health monitoring
  - Status reporting
  - E2E testing

Week 5: Observability (Phase 5)
  - Metrics collection
  - Prometheus export
  - Structured logging
  - Documentation

Total: ~3-4 weeks for full operator (faster with kube-rs leverage)

Time savings with kube-rs:
  - K8s client already built ✅
  - Watchers/streams handled ✅
  - Error handling built-in ✅
  - RBAC support included ✅
  → Focus only on reconciliation logic!
```

---

## Success Criteria

✅ **Functionality**:
- Operator starts successfully in Kubernetes
- Watches and responds to policy changes
- Distributes policies to agents
- Manages agent lifecycle
- Restarts unhealthy agents

✅ **Quality**:
- 100% of endpoints tested
- >80% code coverage
- 0 panics
- No unsafe blocks (except where necessary)

✅ **Performance**:
- Operator restarts in <5 seconds
- Policy distribution latency <500ms
- Memory footprint <100MB
- No resource leaks

✅ **Operations**:
- Metrics exported to Prometheus
- Comprehensive structured logs
- Health endpoints return 200
- Status CRD updated
- Error recovery automatic

---

## Implementation Steps

### Step 1: Scaffolding (Today)
- [ ] Create feature branches
- [ ] Set up module structure
- [ ] Define core types
- [ ] Write architecture documentation

### Step 2: HTTP Server (Day 1-2)
- [ ] Implement `OperatorServer` struct
- [ ] Add health endpoint
- [ ] Add status endpoint
- [ ] Add configuration endpoint

### Step 3: K8s Integration (Day 3-4)
- [ ] Implement `KubernetesClient`
- [ ] Add policy watcher
- [ ] Add agent watcher
- [ ] Test with kind cluster

### Step 4: Policy Management (Day 5-6)
- [ ] Implement policy compiler
- [ ] Add conflict detection
- [ ] Implement distributor
- [ ] Add API endpoints

### Step 5: Agent Management (Day 7-8)
- [ ] Implement DaemonSet manager
- [ ] Add health monitoring
- [ ] Add status reporting
- [ ] Test full lifecycle

### Step 6: Observability (Day 9-10)
- [ ] Add metrics collection
- [ ] Implement Prometheus exporter
- [ ] Add structured logging
- [ ] Write documentation

---

## Next Steps

1. **Now**: 
   - [ ] Add kube-rs to Cargo.toml
   - [ ] Define CRD types using derive macros
   - [ ] Create reconciler structure

2. **Next**: Begin Phase 1 (HTTP server + K8s integration)
   - [ ] Create OperatorServer with kube-rs Client
   - [ ] Implement policy watcher
   - [ ] Implement agent watcher

3. **Target**: v0.2.0 includes basic operator
   - Policies watched and distributed
   - Agents discovered and tracked
   - Health endpoints working

4. **Goal**: Full operator in v0.3.0
   - Agent lifecycle management
   - Metrics and observability
   - Production-ready

---

## Comparison: Custom vs kube-rs

| Feature | Custom | kube-rs |
|---------|--------|----------|
| K8s API Client | Build from scratch | ✅ Included |
| Type Safety | Manual | ✅ Full derive support |
| Error Handling | Manual retry logic | ✅ Built-in |
| Watchers/Streams | Manual polling | ✅ Efficient streams |
| CRD Support | Custom deserialization | ✅ Derive macros |
| RBAC | Manual | ✅ Supported |
| Maintenance | High | ✅ Community maintained |
| Performance | Unknown | ✅ Battle-tested |
| Development Time | 3-4 weeks | **1-2 weeks** |
| Production Readiness | Risky | ✅ Proven |

**Recommendation**: Use kube-rs, focus on reconciliation logic (the differentiation)

---

**Document Version**: 1.0  
**Status**: Implementation Specification Complete  
**GitHub Issue**: #57  
**Effort Estimate**: 3-4 weeks (5 phases, 1 week each)  
**Impact**: Removes upstream dependency, enables full Rust solution  
