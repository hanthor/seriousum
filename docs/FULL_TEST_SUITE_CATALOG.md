# Cilium Full Integration Test Suite Catalog

**Created**: 2026-05-11  
**Sources**: Upstream Cilium repo, cilium-cli, Kubernetes sig-network, Gateway API conformance, upstream CI matrix  
**Purpose**: Track every known integration test and wire them all into the seriousum test runner  
**GitHub Issue**: #59  

---

## Source Overview

| Source | Tests | Type | Location |
|--------|-------|------|----------|
| Cilium ginkgo suites (k8s/) | 19 focus groups, ~80 individual specs | E2E, Kubernetes | `test/k8s/` |
| Cilium ginkgo suites (runtime/) | 1 suite, ~5 specs | E2E, Docker runtime | `test/runtime/` |
| Cilium control-plane | 1 suite | Unit/integration | `test/controlplane/` |
| cilium-cli connectivity | 95 builder scenarios | CLI-driven E2E | `cilium-cli/connectivity/builder/` |
| cilium-cli check scenarios | 27 check modules | Assertion helpers | `cilium-cli/connectivity/check/` |
| Gateway API conformance | Full sig-gateway suite | Conformance | `operator/pkg/gateway-api/` |
| Kubernetes sig-network | [sig-network] full suite | Upstream K8s | External (kubernetes/kubernetes) |
| Kubernetes network policies | Netpol/NetworkPolicy | Upstream K8s | External (kubernetes/kubernetes) |
| CNCF CNI conformance | sonobuoy/cni-conformance | Conformance | External |
| MCS-API conformance | ClusterSet / multicluster | Conformance | `pkg/clustermesh/mcsapi/conformance/` |

**Total known tests**: 200+ individual specs + 95 cilium-cli scenarios

---

## Category 1: Cilium Ginkgo K8s Suites

These map directly to the upstream CI focus matrix (`main-focus.yaml`).

### f01 — Agent Chaos (`K8sAgentChaosTest`)

| Test Spec | File |
|-----------|------|
| Graceful shutdown exits with a success message on SIGTERM | `test/k8s/chaos.go` |
| Endpoint can still connect while Cilium is not running | `test/k8s/chaos.go` |
| L3/L4 policies still work while Cilium is restarted | `test/k8s/chaos.go` |
| TCP connection is not dropped when cilium restarts | `test/k8s/chaos.go` |

**ginkgo focus regex**: `K8sAgentChaosTest`

---

### f02 — Agent FQDN (`K8sAgentFQDNTest`, `K8sAgentPerNodeConfigTest`)

| Test Spec | File |
|-----------|------|
| Restart Cilium validate that FQDN is still working | `test/k8s/fqdn.go` |
| Validate that FQDN policy continues to work after being updated | `test/k8s/fqdn.go` |
| Validate that multiple specs are working correctly | `test/k8s/fqdn.go` |
| Correctly computes config overrides with CNC v2 | `test/k8s/` |

**ginkgo focus regex**: `K8sAgentFQDNTest|K8sAgentPerNodeConfigTest`

---

### f03 — Agent Policy (Namespaces/Clusterwide/External) (`K8sAgentPolicyTest`)

| Test Spec | File |
|-----------|------|
| Clusterwide policies — Test clusterwide connectivity with policies | `test/k8s/net_policies.go` |
| Clusterwide policies — Tests connectivity with default-allow policies | `test/k8s/net_policies.go` |
| External services — To Services first endpoint creation | `test/k8s/net_policies.go` |
| External services — To Services first endpoint creation match service by labels | `test/k8s/net_policies.go` |
| External services — To Services first policy | `test/k8s/net_policies.go` |
| External services — To Services first policy, match service by labels | `test/k8s/net_policies.go` |
| Namespaces policies — Cilium Network policy using namespace label and L7 | `test/k8s/net_policies.go` |
| Namespaces policies — Kubernetes Network Policy by namespace selector | `test/k8s/net_policies.go` |
| Namespaces policies — Tests the same Policy in different namespaces | `test/k8s/net_policies.go` |

**ginkgo focus regex**: `K8sAgentPolicyTest Clusterwide|K8sAgentPolicyTest External|K8sAgentPolicyTest Namespaces`

---

### f04 — Agent Policy Multi-Node 1 (`K8sAgentPolicyTest` multi-node fromEntities)

