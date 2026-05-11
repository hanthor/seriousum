# Service Subsystem Implementation Specification

## Overview
This document specifies what must be implemented to make K8sDatapathServicesTest pass.

## Current State
- **Agent Binary:** Exists, runs, but incomplete
- **Test Framework:** Operational, timeouts on service setup
- **eBPF Programs:** Not compiled/loaded
- **Service Observer:** Not implemented
- **Endpoint Manager:** Not implemented

## Required Implementations

### 1. CNI Socket & Health Endpoint [CRITICAL]

**File:** crates/daemon/src/lib.rs or new crates/cni/src/lib.rs

**Specification:**
```rust
// Must create Unix socket at startup
const CNI_SOCKET_PATH: &str = "/var/run/cilium/cilium.sock";
const HEALTH_CHECK_ADDR: &str = "0.0.0.0:9879";
const HEALTH_CHECK_PATH: &str = "/healthz";

impl Daemon {
    pub async fn run(&self) -> anyhow::Result<()> {
        // 1. Create CNI socket listener
        let cni_listener = UnixListener::bind(CNI_SOCKET_PATH)?;
        
        // 2. Bind health check HTTP server
        let health_server = start_health_server(HEALTH_CHECK_ADDR)?;
        
        // 3. Accept CNI requests in background
        tokio::spawn(handle_cni_requests(cni_listener));
        
        // 4. Block on HTTP server (keep alive)
        health_server.await?;
    }
}

async fn handle_health_check() -> String {
    // Return 200 OK only when:
    // - CNI socket is bound
    // - eBPF programs loaded
    // - Service observer running
    // - Endpoint manager running
    json!({
        "status": "ok",
        "cni": "ready",
        "bpf": "loaded",
        "services": "initialized"
    }).to_string()
}
```

**Tests Required:**
- [ ] Socket created at correct path
- [ ] Socket permissions: 0644 (readable by Kubelet)
- [ ] Health endpoint responds 200 on healthz
- [ ] Health endpoint returns 500 if BPF not ready
- [ ] Socket survives across pod restarts

**Blockers:** Must fix startup probe timeout in agent

---

### 2. Service Observer [HIGH]

**File:** crates/controller/src/service.rs (NEW)

**Specification:**
```rust
pub struct ServiceObserver {
    client: kube::Client,
    cache: Arc<RwLock<ServiceCache>>,
}

impl ServiceObserver {
    pub async fn start(&self) -> anyhow::Result<()> {
        let watcher = watcher(client, ListParams::default());
        
        loop {
            match watcher.next().await {
                Some(Ok(Event::Applied(svc))) => {
                    // New or updated Service
                    let cilium_svc = translate_service(&svc)?;
                    self.cache.write().await.insert(cilium_svc)?;
                    
                    // Notify: Update eBPF map
                    publish_service_to_ebpf(&cilium_svc)?;
                },
                Some(Ok(Event::Deleted(svc))) => {
                    // Service deleted
                    let svc_key = ServiceKey::from(&svc);
                    self.cache.write().await.remove(&svc_key)?;
                    
                    // Notify: Remove from eBPF map
                    remove_service_from_ebpf(&svc_key)?;
                },
                _ => {}
            }
        }
    }
}

fn translate_service(k8s_svc: &Service) -> Result<CiliumService> {
    // Extract:
    // - spec.clusterIP → VIP
    // - spec.ports → port list
    // - spec.selector → label selector for backend discovery
    // - metadata.namespace → namespace
    // - metadata.name → service name
    
    Ok(CiliumService {
        name: k8s_svc.metadata.name.clone()?,
        namespace: k8s_svc.metadata.namespace.clone()?,
        cluster_ip: k8s_svc.spec.cluster_ip.parse()?,
        ports: k8s_svc.spec.ports.clone(),
        selector: k8s_svc.spec.selector.clone(),
    })
}
```

