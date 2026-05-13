#![allow(
    clippy::arc_with_non_send_sync,
    clippy::derivable_impls,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

//! Core types, traits, and utilities for seriousum.
//!
//! This crate provides the foundational abstractions used across all seriousum
//! components: error types, networking types, eBPF map abstractions, identity
//! models, and the controller/job system.

pub mod config;
pub mod controller;
pub mod ebpf;
pub mod error;
pub mod hive;
pub mod identity;
pub mod net;
pub mod time;

pub use controller::{Controller, ControllerConfig, ControllerStatus};
pub use error::{Error, Result};
pub use hive::{Hook, HookError, HookFn, Lifecycle, Promise};
pub use identity::{
    Identity, IdentityAllocator, IdentityMap, IdentityScope, IpIdentityPair, LABEL_SOURCE_CIDR,
    LABEL_SOURCE_FQDN, LABEL_SOURCE_K8S, LABEL_SOURCE_RESERVED, LABEL_SOURCE_UNSPEC, Label, Labels,
    NamedPort, NumericIdentity, NumericIdentitySlice, NumericIdentitySliceExt,
    RESERVED_IDENTITY_HEALTH, RESERVED_IDENTITY_HOST, RESERVED_IDENTITY_INGRESS,
    RESERVED_IDENTITY_INIT, RESERVED_IDENTITY_KUBE_APISERVER, RESERVED_IDENTITY_REMOTE_NODE,
    RESERVED_IDENTITY_UNKNOWN, RESERVED_IDENTITY_UNMANAGED, RESERVED_IDENTITY_WORLD,
    RESERVED_IDENTITY_WORLD_IPV4, RESERVED_IDENTITY_WORLD_IPV6, SecurityIdentity, SecurityLabel,
    SimpleIdentityAllocator, get_all_reserved_identities, get_cidr_labels, get_cluster_id_bits,
    get_cluster_id_shift, get_reserved_id, label_array_from_sorted_list, labels_from_model,
    labels_has_host_label, labels_has_ingress_label, labels_has_kube_apiserver_label,
    labels_has_remote_node_label, labels_is_reserved, lookup_reserved_identity_by_label_map,
    new_identity_from_label_array, scope_for_label_map,
};
pub use net::{IpAddr, IpNetwork, Ipv4Addr, Ipv6Addr, MacAddr, Port, Protocol};

// Re-export commonly used crates
pub use anyhow;
pub use bytes::Bytes;
pub use chrono;
pub use ipnet;
pub use tracing;
pub use uuid;

/// The seriousum version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default BPF map name prefix.
pub const BPF_MAP_PREFIX: &str = "cilium_";

/// Default endpoint prefix for BPF programs.
pub const ENDPOINT_PREFIX: &str = "lxc";

/// Maximum number of endpoints per node.
pub const MAX_ENDPOINTS: u32 = 1024;

/// Default MTU for overlay networks.
pub const DEFAULT_MTU: u16 = 1500;

/// Default BPF map page size.
pub const BPF_MAP_PAGE_SIZE: usize = 4096;

/// Cilium reserved identity range start.
pub const RESERVED_IDENTITY_START: u32 = 0;
pub const RESERVED_IDENTITY_END: u32 = 1023;

// Identity constants are defined in identity.rs; import from there.
// These aliases exist for legacy call-sites outside the module.
pub use identity::{
    IDENTITY_HEALTH, IDENTITY_HOST, IDENTITY_INIT, IDENTITY_INVALID, IDENTITY_SCOPE_GLOBAL,
    IDENTITY_SCOPE_LOCAL, IDENTITY_SCOPE_MASK, IDENTITY_SCOPE_REMOTE_NODE, IDENTITY_UNKNOWN,
    IDENTITY_UNMANAGED, IDENTITY_WORLD, IDENTITY_WORLD_IPV4, IDENTITY_WORLD_IPV6,
};

/// Default health check interval in seconds.
pub const HEALTH_CHECK_INTERVAL: u64 = 30;

/// Default garbage collection interval in seconds.
pub const GC_INTERVAL: u64 = 60;

/// Default policy revision increment.
pub const POLICY_REVISION_INCREMENT: u32 = 1;

/// BPF map maximum entries default.
pub const BPF_MAP_MAX_ENTRIES: u32 = 65536;

/// Default eBPF map name for endpoints.
pub const ENDPOINT_MAP_NAME: &str = "cilium_lxc";

/// Default eBPF map name for IP cache.
pub const IPCACHE_MAP_NAME: &str = "cilium_ipcache";

/// Default eBPF map name for policy.
pub const POLICY_MAP_NAME: &str = "cilium_policy";

/// Default eBPF map name for NAT (loadbalancer).
pub const NAT_MAP_NAME: &str = "cilium_lb4_map";

/// Default eBPF map name for connection tracking.
pub const CONNTRACK_MAP_NAME: &str = "cilium_ct6_global";

/// Default eBPF program type for TC (traffic control) hooks.
pub const BPF_PROG_TC: &str = "tc";

/// Default eBPF program type for XDP hooks.
pub const BPF_PROG_XDP: &str = "xdp";

/// Default eBPF program type for socket hooks.
pub const BPF_PROG_SOCK: &str = "sock_ops";

/// Default eBPF program type for kprobe hooks.
pub const BPF_PROG_KPROBE: &str = "kprobe";

/// Default eBPF program type for cgroup_skb hooks.
pub const BPF_PROG_CGROUP_SKB: &str = "cgroup_skb";

/// Default eBPF program type for cgroup_sock hooks.
pub const BPF_PROG_CGROUP_SOCK: &str = "cgroup_sock";

/// Default eBPF program type for sk_skb hooks.
pub const BPF_PROG_SK_SKB: &str = "sk_skb";

/// Default eBPF program type for sk_msg hooks.
pub const BPF_PROG_SK_MSG: &str = "sk_msg";

/// Default eBPF program type for lirc hooks.
pub const BPF_PROG_LIRC: &str = "lirc";

/// Default eBPF program type for flow_dissector hooks.
pub const BPF_PROG_FLOW_DISSECTOR: &str = "flow_dissector";

/// Default eBPF program type for sk_lookup hooks.
pub const BPF_PROG_SK_LOOKUP: &str = "sk_lookup";

/// Default eBPF program type for sock_ops hooks.
pub const BPF_PROG_SOCK_OPS: &str = "sock_ops";

/// Default eBPF program type for sock_addr hooks.
pub const BPF_PROG_SOCK_ADDR: &str = "sock_addr";

/// Default eBPF program type for sk_skb_verdict hooks.
pub const BPF_PROG_SK_SKB_VERDICT: &str = "sk_skb_verdict";

/// Default eBPF program type for sk_msg_verdict hooks.
pub const BPF_PROG_SK_MSG_VERDICT: &str = "sk_msg_verdict";

/// Default eBPF program type for sock_ops_verdict hooks.
pub const BPF_PROG_SOCK_OPS_VERDICT: &str = "sock_ops_verdict";

/// Default eBPF program type for sock_addr_verdict hooks.
pub const BPF_PROG_SOCK_ADDR_VERDICT: &str = "sock_addr_verdict";

/// Default eBPF program type for lirc_verdict hooks.
pub const BPF_PROG_LIRC_VERDICT: &str = "lirc_verdict";

/// Default eBPF program type for flow_dissector_verdict hooks.
pub const BPF_PROG_FLOW_DISSECTOR_VERDICT: &str = "flow_dissector_verdict";

/// Default eBPF program type for sk_lookup_verdict hooks.
pub const BPF_PROG_SK_LOOKUP_VERDICT: &str = "sk_lookup_verdict";
