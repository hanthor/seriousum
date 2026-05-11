# Justfile Recipes — Quick Start

## Common Workflows

### Fresh Setup (Everything)
```bash
just setup
```
Builds Rust binaries, builds images, creates a kind cluster, and loads images. 

### Run a Single Test
```bash
just test-services           # K8sDatapathServicesTest
just test-policies           # K8sNetworkPoliciesTest
just test-hubble             # K8sHubbleTest
just test-fqdn               # K8sFQDNTest
```

### Run Any Focused Suite
```bash
just test "MyCustomTest" "5m"
```

### Parallel Matrix Testing
```bash
just test-matrix "15m"
```
Runs 12 focused suites in parallel across separate kind clusters.

### Debug Mode (Keep Cluster Running on Failure)
```bash
just test-debug "K8sDatapathServicesTest" "10m"
```
Environment is held for inspection; run `kill -SIGCONT <pid>` to resume.

## Cluster Management

```bash
just cluster-status      # Show pods in the cluster
just cluster-reset       # Delete and recreate the cluster
just cluster-delete      # Remove the cluster entirely
```

## Inspection & Logs

```bash
just logs-agent          # Tail cilium-agent logs
just logs-operator       # Tail cilium-operator logs
just logs <pod-name>     # Tail any pod
just describe <pod>      # kubectl describe
```

## Building

```bash
just build-images        # Build Rust images only
just load-images         # Load already-built images into kind
just build               # Compile release binaries
just check               # Run cargo check + clippy
just validate            # Validate shell scripts
```

## Cleanup

```bash
just clean               # Remove build artifacts (keep cluster)
just clean-all           # Remove everything including cluster
```

## Show Environment
```bash
just env
```

## Examples

**Single test with 12-minute timeout:**
```bash
just test-services "12m"
```

**Build, load, and run full matrix:**
```bash
just build-and-load && just test-matrix "20m"
```

**Quick smoke test (2 min timeout):**
```bash
just smoke
```

**Hold environment for debugging:**
```bash
just test-debug "K8sNetworkPoliciesTest" "15m"
# After failure, inspect pods:
just logs-agent
just cluster-status
# Kill -SIGCONT to resume
```
