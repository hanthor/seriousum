# Seriousum Troubleshooting Guide

**Common issues and solutions for Seriousum Cilium**

---

## 📋 Quick Diagnosis

### System Check

```bash
#!/bin/bash
# Run this to diagnose common issues

echo "=== SYSTEM CHECK ==="
echo ""

echo "1. Kubernetes Version:"
kubectl version --short || echo "kubectl not found"

echo ""
echo "2. Linux Kernel Version:"
uname -r
echo "   (Should be 5.8+)"

echo ""
echo "3. Container Runtime:"
docker --version 2>/dev/null || echo "Docker not found"
podman --version 2>/dev/null || echo "Podman not found"

echo ""
echo "4. Seriousum Agent Status:"
kubectl get pods -n kube-system -l k8s-app=cilium

echo ""
echo "5. Node Status:"
kubectl get nodes

echo ""
echo "6. Network Status:"
ip route show
ip link show

echo ""
echo "7. eBPF Support:"
[ -d /sys/kernel/debug/tracing ] && echo "✅ Tracing available" || echo "❌ Tracing not available"
[ -d /sys/fs/bpf ] && echo "✅ BPF FS available" || echo "❌ BPF FS not available"
```

---

## 🔴 Agent Not Starting

### Issue: Pods stuck in Pending/CrashLoopBackOff

```bash
# Check detailed status
kubectl describe pod -n kube-system -l k8s-app=cilium
kubectl logs -n kube-system -l k8s-app=cilium --tail=100

# Check node conditions
kubectl describe node <node-name>

# Check kernel capabilities
kubectl get pod -n kube-system -l k8s-app=cilium -o yaml | grep -A 10 securityContext
```

### Solutions

#### 1. Insufficient Privileges

**Symptom**: Permission denied errors in logs

```
Error: Operation not permitted
Error: Cannot load eBPF program
```

**Fix**: Verify capabilities

```bash
# Check YAML for required capabilities
kubectl get daemonset cilium -n kube-system -o yaml | grep -A 5 "capabilities:"

# Should include:
# - CAP_NET_ADMIN
# - CAP_SYS_ADMIN
# - CAP_SYS_RESOURCE
# - CAP_NET_RAW

# Restart pods
kubectl rollout restart daemonset cilium -n kube-system
```

#### 2. Missing eBPF Support

**Symptom**: eBPF program load failures

```
Error: ebpf program load failed
Error: No such file or directory (BPF FS)
```

**Fix**: Check and enable eBPF

```bash
# Check kernel support
grep "CONFIG_BPF=" /boot/config-$(uname -r)
grep "CONFIG_BPF_SYSCALL=" /boot/config-$(uname -r)

# Mount BPF filesystem
sudo mount -t bpf bpf /sys/fs/bpf

# Make persistent
echo "bpf /sys/fs/bpf bpf defaults 0 0" | sudo tee -a /etc/fstab
```

#### 3. CNI Plugin Not Found

**Symptom**: Pods fail to get network interfaces

```
Error: failed to setup container networking
Error: CNI plugin not available
```

**Fix**: Ensure CNI is installed

```bash
# Check CNI plugin
ls -la /etc/cni/net.d/
ls -la /opt/cni/bin/

# Reinstall
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set cni.install=true
```

---

## 🟡 Connectivity Issues

### Issue: Pods can't communicate

```bash
# Test connectivity
kubectl run -it --rm debug --image=curlimages/curl -- sh
# Inside the pod:
curl http://kubernetes.default.svc.cluster.local:443
curl http://8.8.8.8
```

### Solutions

#### 1. Network Policy Blocking Traffic

**Symptom**: Connectivity works after disabling policies

```bash
# Check applied policies
kubectl get networkpolicies -A
kubectl describe networkpolicy -n <namespace> <policy-name>

# Temporarily disable enforcement
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set policyEnforcement=never

# Check connectivity
# If it works, policy is the issue

# Re-enable with debug
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set policyEnforcement=default \
  --set debug=true
```

#### 2. Endpoint Not Ready

**Symptom**: Agent sees endpoint but traffic fails

```bash
# Check endpoints
cilium endpoint list

# Expected: "OK" status for all endpoints

# If problematic endpoint, regenerate it
cilium endpoint regenerate <endpoint-id>

# View endpoint details
cilium endpoint get <endpoint-id>
```