| Test Spec | File |
|-----------|------|
| Multi-node — Validates fromEntities all policy | `test/k8s/net_policies.go` |
| Multi-node — Validates fromEntities cluster policy | `test/k8s/net_policies.go` |
| Multi-node — Validates fromEntities remote-node policy (remote-node identity) | `test/k8s/net_policies.go` |
| Multi-node — using connectivity-check to check datapath (L7) | `test/k8s/net_policies.go` |

**ginkgo focus regex**: `K8sAgentPolicyTest Multi-node policy test validates fromEntities|K8sAgentPolicyTest Multi-node policy test with`

---

### f05 — Agent Policy Multi-Node 2 (`K8sAgentPolicyTest` ingress CIDR)

| Test Spec | File |
|-----------|------|
| Multi-node — validates ingress CIDR-dependent L4 — connectivity blocked after deny | `test/k8s/net_policies.go` |
| Multi-node — validates ingress CIDR-dependent L4 — connectivity restored after policy import | `test/k8s/net_policies.go` |
| Multi-node — validates ingress CIDR-dependent L4 — connectivity works before any policy | `test/k8s/net_policies.go` |
| Multi-node — With host policy — Connectivity is restored after importing ingress policy | `test/k8s/net_policies.go` |
| Multi-node — With host policy — Connectivity to hostns is blocked after denying ingress | `test/k8s/net_policies.go` |

**ginkgo focus regex**: `K8sAgentPolicyTest Multi-node policy test validates ingress`

---

### f06 — Agent Policy Basic + Extended (`K8sAgentPolicyTest Basic`, `K8sPolicyTestExtended`)

| Test Spec | File |
|-----------|------|
| Basic Test — Tests proxy visibility with L7 default-allow rules | `test/k8s/net_policies.go` |
| Basic Test — Tests proxy visibility with L7 rules | `test/k8s/net_policies.go` |
| K8sPolicyTestExtended — Allows connection to KubeAPIServer | `test/k8s/net_policies.go` |
| K8sPolicyTestExtended — Denies connection to KubeAPIServer | `test/k8s/net_policies.go` |
| K8sPolicyTestExtended — Still allows connection to KubeAPIServer with a duplicate policy | `test/k8s/net_policies.go` |

**ginkgo focus regex**: `K8sAgentPolicyTest Basic|K8sPolicyTestExtended`

---

### f10 — Agent Hubble & Bandwidth (`K8sAgentHubbleTest`)

| Test Spec | File |
|-----------|------|
| Hubble Observe — Test FQDN Policy with Relay | `test/k8s/hubble.go` |
| Hubble Observe — Test L3/L4 Flow | `test/k8s/hubble.go` |
| Hubble Observe — Test L3/L4 Flow with hubble-relay | `test/k8s/hubble.go` |
| Hubble Observe — Test L7 Flow | `test/k8s/hubble.go` |
| Hubble Observe — Test L7 Flow with hubble-relay | `test/k8s/hubble.go` |
| Hubble Observe — Test TLS certificate | `test/k8s/hubble.go` |

**ginkgo focus regex**: `K8sAgentHubbleTest`

---

### f11 — Datapath Services N/S TC (`K8sDatapathServicesTest` N/S TC)

| Test Spec | File |
|-----------|------|
| N/S LB — Tests with TC, direct routing and dsr with geneve | `test/k8s/services.go` |
| N/S LB — Tests with TC, direct routing and Hybrid-DSR with Geneve | `test/k8s/services.go` |
| N/S LB — Tests with TC, geneve tunnel, and Hybrid-DSR with Geneve | `test/k8s/services.go` |
| N/S LB — Tests with TC, direct routing and Hybrid | `test/k8s/services.go` |
| N/S LB — Tests with TC, geneve tunnel, dsr and Maglev | `test/k8s/services.go` |

**ginkgo focus regex**: `K8sDatapathServicesTest Checks N/S loadbalancing Tests with TC`

---

### f12 — Datapath Services N/S Misc

| Test Spec | File |
|-----------|------|
| N/S LB — Tests GH#10983 | `test/k8s/services.go` |
| N/S LB — Tests NodePort with sessionAffinity from outside | `test/k8s/services.go` |
| N/S LB — Tests security id propagation in N/S LB requests fwd-ed over tunnel | `test/k8s/services.go` |
| N/S LB — Tests with direct routing (variants) | `test/k8s/services.go` |
| N/S LB — with (various) | `test/k8s/services.go` |

