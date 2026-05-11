# CRD Sync Fixes - Implementation Guide

**Objective:** Resolve operator-agent CRD sync issues identified in verification

**Current State:** CRD sync blocked by operator image pull failure and agent startup timeouts

---

## Summary of Issues

| Priority | Issue | Root Cause | Impact |
|----------|-------|-----------|--------|
| **P0** | Operator image pull fails (401 UNAUTHORIZED) | Image registry authentication | Operator never starts; CRDs not created |
| **P0** | Agent startup probe fails | Health check endpoint timeout | Agent never becomes ready |
| **P1** | No explicit CRD wait logic | Missing in agent startup | Race conditions possible |
| **P2** | CRD fields not validated | No schema verification | Silent failures if CRD incomplete |
| **P2** | No sync observability | Missing metrics/logs | Hard to debug failures |

---

## Fix 1: Operator Image Authentication (P0 - IMMEDIATE)

### Problem
```
cilium-operator pod status: ImagePullBackOff
Error: failed to resolve reference "quay.io/cilium/cilium-ci-generic:latest": 
       401 UNAUTHORIZED
```

### Solution A: Use Local Built Image

```bash
# 1. Build the operator image locally
cd seriousum
docker build -f crates/operator/Dockerfile -t localhost:5000/seriousum/cilium-operator:local .

# 2. Load into Kind cluster
kind load docker-image --name=kind localhost:5000/seriousum/cilium-operator:local

# 3. Update Cilium deployment to use local image
kubectl set image deployment/cilium-operator \
  -n kube-system \
  cilium-operator=localhost:5000/seriousum/cilium-operator:local \
  --record

# 4. Verify
kubectl rollout status deployment/cilium-operator -n kube-system
```

### Solution B: Create Image Pull Secret

```bash
# 1. Create registry credentials secret
kubectl create secret docker-registry quay-creds \
  -n kube-system \
  --docker-server=quay.io \
  --docker-username=<USERNAME> \
  --docker-password=<TOKEN> \
  --docker-email=<EMAIL>

# 2. Add to service account
kubectl patch serviceaccount cilium-operator \
  -n kube-system \
  -p '{"imagePullSecrets": [{"name": "quay-creds"}]}'

# 3. Trigger pod restart
kubectl rollout restart deployment/cilium-operator -n kube-system
```

### Solution C: Use Alternative Image

```bash
# 1. Edit Cilium values to use upstream public image
cat > /tmp/cilium-values.yaml <<EOF
image:
  repository: quay.io/cilium/cilium
  tag: latest  # or specific version like v1.13.0

operator:
  image:
    repository: quay.io/cilium/operator  # Use operator image instead of cilium-ci-generic
    tag: latest
EOF

# 2. Update deployment
helm upgrade cilium cilium/cilium \
  -n kube-system \
  -f /tmp/cilium-values.yaml

# 3. Wait for rollout
kubectl rollout status deployment/cilium-operator -n kube-system
```

### Verification
```bash
# Check if operator is now running
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator

# Expected: STATUS=Running, READY=1/1

# Verify CRD creation started
kubectl get crd | grep cilium | wc -l
# Expected: ~9 CRDs (or increasing count)

# Check logs
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator \
  | grep -i 'crd\|registered' | head -10
```

---

## Fix 2: Agent Startup Health Check (P0 - IMMEDIATE)

### Problem
```
cilium-agent pod status: Running but unhealthy
Startup probe failed: Get "http://127.0.0.1:9879/healthz": 
                      dial tcp 127.0.0.1:9879: connect: connection refused
```

### Root Cause Analysis

The agent health endpoint is not responding. This could be due to:

1. **Agent process not started**: Process crashed immediately
2. **Port 9879 not bound**: Health server never initialized
3. **Resource exhaustion**: OOM, CPU throttle preventing startup
4. **BPF subsystem unavailable**: Kernel doesn't support required features
5. **RBAC permissions**: Agent can't access required resources

### Debug Steps

```bash
# 1. Get agent pod details
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl describe pod $AGENT_POD -n kube-system

# 2. Check for previous run logs (if pod crashed)
kubectl logs $AGENT_POD -n kube-system --previous

# 3. Check node resources
NODE=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].spec.nodeName}')
kubectl describe node $NODE

# 4. Check kernel BPF support
kubectl debug node $NODE -it -- bash -c 'grep BPF /boot/config-*'

# 5. Check RBAC permissions
kubectl auth can-i create ciliumnodes --as=system:serviceaccount:kube-system:cilium
kubectl auth can-i get ciliumendpoints --as=system:serviceaccount:kube-system:cilium
```

### Solution A: Increase Resource Limits

```bash
# Edit DaemonSet
kubectl edit ds cilium -n kube-system

# Update resources section:
# spec:
#   template:
#     spec:
#       containers:
#       - name: cilium-agent
#         resources:
#           limits:
#             cpu: 2000m          # Increase from 1000m
#             memory: 1Gi         # Increase from 512Mi
#           requests:
#             cpu: 1000m
#             memory: 512Mi

# Trigger rollout
kubectl rollout restart ds/cilium -n kube-system

# Verify
kubectl top pods -n kube-system -l k8s-app=cilium
```