#### 3. Routing Issues

**Symptom**: Packets routed incorrectly or dropped

```bash
# Check routing table
ip route show
ip route show table local

# Check neighbor table
ip neighbor show
ip neighbor show table local

# Check with traceroute
kubectl run -it --rm debug --image=nicolaka/netshoot -- sh
# Inside pod:
traceroute -n 8.8.8.8
traceroute -n <other-pod-ip>
```

**Fix**: Verify network configuration

```bash
# Check tunnel configuration
cilium status | grep -A 10 "Tunnel"

# Verify eBPF state
cilium bpf tunnel list
cilium bpf route list

# If routes missing, restart agent
kubectl rollout restart daemonset cilium -n kube-system
```

---

## 📊 Performance Issues

### Issue: High CPU/Memory Usage

```bash
# Check resource usage
kubectl top pods -n kube-system -l k8s-app=cilium
kubectl top nodes

# Check resource limits
kubectl get daemonset cilium -n kube-system -o yaml | grep -A 5 "resources:"
```

### Solutions

#### 1. Excessive Logging

**Symptom**: High CPU due to debug logs

```bash
# Check log level
kubectl exec -n kube-system -it daemonset/cilium -- cilium config get log-level

# Disable debug logging
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set debug=false \
  --set logLevel=info
```

#### 2. High Policy Evaluation

**Symptom**: CPU spikes during policy changes

```bash
# Monitor policy updates
kubectl logs -n kube-system -l k8s-app=cilium -f | grep -i "policy"

# Check endpoint count
cilium endpoint list | wc -l

# If many endpoints, consider increasing resources
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set resources.limits.cpu=2000m \
  --set resources.limits.memory=2048Mi
```

#### 3. Memory Leaks

**Symptom**: Memory continuously increases

```bash
# Check Go runtime stats (if available)
kubectl exec -n kube-system -it daemonset/cilium -- \
  curl localhost:6060/debug/pprof/heap | head -20

# If persistent, restart agent
kubectl rollout restart daemonset cilium -n kube-system

# Monitor growth
watch -n 5 'kubectl top pod -n kube-system -l k8s-app=cilium'
```

---

## 🔧 Configuration Issues

### Issue: Changes Not Applied

**Symptom**: Helm values or CLI changes don't take effect

```bash
# Verify values applied
helm get values cilium --namespace kube-system

# Check running config
cilium config

# Check for overrides
kubectl get configmap -n kube-system | grep cilium
kubectl describe configmap cilium-config -n kube-system
```

### Solutions

#### 1. ConfigMap Not Updated

```bash
# Manually trigger rollout
kubectl rollout restart daemonset cilium -n kube-system

# Verify new config is loaded
kubectl logs -n kube-system -l k8s-app=cilium | grep -i "config"
```

#### 2. Agent Caching Old Values

```bash
# Clear agent cache
kubectl exec -n kube-system -it daemonset/cilium -- \
  rm -rf /var/run/cilium/cache/*

# Restart agent
kubectl rollout restart daemonset cilium -n kube-system
```

---

## 🌐 ClusterMesh Issues

### Issue: Multi-cluster Connection Failed

```bash
# Check ClusterMesh status
cilium clustermesh status

# Check remote cluster visibility
cilium clustermesh list

# Test connectivity between clusters
# From cluster A:
kubectl run -it --rm debug --image=nicolaka/netshoot -- sh
# To pod in cluster B:
curl <remote-pod-ip>:8080
```

### Solutions

#### 1. Etcd Not Reachable

```bash
# Verify etcd connection
cilium config get cluster-pool-ipv4-cidr

# Test etcd access
kubectl exec -n kube-system -it daemonset/cilium -- \
  cilium kvstore get --prefix cilium
```

#### 2. KVStore Sync Issues

```bash
# Force re-sync
kubectl rollout restart daemonset cilium -n kube-system

# Monitor KVStore
kubectl logs -n kube-system -l k8s-app=cilium -f | grep -i "kvstore"
```

---

## 🔐 Security Policy Issues

### Issue: Too Restrictive/Permissive

```bash
# Audit policy violations
kubectl logs -n kube-system -l k8s-app=cilium | grep -i "policy.*deny"

# Check which endpoints are affected
cilium endpoint list -o wide

# Review policies
kubectl get networkpolicies -A -o wide
```