**ginkgo focus regex**: `K8sDatapathServicesTest Checks N/S loadbalancing Tests GH|Tests NodePort|Tests security|Tests with direct|with`

---

### f13 — Datapath Services N/S XDP 1

| Test Spec | File |
|-----------|------|
| N/S LB — Tests with XDP, direct routing, DSR and Maglev | `test/k8s/services.go` |
| N/S LB — Tests with XDP, direct routing, DSR and Random | `test/k8s/services.go` |
| N/S LB — Tests with XDP, direct routing, DSR with Geneve and Maglev | `test/k8s/services.go` |
| N/S LB — Tests with XDP, direct routing, Hybrid and Maglev | `test/k8s/services.go` |
| N/S LB — Tests with XDP, direct routing, Hybrid and Random | `test/k8s/services.go` |

**ginkgo focus regex**: `K8sDatapathServicesTest Checks N/S loadbalancing Tests with XDP, direct routing, DSR|Hybrid`

---

### f14 — Datapath Services N/S XDP 2

| Test Spec | File |
|-----------|------|
| N/S LB — Tests with XDP, direct routing, SNAT and Maglev | `test/k8s/services.go` |
| N/S LB — With host policy Tests NodePort | `test/k8s/services.go` |

**ginkgo focus regex**: `K8sDatapathServicesTest Checks N/S loadbalancing Tests with XDP, direct routing, SNAT|With host policy Tests NodePort`

---

### f15 — Datapath Services E/W 1 (KPR in-cluster)

| Test Spec | File |
|-----------|------|
| Device reconfiguration — Detects newly added device and reloads datapath | `test/k8s/services.go` |
| E/W LB — Checks in-cluster KPR — Tests HealthCheckNodePort | `test/k8s/services.go` |
| E/W LB — Checks in-cluster KPR — Tests that binding to NodePort port fails | `test/k8s/services.go` |
| E/W LB — Checks in-cluster KPR with L7 policy — Tests NodePort with L7 Policy | `test/k8s/services.go` |

**ginkgo focus regex**: `K8sDatapathServicesTest Checks device|Checks in-cluster KPR`

---

### f16 — Datapath Services E/W 2 (hairpin, TFTP, L4/L7)

| Test Spec | File |
|-----------|------|
| E/W LB — Checks service accessing itself (hairpin flow) | `test/k8s/services.go` |
| E/W LB — TFTP with DNS Proxy port collision — Tests TFTP from DNS Proxy Port | `test/k8s/services.go` |
| E/W LB — with L4 policy — Tests NodePort with L4 Policy | `test/k8s/services.go` |
| E/W LB — with L7 policy — Tests NodePort with L7 Policy | `test/k8s/services.go` |

**ginkgo focus regex**: `Checks service|TFTP|with L4 policy|with L7 policy` (scoped to E/W)

---

### f17 — Datapath Services E/W kube-proxy

| Test Spec | File |
|-----------|------|
| E/W LB — Tests NodePort inside cluster (kube-proxy) vanilla | `test/k8s/services.go` |
| E/W LB — Tests NodePort inside cluster (kube-proxy) with externalTrafficPolicy=Local | `test/k8s/services.go` |
| E/W LB — Tests NodePort inside cluster (kube-proxy) with IPSec + externalTrafficPolicy=Local | `test/k8s/services.go` |
| E/W LB — Tests NodePort inside cluster (kube-proxy) with host firewall + externalTrafficPolicy=Local | `test/k8s/services.go` |

**ginkgo focus regex**: `K8sDatapathServicesTest Checks E/W loadbalancing.*Tests NodePort inside cluster`

---

### f18 — Datapath LRP (`K8sDatapathLRPTests`)

| Test Spec | File |
|-----------|------|
| LRP connectivity | `test/k8s/lrp.go` |
| LRP restores service when removed | `test/k8s/lrp.go` |

**ginkgo focus regex**: `K8sDatapathLRPTests`

---

### f19 — MAC Address (`K8sSpecificMACAddressTests`)

| Test Spec | File |
|-----------|------|
| Check whether the pod is created — Checks the pod's mac address | `test/k8s/pod_mac_address.go` |

**ginkgo focus regex**: `K8sSpecificMACAddressTests`

---

## Category 2: Cilium Runtime Tests

### RuntimeDatapathMonitorTest

