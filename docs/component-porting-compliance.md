# Component porting compliance report

Living report for the major protected components/crates in the Rust port. Update the status/score as unit tests, drop-in smoke checks, and kind/image harnesses mature.

## Status legend

- `5/5` — end-to-end parity validated
- `4/5` — harness-backed and smoke-tested
- `3/5` — crate complete with drop-in smoke coverage
- `2/5` — crate scaffolded with unit tests
- `1/5` — placeholder or no dedicated coverage yet

| Component / crate | Cilium parity anchors | Current Rust artifact status | Relevant integration suites | Score |
| --- | --- | --- | --- | --- |
| `seriousum-core` | `pkg/option/config_test.go`, `pkg/identity/identity_test.go`, `pkg/ebpf/ebpf_test.go`, `pkg/datapath/linux/net_test.go` | Shared foundation crate; unit tests cover config, identity, eBPF descriptors, and network primitives. | Workspace unit tests; foundation for all other porting checks. | `2/5` |
| `seriousum-config` | `pkg/option/config_test.go` | Thin config crate; current Rust surface is the default/merge access layer. | Workspace unit tests. | `2/5` |
| `seriousum-crypto` | `pkg/crypto/crypto_test.go`, `pkg/crypto/key_test.go` | Key/fingerprint helpers are present and unit-tested. | Workspace unit tests. | `2/5` |
| `seriousum-kvstore` | `pkg/kvstore/kvstore_test.go` | In-memory async set/get/delete behavior is implemented. | Workspace unit tests. | `2/5` |
| `seriousum-api` | `pkg/api/v1/api_test.go`, `pkg/api/v1/health_test.go` | Request/response envelopes and health/version metadata are present. | Workspace unit tests. | `2/5` |
| `seriousum-daemon` | `daemon/cmd/daemon_test.go`, `daemon/cmd/config_test.go` | Drop-in daemon binary with config parsing/fallback behavior. | `cilium --config ...`, `cilium-dbg --config ...`, drop-in smoke checks. | `3/5` |
| `seriousum-cli` | `cilium-cli/cli/cmd_test.go`, `cilium-cli/cli/install_test.go`, `cilium-cli/clustermesh/clustermesh_test.go` | Drop-in CLI binary with config-check and command wiring coverage. | `cilium-cli version`, `cilium-cli config check --path ...`. | `3/5` |
| `seriousum-operator` | `operator/cmd/root_test.go`, `operator/api/server_test.go`, `operator/api/health_test.go` | Drop-in operator binary with startup/reporting scaffold. | `operator --summary ...`; image-harness passes for operator health and metrics. | `3/5` |
| `seriousum-hubble` | `hubble/cmd/cli_test.go`, `hubble/cmd/observe/flows_test.go` | Drop-in Hubble binary with flow reporting/serialization scaffold. | `hubble`; image-harness Hubble pass. | `3/5` |
| `seriousum-clustermesh` | `clustermesh-apiserver/clustermesh/script_test.go`, `clustermesh-apiserver/clustermesh/users_mgmt_test.go`, `clustermesh-apiserver/syncstate/syncstate_test.go` | Drop-in clustermesh-apiserver binary with sync/status scaffolding. | `clustermesh-apiserver`; image-harness clustermesh pass. | `3/5` |
| `seriousum-auth` | `pkg/auth/manager_test.go`, `pkg/auth/mutual_authhandler_test.go`, `pkg/auth/authmap_cache_test.go`, `pkg/auth/authmap_gc_test.go` | Auth session/config/report scaffolding is present. | Workspace unit tests; future mTLS/auth-policy harness. | `2/5` |
| `seriousum-proxy` | `pkg/proxy/proxy_test.go`, `pkg/proxy/proxyports/proxyports_test.go`, `pkg/proxy/routes_test.go` | Proxy session/config/report scaffolding is present. | Workspace unit tests; future control-plane and service-routing passes. | `2/5` |
| `seriousum-wireguard` | `pkg/wireguard/agent/cell_test.go`, `pkg/wireguard/agent/agent_test.go` | WireGuard state/report scaffolding is present. | Workspace unit tests; future control-plane pass. | `2/5` |
| `seriousum-cni` | `plugins/cilium-cni/types/types_test.go`, `plugins/cilium-cni/chaining/api/api_test.go`, `plugins/cilium-cni/lib/deletion_queue_test.go` | CNI config/session/report scaffolding is present. | Workspace unit tests; future install / k8s kind passes. | `2/5` |
| `seriousum-bgp` | `pkg/bgp/test/script_test.go`, `pkg/bgp/types/conversions_test.go` | BGP route/neighbor/report scaffolding is present. | Workspace unit tests; future BGP-specific harness. | `2/5` |
| `seriousum-fqdn` | `pkg/fqdn/cache_test.go`, `pkg/fqdn/dnsproxy/helpers_test.go`, `pkg/fqdn/dns/dns_test.go` | FQDN cache/report scaffolding is present. | Image-harness FQDN pass; workspace unit tests. | `2/5` |
| `seriousum-envoy` | `pkg/envoy/standalone_envoy_test.go`, `pkg/ciliumenvoyconfig/script_test.go`, `pkg/ciliumenvoyconfig/cec_resource_parser_test.go` | Envoy model/report scaffolding is present. | Workspace unit tests; future control-plane / k8s-service passes. | `2/5` |
| `seriousum-k8s` | `pkg/k8s/utils/utils_test.go`, `pkg/k8s/client/testutils/script_test.go`, `pkg/k8s/tables/script_test.go` | K8s status/report scaffolding is present. | `k8s services` image-harness pass; workspace unit tests. | `2/5` |
| `seriousum-datapath` | `pkg/datapath/connector/config_test.go`, `pkg/datapath/linux/config/config_test.go`, `pkg/datapath/linux/devices_controller_test.go` | Datapath model/report scaffolding is present. | `controlplane` image-harness pass; workspace unit tests. | `2/5` |
| `seriousum-ebpf` | `pkg/bpf/map_linux_test.go`, `pkg/bpf/unused_maps_test.go`, `pkg/bpf/ops_linux_test.go` | eBPF descriptor/report scaffolding is present. | `controlplane` / `k8s services` image-harness passes; workspace unit tests. | `2/5` |
| `seriousum-controller` | `pkg/controller/controller_test.go` | Controller lifecycle/report scaffolding is present. | Workspace unit tests. | `2/5` |

## Maintenance notes

- Keep this report aligned with `docs/parity-matrix.md` when new Rust crates or Cilium anchors are added.
- Promote a row from `2/5` to `3/5` only after the binary or crate has a repeatable smoke check.
- Promote to `4/5` or `5/5` only after the relevant harness is automated and consistently green.
- The most useful harness entry points today are documented in `docs/cilium-dropin-testing.md` and `docs/cilium-integration-images.md`.
