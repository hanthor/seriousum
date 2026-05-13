# Cilium Parity TODOs

This document outlines the TODO items required for each crate in the `seriousum` repository to achieve full feature parity with upstream Cilium.

## `seriousum-api`
- [ ] Implement full REST API parity with Cilium v1 API definitions (`api/v1/openapi.yaml`).
- [ ] Add robust authentication and RBAC integration identical to upstream.
- [ ] Implement structured error handling and responses aligned with Cilium agent API.

## `seriousum-bgp`
- [ ] Implement full BGP control plane integration.
- [ ] Support GoBGP/FRR parity for route advertisement.

## `seriousum-cli` / `seriousum-dbg`
- [ ] Implement all `cilium-dbg` subcommands with identical flags and outputs.
- [ ] Implement `cilium-cli` parity for cluster installation, connectivity tests, and status checking.

## `seriousum-clustermesh`
- [ ] Implement multi-cluster service synchronization.
- [ ] Handle identity translation across clusters.

## `seriousum-cni`
- [ ] Achieve full parity with Cilium CNI plugin interface.
- [ ] Implement robust IPAM and network interface setup (veth pairs, routing).
- [ ] Ensure compatibility with all standard Kubernetes CNI execution environments.

## `seriousum-controller`
- [ ] Implement a generic controller loop with identical backoff and failure semantics as `pkg/controller`.

## `seriousum-crypto`
- [ ] Integrate TLS and WireGuard key management aligned with `pkg/crypto`.

## `seriousum-daemon`
- [ ] Port full agent orchestration (`daemon/`).
- [ ] Handle component lifecycle, initialization order, and clean shutdown.

## `seriousum-datapath`
- [ ] Implement robust eBPF program loader parity (`pkg/datapath/loader`).
- [ ] Ensure tc/XDP hooks are fully compatible with Cilium's native C datapath.

## `seriousum-ebpf`
- [ ] Fully implement all eBPF map abstractions (HashMap, LRU, Array, LPM Trie, RingBuffer).
- [ ] Ensure memory layouts match exactly with C eBPF map types.

## `seriousum-endpoint` / `seriousum-endpoints`
- [ ] Implement full endpoint lifecycle state machine.
- [ ] Port endpoint regeneration and policy realization logic.

## `seriousum-envoy`
- [ ] Implement Envoy xDS management server parity.
- [ ] Translate Cilium Network Policies into Envoy RBAC filters.

## `seriousum-fqdn`
- [ ] Implement DNS proxy for FQDN policy enforcement.
- [ ] Implement DNS cache and identity mapping for external domains.

## `seriousum-hubble`
- [ ] Implement Hubble flow exporter and observer server.
- [ ] Support `hubble-relay` for multi-node flow aggregation.

## `seriousum-identity`
- [ ] Implement security identity allocation and IPCache synchronization.
- [ ] Handle local and global identity allocation parity.

## `seriousum-ipam`
- [ ] Port CRD-backed, ENI, and Azure IPAM modes.
- [ ] Parity with node-local host IPAM allocation.

## `cilium-k8s`
- [ ] Implement comprehensive Kubernetes resource watchers.
- [ ] Sync endpoints, pods, nodes, and policies exactly like upstream.

## `seriousum-kvstore`
- [ ] Implement etcd client interface with the same retry/backoff logic.
- [ ] Support Consul/etcd failover as in upstream.

## `seriousum-loadbalancer`
- [ ] Implement Service Load Balancer parity.
- [ ] Support Maglev hashing and DSR (Direct Server Return) features.

## `seriousum-metrics`
- [ ] Expose identical Prometheus metric names and labels.

## `seriousum-monitor`
- [ ] Consume eBPF perf events and ring buffers precisely like upstream monitor.

## `seriousum-network`
- [ ] Implement egress gateway, Netlink routing, and IP rule management.

## `seriousum-node`
- [ ] Handle Node identity and addressing identical to upstream.

## `seriousum-operator`
- [ ] Implement all Kubernetes operator controllers.
- [ ] Port CRD cleanup, identity garbage collection, and IPAM management from `operator/pkg`.

## `seriousum-policy`
- [ ] Implement Network Policy engine with full L3/L4/L7 semantics.
- [ ] Support CNP (CiliumNetworkPolicy) and CCNP (CiliumClusterwideNetworkPolicy).

## `seriousum-proxy`
- [ ] L7 proxy integration logic parity.

## `seriousum-service-observer`
- [ ] Complete K8s service watcher.

## `seriousum-wireguard`
- [ ] Implement WireGuard + IPsec configuration management.