| Test Spec | File |
|-----------|------|
| cilium-dbg monitor check --from | `test/runtime/monitor.go` |
| cilium-dbg monitor check --to | `test/runtime/monitor.go` |
| cilium-dbg monitor check --related-to | `test/runtime/monitor.go` |
| delivers the same information to multiple monitors | `test/runtime/monitor.go` |
| checks container ids match monitor output | `test/runtime/monitor.go` |

**ginkgo focus regex**: `RuntimeDatapathMonitorTest`

---

## Category 3: Control-Plane Tests

### TestControlPlane

| Test Spec | File |
|-----------|------|
| Node — CiliumNodes reconciliation | `test/controlplane/node/ciliumnodes/` |
| Node — Local node handler | `test/controlplane/node/localnode.go` |
| Node — Node handler | `test/controlplane/node/nodehandler.go` |
| Suite framework | `test/controlplane/suite/` |

**go test command**: `go test ./test/controlplane/...`

---

## Category 4: cilium-cli Connectivity Scenarios

These are run via `cilium connectivity test` and cover the full datapath end-to-end.

### Builder Scenarios (95 total)

| Scenario File | Description |
|--------------|-------------|
| `all_egress_deny` | Deny all egress traffic |
| `all_egress_deny_knp` | Deny all egress via KNP |
| `all_entities_deny` | Deny all entity policies |
| `all_ingress_deny` | Deny all ingress traffic |
| `all_ingress_deny_from_outside` | Deny ingress from outside cluster |
| `all_ingress_deny_knp` | Deny all ingress via KNP |
| `allow_all_except_world` | Allow all except world entity |
| `allow_all_with_metrics_check` | Allow all + verify metrics |
| `bgp_control_plane` | BGP control plane integration |
| `check_log_errors` | Verify no unexpected log errors |
| `client_egress` | Client egress policies |
| `client_egress_expression` | Client egress with CEL expressions |
| `client_egress_expression_knp` | Client egress expression via KNP |
| `client_egress_knp` | Client egress via KNP |
| `client_egress_l7` | Client egress L7 HTTP |
| `client_egress_l7_method` | Client egress L7 method matching |
| `client_egress_l7_named_port` | Client egress L7 named port |
| `client_egress_l7_set_header` | Client egress L7 header setting |
| `client_egress_l7_tls_deny_without_headers` | L7 TLS deny without required headers |
| `client_egress_l7_tls_headers` | L7 TLS with headers |
| `client_egress_tls_sni` | TLS SNI-based egress routing |
| `client_egress_to_cidr_deny` | Egress to CIDR deny |
| `client_egress_to_cidr_deny_default` | Egress to CIDR default deny |
| `client_egress_to_cidrgroup_deny` | Egress to CIDRGroup deny |
| `client_egress_to_echo_deny` | Egress to echo pod deny |
| `client_egress_to_echo_expression_deny` | Egress to echo with expression deny |
| `client_egress_to_echo_service_account` | Egress to echo by service account |
| `client_egress_to_echo_service_account_deny` | Egress to echo service account deny |
| `client_ingress` | Client ingress policies |
| `client_ingress_from_other_client_icmp_deny` | ICMP ingress deny from other client |
| `client_ingress_icmp` | ICMP ingress |
| `client_ingress_knp` | Client ingress via KNP |
| `client_ingress_to_echo_named_port_deny` | Ingress to echo named port deny |
| `client_with_service_account_egress_to_echo` | Service account egress to echo |
| `client_with_service_account_egress_to_echo_deny` | Service account egress deny |
| `cluster_entity` | Cluster entity policy |
| `cluster_entity_multi_cluster` | Cluster entity in multi-cluster |
| `dns_only` | DNS-only egress policy |
| `echo_ingress` | Echo pod ingress policies |
| `echo_ingress_auth_always_fail` | Echo ingress mutual auth always-fail |
| `echo_ingress_from_other_client_deny` | Echo ingress deny from other client |
| `echo_ingress_from_outside` | Echo ingress from outside cluster |
| `echo_ingress_knp` | Echo ingress via KNP |
| `echo_ingress_l7` | Echo ingress L7 |
| `echo_ingress_l7_named_port` | Echo ingress L7 named port |
| `echo_ingress_mutual_auth_spiffe` | Echo ingress SPIFFE mutual auth |
| `egress_gateway` | Egress gateway routing |
| `egress_gateway_excluded_cidrs` | Egress gateway with excluded CIDRs |
| `egress_gateway_multigateway` | Multiple egress gateways |
| `egress_gateway_with_l7_policy` | Egress gateway + L7 policy |
| `egress_to_specific_namespace` | Egress to specific namespace |
| `endpointslice_clustermesh_sync` | EndpointSlice sync across clusters |
| `from_cidr_host_netns` | From CIDR in host network namespace |
| `health` | Cilium health connectivity |
| `host_entity_egress` | Host entity egress |
| `host_entity_ingress` | Host entity ingress |
| `host_firewall_egress` | Host firewall egress rules |
| `host_firewall_ingress` | Host firewall ingress rules |
| `ingress_from_specific_ns` | Ingress from specific namespace |
| `ipsec_key_derivation` | IPsec key derivation |
| `l7_lb` | L7 load balancing |
| `local_redirect_policy` | Local redirect policy (LRP) |
| `local_redirect_policy_with_nodedns` | LRP with node-local DNS |
| `multicast` | Multicast traffic |
| `network_bandwidth_limit` | Network bandwidth limiting |
| `network_perf` | Network performance baseline |
| `network_qos` | Network QoS policies |
| `node_to_node_encryption` | Node-to-node encryption |
| `no_fragmentation` | No IP fragmentation |
| `no_interrupted_connections` | No connections interrupted during update |
| `no_ipsec_xfrm_errors` | No IPsec XFRM errors |
| `no_policies` | Baseline with no policies |
| `no_policies_extra` | No policies extra checks |
| `no_policies_from_outside` | No policies from outside |
| `no_unexpected_packet_drops` | No unexpected packet drops |
| `north_south_loadbalancing` | N/S load balancing |
| `north_south_loadbalancing_with_l7_policy` | N/S LB + L7 policy |
| `outside_to_ingress_service` | Outside-to-ingress service |
| `pod_to_controlplane_host` | Pod to control-plane host |
| `pod_to_controlplane_host_cidr` | Pod to control-plane host CIDR |
| `pod_to_ingress_service` | Pod to ingress service |
| `pod_to_k8s_on_controlplane` | Pod to K8s API on control-plane |
| `pod_to_k8s_on_controlplane_cidr` | Pod to K8s API CIDR |
| `pod_to_node_cidrpolicy` | Pod to node with CIDR policy |
| `pod_to_pod_encryption` | Pod-to-pod encryption |
| `pod_to_pod_encryption_v2` | Pod-to-pod encryption v2 |
| `policy_local_cluster` | Policy scoped to local cluster |
| `service_loopback` | Service loopback traffic |
| `strict_mode_encryption` | Strict-mode encryption |
| `to_cidr_external` | To CIDR external |
| `to_cidr_external_knp` | To CIDR external via KNP |
| `to_entities_world` | To world entity |
| `to_fqdns` | To FQDNs |
| `ztunnel_pod_to_pod_encryption` | Ambient/ztunnel pod-to-pod encryption |

