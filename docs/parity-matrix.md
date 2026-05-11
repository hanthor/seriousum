# Rust rewrite parity matrix

This is a lightweight parity manifest for the currently rewritten Rust crates. It is intentionally small and sequence-oriented: the runner script below executes the listed Rust crate tests in order and fails fast on the first error.

> Note: the original Go file paths below are the current parity anchors for the Rust rewrite. If `gomtests-map.md` is restored or expanded, refresh this manifest from that source.

| Original Go test file(s) | Rewritten Rust crate | Validate | Notes on gaps / future work |
| --- | --- | --- | --- |
| `pkg/option/config_test.go`, `pkg/identity/identity_test.go`, `pkg/ebpf/ebpf_test.go`, `pkg/datapath/linux/net_test.go` | `seriousum-core` | `cargo test -p seriousum-core` | Covers shared config defaults/round-trips, identity helpers, eBPF descriptors, and network primitives. Future work: split the broad crate into smaller parity slices once the rewrite gains more surface area. |
| `pkg/option/config_test.go` | `seriousum-config` | `cargo test -p seriousum-config` | Thin re-export crate; parity is currently limited to default config access. Future work: add merge/override behavior if the Rust config layer starts owning more policy. |
| `pkg/crypto/crypto_test.go`, `pkg/crypto/key_test.go` | `seriousum-crypto` | `cargo test -p seriousum-crypto` | Covers fingerprints and keypair helpers. Future work: add certificate, signing, and rotation parity when the Rust crypto layer grows beyond the current scaffolding. |
| `pkg/kvstore/kvstore_test.go` | `seriousum-kvstore` | `cargo test -p seriousum-kvstore` | Covers in-memory async set/get/delete semantics. Future work: add persistence, watch, and prefix-scoped behavior if the Rust store expands. |
| `daemon/cmd/daemon_test.go`, `daemon/cmd/config_test.go` | `seriousum-daemon` | `cargo test -p seriousum-daemon` | Covers CLI parsing and config fallback behavior. Future work: add lifecycle, signal handling, and subsystem wiring parity as the daemon becomes real. |
| `pkg/api/v1/api_test.go`, `pkg/api/v1/health_test.go` | `seriousum-api` | `cargo test -p seriousum-api` | Covers request/response envelopes and health/version metadata. Future work: add additional message schemas and compatibility checks as the shared API contract stabilizes. |
| `cilium-cli/cli/cmd_test.go`, `cilium-cli/cli/install_test.go`, `cilium-cli/clustermesh/clustermesh_test.go` | `seriousum-cli` | `cargo test -p seriousum-cli` | Covers CLI wiring, config checking, and synthesized operator reporting. Future work: add more subcommands and richer validation once the CLI grows beyond the scaffold. |
| `operator/cmd/root_test.go`, `operator/api/server_test.go`, `operator/api/health_test.go` | `seriousum-operator` | `cargo test -p seriousum-operator` | Covers operator startup/reporting scaffold and health metadata. Future work: add real HTTP endpoints and k8s-backed behavior when the operator service becomes live. |
| `hubble/cmd/cli_test.go`, `hubble/cmd/observe/flows_test.go` | `seriousum-hubble` | `cargo test -p seriousum-hubble` | Covers flow observation/reporting scaffolding and serialization. Future work: add CLI parsing and streaming adapters when Hubble ingestion is ported. |
| `clustermesh-apiserver/clustermesh/script_test.go`, `clustermesh-apiserver/clustermesh/users_mgmt_test.go`, `clustermesh-apiserver/syncstate/syncstate_test.go` | `seriousum-clustermesh` | `cargo test -p seriousum-clustermesh` | Covers sync/status reporting scaffolding. Future work: add peer discovery and cluster synchronization adapters once the runtime layer exists. |
| `pkg/auth/manager_test.go`, `pkg/auth/mutual_authhandler_test.go`, `pkg/auth/authmap_cache_test.go`, `pkg/auth/authmap_gc_test.go` | `seriousum-auth` | `cargo test -p seriousum-auth` | Covers auth session/config/report scaffolding. Future work: add cert rotation, mTLS validation, and auth-map GC parity when the runtime layer grows. |
| `pkg/proxy/proxy_test.go`, `pkg/proxy/proxyports/proxyports_test.go`, `pkg/proxy/routes_test.go` | `seriousum-proxy` | `cargo test -p seriousum-proxy` | Covers proxy session/config/report scaffolding. Future work: add redirect/routing integration and port allocation parity once the proxy layer is real. |
| `pkg/wireguard/agent/cell_test.go`, `pkg/wireguard/agent/agent_test.go` | `seriousum-wireguard` | `cargo test -p seriousum-wireguard` | Covers WireGuard state/report scaffolding. Future work: add peer reconciliation and cell wiring parity when the agent layer lands. |
| `plugins/cilium-cni/types/types_test.go`, `plugins/cilium-cni/chaining/api/api_test.go`, `plugins/cilium-cni/lib/deletion_queue_test.go` | `seriousum-cni` | `cargo test -p seriousum-cni` | Covers CNI config/session/report scaffolding. Future work: add chaining/deletion-queue behavior and real plugin wiring later. |
| `pkg/bgp/test/script_test.go`, `pkg/bgp/types/conversions_test.go` | `seriousum-bgp` | `cargo test -p seriousum-bgp` | Covers BGP route/neighbor/report scaffolding. Future work: add K8s-backed route writer and script parity later. |
| `pkg/fqdn/cache_test.go`, `pkg/fqdn/dnsproxy/helpers_test.go`, `pkg/fqdn/dns/dns_test.go` | `seriousum-fqdn` | `cargo test -p seriousum-fqdn` | Covers FQDN cache/report scaffolding. Future work: add TTL/zombie GC and selector matching parity later. |
| `pkg/envoy/standalone_envoy_test.go`, `pkg/ciliumenvoyconfig/script_test.go`, `pkg/ciliumenvoyconfig/cec_resource_parser_test.go` | `seriousum-envoy` | `cargo test -p seriousum-envoy` | Covers Envoy model/report scaffolding. Future work: add listener/cluster resource parsing and NACK handling later. |
| `pkg/k8s/utils/utils_test.go`, `pkg/k8s/client/testutils/script_test.go`, `pkg/k8s/tables/script_test.go` | `seriousum-k8s` | `cargo test -p seriousum-k8s` | Covers K8s status/report scaffolding. Future work: add fake client, table processing, and endpoint/service utilities later. |
| `pkg/datapath/connector/config_test.go`, `pkg/datapath/linux/config/config_test.go`, `pkg/datapath/linux/devices_controller_test.go` | `seriousum-datapath` | `cargo test -p seriousum-datapath` | Covers datapath model/report scaffolding. Future work: add real connector/config/device-controller logic later. |
| `pkg/bpf/map_linux_test.go`, `pkg/bpf/unused_maps_test.go`, `pkg/bpf/ops_linux_test.go` | `seriousum-ebpf` | `cargo test -p seriousum-ebpf` | Covers eBPF descriptor/report scaffolding. Future work: add real map lifecycle, pruning, and reconciler parity later. |
| `pkg/controller/controller_test.go` | `seriousum-controller` | `cargo test -p seriousum-controller` | Covers controller lifecycle/report scaffolding. Future work: add real add/remove/wait semantics and cancellation parity later. |

## Recommended runner order

1. `seriousum-core`
2. `seriousum-config`
3. `seriousum-crypto`
4. `seriousum-kvstore`
5. `seriousum-api`
6. `seriousum-daemon`
7. `seriousum-operator`
8. `seriousum-cli`
9. `seriousum-hubble`
10. `seriousum-clustermesh`
11. `seriousum-auth`
12. `seriousum-proxy`
13. `seriousum-wireguard`
14. `seriousum-cni`
15. `seriousum-bgp`
16. `seriousum-fqdn`
17. `seriousum-envoy`
18. `seriousum-k8s`
19. `seriousum-datapath`
20. `seriousum-ebpf`
21. `seriousum-controller`

That order keeps the shared foundation first and the runtime scaffold last.