**Events to Handle:**
- [x] Service Created → Create CiliumService
- [x] Service Updated (spec changed) → Update mappings
- [x] Service Deleted → Remove from eBPF
- [x] Service port changed → Update eBPF map
- [x] Service selector changed → Resolve new backends

**Maps to Update:**
- `cilium_svc_*` - Store service definitions
- `cilium_lb4_map` - Store VIP → backend entries
- `cilium_lb6_map` - Store IPv6 VIP entries

**Tests Required:**
- [ ] Service created → CiliumService created
- [ ] Service deleted → CiliumService removed
- [ ] Service selector changed → Backends re-resolved
- [ ] Service port changed → Map entries updated
- [ ] Race condition: Service created before endpoints
- [ ] Race condition: Service deleted but endpoints exist

---

### 3. Endpoint Manager [HIGH]

**File:** crates/endpoint/src/manager.rs (NEW)

**Specification:**
```rust
pub struct EndpointManager {
    client: kube::Client,
    cache: Arc<RwLock<EndpointCache>>,
    identity_allocator: Arc<IdentityAllocator>,
}

impl EndpointManager {
    pub async fn start(&self) -> anyhow::Result<()> {
        // Watch Pods in all namespaces
        let watcher = watcher(client, ListParams::default());
        
        loop {
            match watcher.next().await {
                Some(Ok(Event::Applied(pod))) => {
                    if should_manage(&pod) {
                        let endpoint = create_endpoint(&pod)?;
                        self.cache.write().await.insert(endpoint.clone())?;
                        
                        // Assign identity from labels
                        let identity = self.identity_allocator.allocate(&pod).await?;
                        
                        // Write to BPF endpoint map
                        write_endpoint_map(&endpoint, identity)?;
                        
                        // Create CiliumEndpoint resource
                        create_cilium_endpoint_crd(&endpoint)?;
                    }
                },
                Some(Ok(Event::Deleted(pod))) => {
                    let endpoint = endpoint_from_pod(&pod)?;
                    self.cache.write().await.remove(&endpoint)?;
                    
                    // Remove from BPF map
                    remove_endpoint_map(&endpoint)?;
                    
                    // Delete CiliumEndpoint resource
                    delete_cilium_endpoint_crd(&endpoint)?;
                },
                _ => {}
            }
        }
    }
}

fn create_endpoint(pod: &Pod) -> Result<Endpoint> {
    // Extract:
    // - metadata.name → endpoint name
    // - metadata.namespace → namespace
    // - spec.containers[0].containerID → container ID (from status)
    // - status.podIP → endpoint IP
    // - status.containerStatuses → state
    // - metadata.labels → pod labels
    
    let pod_ip = pod.status.pod_ip.clone()?;
    
    Ok(Endpoint {
        id: generate_endpoint_id(&pod)?,
        name: pod.metadata.name.clone()?,
        namespace: pod.metadata.namespace.clone()?,
        pod_name: pod.metadata.name.clone()?,
        pod_ip: pod_ip.parse()?,
        labels: pod.metadata.labels.clone().unwrap_or_default(),
        state: EndpointState::Ready,
    })
}
```

**Events to Handle:**
- [x] Pod Created (Running) → Create Endpoint
- [x] Pod IP assigned → Update endpoint mapping
- [x] Pod labels changed → Update identity
- [x] Pod Deleted → Clean up endpoint
- [x] Pod moved to different node → Update mapping
- [x] Container ID changes → Track updates

**BPF Maps to Update:**
- `cilium_endpoint` - Store endpoint metadata
- `cilium_ipcache` - Store pod IP → identity mapping
- `cilium_ct_*` - Connection tracking info

**Kubernetes Resources to Create:**
- `CiliumEndpoint` - Per-pod network configuration
- Update to `CiliumIdentity` - Pod security labels

**Tests Required:**
- [ ] Pod created → Endpoint created
- [ ] Pod deleted → Endpoint removed
- [ ] Pod IP change → Endpoint IP updated
- [ ] Pod moved to node → Endpoint re-created
- [ ] Label change → Identity re-assigned
- [ ] Endpoint map consistency after pod churn