**Run command**: `cilium connectivity test [--test <scenario>]`

---

## Category 5: Gateway API Conformance

Tests from `operator/pkg/gateway-api/conformance_test.go` and `vendor/sigs.k8s.io/gateway-api/conformance/`.

| Test Area | Description |
|-----------|-------------|
| BackendTLSPolicy | TLS to backends |
| GatewayClass reconciliation | GatewayClass CRUD |
| Gateway reconciliation | Gateway CRUD + status |
| HTTPRoute routing | Path/header/method matching |
| GRPCRoute routing | gRPC routing |
| TLSRoute routing | TLS passthrough |
| TCPRoute routing | TCP routing |
| ReferenceGrant | Cross-namespace references |
| BackendLBPolicy | Backend LB policies |
| GAMMA (mesh) | Service mesh routes |

**Run command**: `go test ./operator/pkg/gateway-api/... -run TestConformance`

---

## Category 6: Kubernetes sig-network Tests

Run by Cilium CI against a live cluster using the upstream Kubernetes e2e binary.

### [sig-network] Tests (broad)
- Pod networking (IP assignment, DNS)
- Service clusterIP, nodePort, LoadBalancer
- Endpoint connectivity
- Network namespaces
- Host networking

**ginkgo focus regex**: `\[sig-network\]`  
**Run command**: `e2e.test --ginkgo.focus='\[sig-network\]'`