### Solutions

#### 1. Unexpected Drops

```bash
# Enable monitoring
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set policyMonitoring=true

# Check drop reasons
kubectl logs -n kube-system -l k8s-app=cilium | grep "DROP"

# Identify problematic rules
kubectl get networkpolicy -A -o yaml | grep -A 5 "rules:"
```

#### 2. Policy Not Enforced

```bash
# Verify policy enforcement is enabled
cilium config | grep "enable-policy-enforcement"

# Check specific pod labels
kubectl get pod <pod-name> -o yaml | grep "labels:"

# Ensure labels match policy selectors
kubectl get networkpolicy -A -o yaml | grep -A 3 "podSelector:"
```

---

## 📡 Hubble/Observability Issues

### Issue: No Flow Visibility

```bash
# Check Hubble status
cilium hubble status

# Check Hubble relay
kubectl get pod -n kube-system -l k8s-app=hubble-relay

# Query flows
hubble observe --follow
```

### Solutions

#### 1. Hubble Relay Not Running

```bash
# Enable Hubble
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set hubble.enabled=true \
  --set hubble.relay.enabled=true

# Verify
kubectl get pod -n kube-system -l k8s-app=hubble-relay
```

#### 2. No Flows in Database

```bash
# Check Hubble configuration
kubectl get configmap hubble-config -n kube-system

# Restart Hubble components
kubectl rollout restart daemonset cilium -n kube-system
kubectl rollout restart deployment hubble-relay -n kube-system

# Monitor flow generation
kubectl logs -n kube-system -l k8s-app=cilium -f | grep -i "flow"
```

---

## 🐛 Debugging Commands

### Enable Verbose Logging

```bash
# Increase log verbosity
helm upgrade cilium seriousum/seriousum \
  --namespace kube-system \
  --set debug=true \
  --set logLevel=debug

# View debug logs
kubectl logs -n kube-system -l k8s-app=cilium -f --tail=200
```

### Collect Diagnostics

```bash
#!/bin/bash
# Capture system diagnostics for support

mkdir -p seriousum-diags
cd seriousum-diags

# K8s info
kubectl version > k8s-version.txt 2>&1
kubectl cluster-info > k8s-info.txt 2>&1
kubectl get nodes > k8s-nodes.txt 2>&1

# Network info
ip route > network-routes.txt 2>&1
ip link > network-links.txt 2>&1

# Seriousum info
kubectl describe daemonset cilium -n kube-system > cilium-daemonset.txt 2>&1
kubectl describe deployment cilium-operator -n kube-system > cilium-operator.txt 2>&1
kubectl get pods -n kube-system -l k8s-app=cilium -o yaml > cilium-pods.yaml 2>&1

# Logs
kubectl logs -n kube-system -l k8s-app=cilium --tail=1000 > cilium-logs.txt 2>&1
kubectl logs -n kube-system -l k8s-app=cilium-operator --tail=1000 > operator-logs.txt 2>&1

# Config
kubectl get configmap cilium-config -n kube-system -o yaml > cilium-config.yaml 2>&1

echo "Diagnostics collected in seriousum-diags/"
tar czf seriousum-diags.tar.gz seriousum-diags/
echo "Archive: seriousum-diags.tar.gz"
```

---

## 📞 Getting Help

### Before Filing Issues

1. **Run diagnostics**: Use the debug commands above
2. **Check logs**: `kubectl logs -n kube-system -l k8s-app=cilium`
3. **Verify version**: `cilium version`
4. **Test connectivity**: Use test pods to isolate issues

### Report Issues

**GitHub**: https://github.com/hanthor/seriousum/issues

Include:
- Seriousum version
- Kubernetes version
- Linux kernel version
- Pod logs (last 50-100 lines)
- Network configuration
- Exact reproduction steps

### Resources

- **[Installation Guide](INSTALLATION.md)** - Setup help
- **[Developer Guide](DEVELOPER_GUIDE.md)** - Build help
- **[Cilium Docs](https://docs.cilium.io/)** - General networking
- **[GitHub Discussions](https://github.com/hanthor/seriousum/discussions)** - Community help

---

**Last Updated**: May 11, 2026  
**Version**: v0.1.0-alpha