---

### 4. eBPF Program Loading [HIGH]

**File:** crates/ebpf/src/programs.rs (extend existing)

**Specification:**
```rust
pub struct ServicePrograms {
    // XDP programs
    xdp_lb4: BpfProgram,     // IPv4 service load balancing
    xdp_lb6: BpfProgram,     // IPv6 service load balancing
    
    // TC programs
    tc_service_ingress: BpfProgram,
    tc_service_egress: BpfProgram,
    
    // Maps referenced by programs
    lb4_map: BpfMap,
    lb6_map: BpfMap,
    endpoint_map: BpfMap,
    ipcache_map: BpfMap,
}

impl ServicePrograms {
    pub fn load() -> Result<Self> {
        // 1. Mount BPF filesystem
        ensure_bpf_fs()?;
        
        // 2. Compile or load pre-compiled eBPF programs
        let xdp_lb4 = load_ebpf_program("service_lb4.o")?;
        let xdp_lb6 = load_ebpf_program("service_lb6.o")?;
        let tc_service = load_ebpf_program("service_tc.o")?;
        
        // 3. Load BPF maps
        let lb4_map = load_or_create_map(
            "cilium_lb4_map",
            BPF_MAP_TYPE_HASH,
            12,      // key size (IP+port+proto)
            16,      // value size (backend info)
            65536,   // max entries
        )?;
        
        let lb6_map = load_or_create_map(
            "cilium_lb6_map",
            BPF_MAP_TYPE_HASH,
            32,      // key size (IPv6+port+proto)
            16,      // value size (backend info)
            65536,   // max entries
        )?;
        
        // 4. Attach programs to pod interfaces
        // This happens dynamically when endpoints created
        
        Ok(Self {
            xdp_lb4,
            xdp_lb6,
            tc_service_ingress,
            tc_service_egress,
            lb4_map,
            lb6_map,
            endpoint_map,
            ipcache_map,
        })
    }
    
    pub fn attach_to_endpoint(&self, endpoint: &Endpoint) -> Result<()> {
        // Attach XDP program to pod's veth interface
        let iface = format!("eth0");  // In pod namespace
        
        self.xdp_lb4.attach_xdp(&iface)?;
        self.xdp_lb6.attach_xdp(&iface)?;
        self.tc_service_ingress.attach_tc_ingress(&iface)?;
        self.tc_service_egress.attach_tc_egress(&iface)?;
        
        Ok(())
    }
}
```

**eBPF Program Pseudocode (service_lb4.o - XDP):**
```c
// Simplified XDP program for IPv4 service load balancing
SEC("xdp/entry")
int xdp_service_lb4(struct xdp_md *ctx) {
    // 1. Parse packet (Ethernet, IP, TCP/UDP headers)
    struct iphdr *iph = parse_ipv4(ctx);
    if (!iph) return XDP_PASS;
    
    // 2. Check if destination is service VIP
    struct service_key key = {
        .ip = iph->daddr,
        .port = parse_port(iph),  // From TCP/UDP header
        .proto = iph->protocol,
    };
    
    // 3. Look up in service map
    struct service_value *svc = bpf_map_lookup_elem(&cilium_lb4_map, &key);
    if (!svc) {
        return XDP_PASS;  // Not a service VIP, pass through
    }
    
    // 4. Select backend (simplified: round-robin)
    u32 backend_idx = (ctx->rx_queue_index) % svc->num_backends;
    struct backend_info backend = svc->backends[backend_idx];
    
    // 5. Rewrite packet destination to backend IP
    iph->daddr = backend.ip;
    
    // 6. Update checksum
    update_ipv4_checksum(iph);
    
    // 7. Record connection for return traffic
    struct conn_entry *conn = alloc_connection();
    conn->original_dip = key.ip;
    conn->backend_ip = backend.ip;
    bpf_map_update_elem(&cilium_ct_lb4, conn, ...);
    
    // 8. Redirect to backend
    return XDP_TX;  // or XDP_REDIRECT to different interface
}
```