### NetworkPolicy / Netpol Tests
- Basic allow/deny ingress
- Basic allow/deny egress
- Cross-namespace policies
- Port-based policies
- Label-based policies
- CIDR-based policies

**ginkgo focus regex**: `(Netpol|NetworkPolicy)`  
**Run command**: `e2e.test --ginkgo.focus='(Netpol|NetworkPolicy)'`

---

## Category 7: CNCF / Sonobuoy Conformance

Standard Kubernetes conformance tests run via sonobuoy.

| Suite | Coverage |
|-------|---------|
| `e2e` (certified conformance) | Core K8s APIs + networking |
| `cni-conformance` | CNI plugin correctness |
| `network-policy-conformance` | NetworkPolicy spec compliance |

**Run command**: `sonobuoy run --mode=certified-conformance`

---

## Category 8: MCS-API / ClusterMesh Conformance

From `pkg/clustermesh/mcsapi/conformance/conformance_test.go`.

| Test Area | Description |
|-----------|-------------|
| ServiceExport creation | Export a service across clusters |
| ServiceImport visibility | See imported services |
| Cross-cluster DNS | Resolve services across clusters |
| Endpoint sync | EndpointSlice replication |

**Run command**: `go test ./pkg/clustermesh/mcsapi/conformance/...`

---

## Coverage Gap Analysis

### Currently Wired in `seriousum`

| Suite | Status | Justfile recipe |
|-------|--------|----------------|
| K8sAgentFQDNTest (f02) | ✅ PASSING | `just run K8sAgentFQDNTest` |
| K8sNetworkPoliciesTest (f03/f06) | ✅ PASSING | `just run K8sNetworkPoliciesTest` |
| K8sAgentPolicyTest (f03-f06) | ✅ PASSING | `just run K8sAgentPolicyTest` |
| K8sDatapathServicesTest (f11-f17) | ⏳ VALIDATING | `just run K8sDatapathServicesTest` |

### Missing — Need to Wire

| Suite | Category | Priority | Effort |
|-------|----------|----------|--------|
| K8sAgentChaosTest (f01) | Ginkgo | P1 | 1h |
| K8sAgentPerNodeConfigTest (f02) | Ginkgo | P2 | 1h |
| K8sAgentHubbleTest (f10) | Ginkgo | P2 (needs Hubble) | 2h |
| K8sDatapathLRPTests (f18) | Ginkgo | P1 | 1h |
| K8sSpecificMACAddressTests (f19) | Ginkgo | P3 | 1h |
| K8sPolicyTestExtended (f06) | Ginkgo | P1 | 1h |
| RuntimeDatapathMonitorTest | Ginkgo (runtime) | P2 | 2h |
| TestControlPlane | Go test | P1 | 1h |
| cilium-cli connectivity (all 95) | cilium-cli | P1 | 4h |
| Gateway API conformance | Conformance | P3 (needs GW) | 4h |
| [sig-network] | Upstream K8s e2e | P2 | 3h |
| (Netpol\|NetworkPolicy) | Upstream K8s e2e | P1 | 2h |
| CNCF sonobuoy | Conformance | P2 | 2h |
| MCS-API conformance | Conformance | P3 (needs mesh) | 2h |

**Total missing suites**: 14  
**Estimated wiring effort**: ~27 hours

---

## Implementation Plan

### Phase 1: Wire Missing Ginkgo Suites (Week 1)

Add justfile recipes and CI matrix entries for all 19 focus groups:

```just
# Add to justfile
run-f01 focus="K8sAgentChaosTest":
    ./scripts/run-cilium-kind-test.sh --focus "{{focus}}" --timeout 30m

run-f18 focus="K8sDatapathLRPTests":
    ./scripts/run-cilium-kind-test.sh --focus "{{focus}}" --timeout 20m

run-all-ginkgo:
    just run-parallel-ginkgo f01 f02 f03 f04 f05 f06 f10 f11 f12 f13 f14 f15 f16 f17 f18 f19

run-parallel-ginkgo *focuses:
    ./scripts/run-parallel-focuses.sh {{focuses}}
```

### Phase 2: cilium-cli Connectivity (Week 1-2)

Wire all 95 cilium-cli scenarios into the test runner:

```bash
# scripts/run-cilium-cli-connectivity.sh
cilium connectivity test \
  --test no-policies \
  --test client-egress \
  --test echo-ingress \
  ... (all 95 scenarios)
```