### Solution B: Verify BPF Subsystem

```bash
# On a node, check BPF support
kubectl debug node <node-name> -it -- bash

# Inside node shell:
cat /proc/sys/kernel/unprivileged_bpf_disabled
# Should be 0 (BPF enabled for all) or 1 (restricted, but allowed for privileged)

grep -i BPF /boot/config-*
# Should show CONFIG_BPF=y and related BPF configs

# Check eBPF programs are loading
bpftool prog list
# Should show Cilium eBPF programs
```

### Solution C: Increase Startup Probe Timeout

```bash
# Edit DaemonSet
kubectl edit ds cilium -n kube-system

# Update startup probe:
# spec:
#   template:
#     spec:
#       containers:
#       - name: cilium-agent
#         startupProbe:
#           httpGet:
#             path: /healthz
#             port: 9879
#           failureThreshold: 30    # Increase from default
#           periodSeconds: 10       # Check every 10s instead of default
#         # Total timeout: 30 * 10 = 300 seconds (5 minutes)

# Trigger rollout
kubectl rollout restart ds/cilium -n kube-system
```

### Verification
```bash
# Check agent health
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl exec -it $AGENT_POD -n kube-system -- cilium status

# Expected output should show:
# /healthz:           OK
# Cilium version:     ...
# Agent features:     ...

# Check CNI socket exists
kubectl debug node $(kubectl get nodes -o jsonpath='{.items[0].metadata.name}') -it -- \
  ls -la /var/run/cilium/cilium.sock
# Should exist and be a socket (S)
```

---

## Fix 3: Add Explicit CRD Wait Logic (P1 - HIGH)

### Current State
The agent may start and try to initialize datapath before the operator has created CRDs, causing race conditions.

### Required Changes

#### In agent startup code (daemon/agent initialization):

```rust
// File: crates/daemon/src/lib.rs

use std::time::Duration;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::{Api, Client};

/// Wait for operator to create required CRDs
async fn wait_for_crds() -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let crd_api: Api<CustomResourceDefinition> = Api::all(client.clone());
    
    let required_crds = vec![
        "ciliumnodes.cilium.io",
        "ciliumendpoints.cilium.io",
        "ciliumnetworkpolicies.cilium.io",
        "ciliumclusterwidenetworkpolicies.cilium.io",
        "ciliumidentities.cilium.io",
    ];
    
    for crd_name in required_crds {
        let timeout = Duration::from_secs(60);
        let start = std::time::Instant::now();
        
        loop {
            match crd_api.get(crd_name).await {
                Ok(_) => {
                    info!(crd = crd_name, "CRD registered and available");
                    break;
                }
                Err(_) => {
                    if start.elapsed() > timeout {
                        return Err(anyhow::anyhow!(
                            "Timeout waiting for CRD '{}' after {:?}",
                            crd_name,
                            timeout
                        ));
                    }
                    warn!(crd = crd_name, "Waiting for CRD registration...");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
    
    info!("All required CRDs are registered");
    Ok(())
}

/// Update daemon startup sequence
impl Daemon {
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Starting seriousum daemon");
        
        // STEP 1: Wait for operator to create CRDs
        info!("Waiting for operator to register CRDs...");
        wait_for_crds().await?;
        
        // STEP 2: Verify CRD schema
        info!("Verifying CRD schema...");
        verify_crd_schema().await?;
        
        // STEP 3: Create/update node resource
        info!("Creating CiliumNode resource...");
        create_cilium_node_resource().await?;
        
        // STEP 4: Initialize datapath
        info!("Initializing datapath...");
        self.store.set("daemon/state", b"running".to_vec()).await;
        
        // STEP 5: Wait for node resource population
        info!("Waiting for operator to populate node resource...");
        wait_for_node_populated().await?;
        
        // STEP 6: Report ready
        info!("Daemon initialization complete - reporting healthy");
        
        Ok(())
    }
}
```

### Add to Cargo.toml
```toml
[dependencies]
k8s-openapi = { version = "0.19", features = ["v1_27"] }
kube = { version = "0.84", features = ["client", "runtime"] }
```

### Verification
```bash
# Check agent logs for CRD wait messages
kubectl logs -n kube-system -l k8s-app=cilium | grep -i 'crd\|registr\|wait'

# Expected:
# "Waiting for operator to register CRDs..."
# "CRD ciliumnodes.cilium.io registered and available"
# "All required CRDs are registered"
# "Agent initialization complete"
```

---

## Fix 4: Add CRD Field Validation (P2 - MEDIUM)

### Add validation function to agent startup:

```rust
/// Verify CRD has expected fields
async fn verify_crd_schema() -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let crd_api: Api<CustomResourceDefinition> = Api::all(client);
    
    // Check CiliumNode CRD
    let crd = crd_api.get("ciliumnodes.cilium.io").await?;
    
    let openapi_schema = crd
        .spec
        .names
        .kind
        .clone();
    
    // Verify schema includes spec and status
    let schema = &crd.spec.validation;
    
    match schema {
        Some(v) => {
            if let Some(schema) = &v.open_api_v3_schema {
                let properties = schema.properties.as_ref().unwrap_or(&Default::default());
                
                if !properties.contains_key("spec") {
                    return Err(anyhow::anyhow!("CRD missing 'spec' field"));
                }
                if !properties.contains_key("status") {
                    return Err(anyhow::anyhow!("CRD missing 'status' field"));
                }
                
                info!("CRD schema validation successful");
                Ok(())
            } else {
                warn!("Could not verify CRD schema - validation not available");
                Ok(())
            }
        }
        None => {
            warn!("CRD validation schema not found");
            Ok(())
        }
    }
}
```

---

## Fix 5: Add Sync Observability (P2 - MEDIUM)

### Add metrics:

```rust
/// Metrics for CRD sync
pub mod metrics {
    use prometheus::{histogram_vec, counter_vec, gauge_vec, register_histogram_vec, 
                      register_counter_vec, register_gauge_vec};
    use lazy_static::lazy_static;
    
    lazy_static! {
        pub static ref OPERATOR_CRD_REGISTRATION_DURATION: prometheus::HistogramVec =
            register_histogram_vec!(
                "cilium_operator_crd_registration_duration_seconds",
                "Time taken to register each CRD",
                &["crd_name"]
            ).unwrap();
        
        pub static ref AGENT_CRD_SYNC_DURATION: prometheus::HistogramVec =
            register_histogram_vec!(
                "cilium_agent_crd_sync_duration_seconds",
                "Time taken for agent to sync with CRD",
                &["crd_name"]
            ).unwrap();
        
        pub static ref CRD_SYNC_SUCCESS: prometheus::CounterVec =
            register_counter_vec!(
                "cilium_crd_sync_success_total",
                "Successful CRD syncs",
                &["component", "crd_name"]
            ).unwrap();
        
        pub static ref CRD_SYNC_ERRORS: prometheus::CounterVec =
            register_counter_vec!(
                "cilium_crd_sync_errors_total",
                "Failed CRD syncs",
                &["component", "crd_name", "error_type"]
            ).unwrap();
        
        pub static ref CRD_FIELDS_POPULATED: prometheus::GaugeVec =
            register_gauge_vec!(
                "cilium_crd_fields_populated",
                "Number of populated fields in CRD",
                &["crd_name", "resource_name"]
            ).unwrap();
    }
}

/// Add metrics recording
fn wait_for_crds_with_metrics() -> anyhow::Result<()> {
    // ... (previous code with added metrics)
    
    for crd_name in required_crds {
        let start = std::time::Instant::now();
        
        // ... wait logic ...
        
        let duration = start.elapsed();
        metrics::AGENT_CRD_SYNC_DURATION
            .with_label_values(&[crd_name])
            .observe(duration.as_secs_f64());
        
        metrics::CRD_SYNC_SUCCESS
            .with_label_values(&["agent", crd_name])
            .inc();
    }
}
```

### Add logging:

```rust
// In operator CRD registration:
info!(
    crd_name = "CiliumNode",
    timestamp = %chrono::Utc::now(),
    "CRD registration started"
);
info!(
    crd_name = "CiliumNode",
    field_count = 12,
    established = true,
    "CRD registration complete"
);

// In agent CRD wait:
info!(
    crd_name = "CiliumNode",
    wait_duration_ms = duration.as_millis(),
    "CRD available - agent observing"
);
info!(
    node_name = node.metadata.name,
    fields_populated = 8,
    "Operator populated CiliumNode resource"
);
```

---

## Verification Checklist

After applying all fixes:

```bash
# 1. Operator is running
kubectl get pods -n kube-system -l app.kubernetes.io/name=cilium-operator
# Expected: Running, Ready 1/1

# 2. CRDs are created (~9 total)
kubectl get crd | grep cilium | wc -l

# 3. Agent pods are ready
kubectl get ds cilium -n kube-system
# Expected: READY count = DESIRED count

# 4. CNI socket exists
kubectl debug node $(kubectl get nodes -o jsonpath='{.items[0].metadata.name}') -it -- \
  ls -la /var/run/cilium/cilium.sock

# 5. Agent is healthy
AGENT_POD=$(kubectl get pods -n kube-system -l k8s-app=cilium -o jsonpath='{.items[0].metadata.name}')
kubectl exec -it $AGENT_POD -n kube-system -- cilium status

# 6. Operator logs show CRD registration
kubectl logs -n kube-system -l app.kubernetes.io/name=cilium-operator | grep "registered"

# 7. Agent logs show CRD sync
kubectl logs -n kube-system -l k8s-app=cilium | grep "CRD"

# 8. CiliumNode resources present
kubectl get ciliumnodes

# 9. Metrics are being recorded (if applicable)
kubectl port-forward -n kube-system svc/cilium-agent 9090:9090
curl localhost:9090/metrics | grep cilium_crd
```

---

## Implementation Priority

1. **Fix 1 + Fix 2** (P0): **Today** - Required for any testing to proceed
2. **Fix 3** (P1): **This Sprint** - Prevents race conditions
3. **Fix 4 + Fix 5** (P2): **Next Sprint** - Improves observability