**Maps Structure:**
```c
// cilium_lb4_map - Key
struct service_key {
    __be32 ip;        // Service VIP (e.g., 10.0.0.1)
    __u16 port;       // Service port (e.g., 80)
    __u8 proto;       // Protocol (IPPROTO_TCP, IPPROTO_UDP)
};

// cilium_lb4_map - Value
struct service_value {
    __u32 num_backends;
    struct {
        __be32 ip;
        __u16 port;
        __u8 state;  // UP, DOWN, DRAINING
    } backends[16];  // Up to 16 backends per service
};
```

**Tests Required:**
- [ ] eBPF programs compile successfully
- [ ] Programs load into kernel without errors
- [ ] Maps created with correct size/type
- [ ] Program can be attached to veth interface
- [ ] Program detaches cleanly on pod deletion
- [ ] Traffic matches service VIP reaches program
- [ ] Backend rewrite happens correctly

---

### 5. Service-to-Backend Mapping [HIGH]

**File:** crates/controller/src/service_backend.rs (NEW)

**Specification:**
```rust
pub struct ServiceBackendResolver {
    endpoint_cache: Arc<EndpointCache>,
    service_cache: Arc<ServiceCache>,
    ebpf: Arc<ServicePrograms>,
}

impl ServiceBackendResolver {
    pub async fn reconcile_service(&self, svc: &CiliumService) -> Result<()> {
        // 1. Resolve service selector to endpoint IPs
        let selector = &svc.selector;
        let backends = self.endpoint_cache.find_by_labels(selector)?;
        
        // 2. For each service port, create VIP → backends mapping
        for port in &svc.ports {
            let vip = (svc.cluster_ip, port.port);
            let protocols = vec![port.protocol];
            
            // 3. Write to eBPF map
            for proto in protocols {
                self.ebpf.update_lb4_map(vip, proto, &backends)?;
            }
        }
        
        // 4. Store in CRD (for observability)
        self.create_or_update_service_crd(svc, &backends)?;
        
        Ok(())
    }
    
    pub async fn on_endpoint_changed(&self, endpoint: &Endpoint) -> Result<()> {
        // Find all services that might use this endpoint
        let services = self.service_cache.find_by_selector(&endpoint.labels)?;
        
        // Reconcile each affected service
        for svc in services {
            self.reconcile_service(&svc).await?;
        }
        
        Ok(())
    }
}
```

**Workflow:**
1. Service observer detects: `service.default/nginx` created with selector `app=nginx`
2. Service backend resolver triggered
3. Query endpoint cache: Find all endpoints with label `app=nginx`
4. Found endpoints: `10.1.0.5:0`, `10.1.0.6:0`
5. Write to eBPF map:
   ```
   cilium_lb4_map[(10.0.0.1, 80, IPPROTO_TCP)] = {
       num_backends: 2,
       backends: [
           {ip: 10.1.0.5, port: 8080},
           {ip: 10.1.0.6, port: 8080},
       ]
   }
   ```
6. When pod sends traffic to 10.0.0.1:80 → eBPF chooses 10.1.0.5 or 10.1.0.6

**Tests Required:**
- [ ] Single service, single backend
- [ ] Single service, multiple backends
- [ ] Multiple services, overlapping backends
- [ ] Endpoint added → service map updated
- [ ] Endpoint removed → service map updated
- [ ] Selector changed → backends re-resolved
- [ ] Multiple ports per service
- [ ] Both TCP and UDP backends

---

## Integration Points