```just
run-connectivity:
    ./scripts/run-cilium-cli-connectivity.sh

run-connectivity-test test="no-policies":
    cilium connectivity test --test {{test}}
```

### Phase 3: Upstream K8s sig-network (Week 2)

Download and run the upstream K8s e2e binary against the kind cluster:

```bash
# scripts/run-k8s-sig-network.sh
K8S_VERSION=1.33
curl -Lo e2e.test "https://dl.k8s.io/v${K8S_VERSION}.0/kubernetes-test-linux-amd64.tar.gz"
./e2e.test \
  --provider=local \
  --kubeconfig="$KUBECONFIG" \
  --ginkgo.focus='\[sig-network\]' \
  --ginkgo.skip='Alpha|Beta|Disruptive|Serial|...'
```

```just
run-sig-network:
    ./scripts/run-k8s-sig-network.sh

run-netpol-conformance:
    ./scripts/run-k8s-sig-network.sh --focus '(Netpol|NetworkPolicy)'
```

### Phase 4: CNCF Sonobuoy (Week 2-3)

```bash
# scripts/run-sonobuoy.sh
sonobuoy run \
  --mode=certified-conformance \
  --wait
sonobuoy results
```

```just
run-sonobuoy:
    ./scripts/run-sonobuoy.sh

run-cni-conformance:
    sonobuoy run --plugin cni-conformance --wait
```

### Phase 5: Parallel Execution (Week 3)

Group all suites by resource requirements, run in parallel on isolated clusters:

```
Cluster 1: f01, f02, f03, f04, f05, f06 (policy-focused)
Cluster 2: f11, f12, f13, f14, f15, f16, f17 (datapath-focused)
Cluster 3: f10, f18, f19, connectivity, sig-network (misc)
```

---

## New Justfile Recipes Needed

```just
# Run a specific CI focus group
run-focus focus timeout="45m":
    ./scripts/run-cilium-kind-test.sh --focus "{{focus}}" --timeout {{timeout}}

# Run all 19 ginkgo focus groups (sequentially)
run-all-ginkgo:
    for f in f01 f02 f03 f04 f05 f06 f10 f11 f12 f13 f14 f15 f16 f17 f18 f19; do \
        just run-focus $f; done

# Run all ginkgo groups in parallel across 3 clusters
run-all-ginkgo-parallel:
    ./scripts/run-parallel-focuses.sh

# Run cilium-cli connectivity suite
run-connectivity test="":
    ./scripts/run-cilium-cli-connectivity.sh {{test}}

# Run Kubernetes sig-network conformance
run-sig-network:
    ./scripts/run-k8s-sig-network.sh

# Run network policy conformance only  
run-netpol-conformance:
    ./scripts/run-k8s-sig-network.sh --focus 'Netpol|NetworkPolicy'

# Run CNCF sonobuoy
run-sonobuoy mode="certified-conformance":
    sonobuoy run --mode={{mode}} --wait && sonobuoy results

# Run every test suite (full CI equivalence)
run-full-ci:
    just run-all-ginkgo-parallel
    just run-connectivity
    just run-netpol-conformance
    just run-sonobuoy mode=quick

# Print master test catalog
list-tests:
    @cat docs/FULL_TEST_SUITE_CATALOG.md | grep '^###\|^| ' | head -200
```

---

## Summary Statistics

| Category | Suite Count | Individual Tests | Priority |
|----------|-------------|-----------------|----------|
| Cilium Ginkgo k8s | 19 focus groups | ~80 specs | P0-P1 |
| Cilium Ginkgo runtime | 1 suite | ~5 specs | P2 |
| Control-plane Go tests | 1 suite | ~10 specs | P1 |
| cilium-cli connectivity | 95 scenarios | 95 scenarios | P1 |
| Gateway API conformance | 1 suite | ~50 tests | P3 |
| K8s sig-network | 1 suite | ~200+ tests | P2 |
| K8s NetworkPolicy | 1 focus | ~40 tests | P1 |
| CNCF sonobuoy | 1 suite | ~300 tests | P2 |
| MCS-API conformance | 1 suite | ~20 tests | P3 |
| **TOTAL** | **~122 suites** | **800+ tests** | — |

---

**Document Version**: 1.0  
**Created**: 2026-05-11  
**GitHub Issue**: #59  
**Status**: Catalog complete — implementation in progress  