### 1. Agent Startup Sequence
```
main()
  → load_config()
  → init_tracing()
  → mount_bpf_fs()
  → load_service_ebpf_programs()  // NEW
  → create_ebpf_maps()            // NEW
  → create_cni_socket()           // NEW
  → bind_health_endpoint()        // NEW
  → start_daemon()
      → start_service_observer()  // NEW
      → start_endpoint_manager()  // NEW
      → start_health_check()      // NEW
      → accept_cni_requests()     // NEW
```

### 2. Kubernetes Event Handlers
```
On Pod Created:
  endpoint_manager → create_endpoint()
  identity_allocator → assign_identity()
  service_backend_resolver → on_endpoint_changed()
    → find_affected_services()
    → reconcile_service()
      → update_ebpf_lb4_map()

On Service Created:
  service_observer → translate_service()
  service_backend_resolver → reconcile_service()
    → find_matching_endpoints()
    → update_ebpf_lb4_map()
    → create_cilium_service_crd()

On Pod Labels Changed:
  identity_allocator → update_identity()
  service_backend_resolver → on_endpoint_changed()
    → reconcile_affected_services()

On Service Deleted:
  service_observer → remove_service()
  service_backend_resolver → remove_service_mappings()
    → remove_from_ebpf_lb4_map()
```

---

## Testing Strategy

### Unit Tests
- [ ] ServiceObserver translates K8s Service to CiliumService
- [ ] EndpointManager creates Endpoint from Pod
- [ ] ServiceBackendResolver resolves selectors to backends
- [ ] eBPF map lookups return correct backends

### Integration Tests (with kind cluster)
```gherkin
Feature: ClusterIP Service Load Balancing
  
  Scenario: Pod connects to service VIP
    Given a cluster with 2 worker nodes
    And a deployment with 2 pods labeled app=web
    When a service is created with selector app=web and clusterIP 10.0.0.1:80
    And pods have IPs 10.1.0.5 and 10.1.0.6
    Then a pod should be able to reach the service VIP
    And traffic should be balanced across both backends
    And response should come from one of the backends
  
  Scenario: Service with no backends
    Given a service with selector app=nonexistent
    When a pod tries to connect to the service VIP
    Then the connection should timeout
    And the eBPF maps should show 0 backends
  
  Scenario: Dynamic backend addition
    Given a service with 1 backend pod
    When a second pod matching the service selector is created
    Then the service should immediately use both backends
    And existing connections should not be interrupted
```

### Verification Commands
```bash
# After test setup:

# 1. Verify eBPF maps populated
bpftool map dump id <lb4_map_id>

# 2. Verify endpoints created
kubectl get ciliumendpoints -n default

# 3. Verify service connectivity
POD=$(kubectl get pod -l app=client -o name)
kubectl exec $POD -- curl http://10.0.0.1:80/

# 4. Verify load balancing
for i in {1..100}; do
  kubectl exec $POD -- curl -s http://10.0.0.1:80/ | grep hostname
done
# Should see both backend hostnames

# 5. Check agent logs
kubectl logs -n kube-system -l k8s-app=cilium \
  | grep -E "service.*created|backend.*added|map.*updated"
```

---

## Implementation Order

1. **Phase 1:** CNI socket + health endpoint (fixes cluster bootstrap)
2. **Phase 2:** Service observer (gets Service objects into agent)
3. **Phase 3:** Endpoint manager (gets Pod endpoints into agent)
4. **Phase 4:** eBPF program loading (enables datapath)
5. **Phase 5:** Service-backend mapping (connects VIP → endpoints)
6. **Phase 6:** Testing and hardening

---

## Success Criteria

- [x] K8sDatapathServicesTest::BeforeEach completes without timeout
- [x] At least 1 service scenario passes (e.g., basic ClusterIP)
- [x] At least 25% of 50 service tests pass
- [x] Agent health check passes
- [x] CNI socket responsive
- [x] eBPF service maps populated
- [x] Pod can reach service VIP
- [x] Load balancing distributes traffic

---

**Document Version:** 1.0  
**Created:** 2026-05-11  
**Status:** IMPLEMENTATION_READY
