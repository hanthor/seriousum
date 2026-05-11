//! eBPF map and program abstractions for seriousum.

use std::fmt;

/// eBPF map types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum MapType {
    /// Hash map.
    Hash = 1,
    /// Array map.
    Array = 2,
    /// Hash map with per-CPU values.
    PerCpuHash = 6,
    /// Array map with per-CPU values.
    PerCpuArray = 9,
    /// LRU hash map.
    LruHash = 17,
    /// LRU hash map with per-CPU values.
    PerCpuLruHash = 29,
    /// Program array (for tail calls).
    ProgArray = 5,
    /// Dev map.
    DevMap = 11,
    /// Sock map.
    SockMap = 14,
    /// CGroup array.
    CGroupArray = 19,
    /// XSK map.
    XskMap = 22,
    /// Struct ops map.
    StructOps = 24,
    /// Ring buffer.
    RingBuf = 26,
}

impl MapType {
    /// Get the Linux BPF map type value.
    pub const fn as_u32(&self) -> u32 {
        *self as u32
    }
}

impl fmt::Display for MapType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hash => write!(f, "hash"),
            Self::Array => write!(f, "array"),
            Self::PerCpuHash => write!(f, "percpu_hash"),
            Self::PerCpuArray => write!(f, "percpu_array"),
            Self::LruHash => write!(f, "lru_hash"),
            Self::PerCpuLruHash => write!(f, "percpu_lru_hash"),
            Self::ProgArray => write!(f, "prog_array"),
            Self::DevMap => write!(f, "devmap"),
            Self::SockMap => write!(f, "sockmap"),
            Self::CGroupArray => write!(f, "cgroup_array"),
            Self::XskMap => write!(f, "xskmap"),
            Self::StructOps => write!(f, "struct_ops"),
            Self::RingBuf => write!(f, "ringbuf"),
        }
    }
}

/// eBPF program types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ProgType {
    /// Socket filter.
    SocketFilter = 1,
    /// kprobe.
    Kprobe = 2,
    /// Scheduler classifier.
    SchedAct = 3,
    /// XDP.
    Xdp = 5,
    /// cgroup_skb.
    CgroupSkb = 6,
    /// cgroup_sock.
    CgroupSock = 7,
    /// Socket ops.
    SocketOps = 8,
    /// Sk_skb.
    SkSkb = 9,
    /// Sk_msg.
    SkMsg = 10,
    /// Raw tracepoint.
    RawTracepoint = 12,
    /// Cgroup device.
    CgroupDevice = 13,
    /// Sk_lookup.
    SkLookup = 16,
    /// Syscall.
    Syscall = 17,
}

impl ProgType {
    /// Get the Linux BPF program type value.
    pub const fn as_u32(&self) -> u32 {
        *self as u32
    }
}

impl fmt::Display for ProgType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SocketFilter => write!(f, "socket_filter"),
            Self::Kprobe => write!(f, "kprobe"),
            Self::SchedAct => write!(f, "sched_act"),
            Self::Xdp => write!(f, "xdp"),
            Self::CgroupSkb => write!(f, "cgroup_skb"),
            Self::CgroupSock => write!(f, "cgroup_sock"),
            Self::SocketOps => write!(f, "socket_ops"),
            Self::SkSkb => write!(f, "sk_skb"),
            Self::SkMsg => write!(f, "sk_msg"),
            Self::RawTracepoint => write!(f, "raw_tracepoint"),
            Self::CgroupDevice => write!(f, "cgroup_device"),
            Self::SkLookup => write!(f, "sk_lookup"),
            Self::Syscall => write!(f, "syscall"),
        }
    }
}

/// eBPF attach types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum AttachType {
    /// cgroup/sock_create.
    CgroupSkbIngress = 0,
    /// cgroup/sock_create.
    CgroupSkbEgress = 1,
    /// cgroup/bind4, cgroup/bind6.
    CgroupSock = 2,
    /// cgroup/getsockopt.
    CgroupSockAddr = 3,
    /// cgroup/getsockopt.
    CgroupSkb = 4,
    /// sk_skb.
    SkSkb = 5,
    /// sk_msg.
    SkMsg = 6,
    /// XDP.
    Xdp = 7,
    /// tc ingress.
    Ingress = 8,
    /// tc egress.
    Egress = 9,
    /// Sk_lookup.
    SkLookup = 10,
}

impl AttachType {
    /// Get the Linux BPF attach type value.
    pub const fn as_u32(&self) -> u32 {
        *self as u32
    }
}

/// An eBPF map descriptor.
#[derive(Debug, Clone)]
pub struct MapDescriptor {
    /// Map name.
    pub name: String,
    /// Map type.
    pub map_type: MapType,
    /// Key size in bytes.
    pub key_size: u32,
    /// Value size in bytes.
    pub value_size: u32,
    /// Maximum number of entries.
    pub max_entries: u32,
    /// Map flags (e.g., BPF_F_NO_PREALLOC).
    pub flags: u32,
    /// Whether to pin this map in the BPF filesystem.
    pub pin: bool,
    /// Pin path in the BPF filesystem.
    pub pin_path: Option<String>,
}

impl MapDescriptor {
    /// Create a new map descriptor.
    pub fn new(
        name: impl Into<String>,
        map_type: MapType,
        key_size: u32,
        value_size: u32,
        max_entries: u32,
    ) -> Self {
        Self {
            name: name.into(),
            map_type,
            key_size,
            value_size,
            max_entries,
            flags: 0,
            pin: false,
            pin_path: None,
        }
    }

    /// Set the map flags.
    pub fn with_flags(mut self, flags: u32) -> Self {
        self.flags = flags;
        self
    }

    /// Pin this map in the BPF filesystem.
    pub fn with_pin(mut self, path: impl Into<String>) -> Self {
        self.pin = true;
        self.pin_path = Some(path.into());
        self
    }
}

/// An eBPF program descriptor.
#[derive(Debug, Clone)]
pub struct ProgDescriptor {
    /// Program name.
    pub name: String,
    /// Program type.
    pub prog_type: ProgType,
    /// Attach type.
    pub attach_type: Option<AttachType>,
    /// Attach to interface index (for TC/XDP).
    pub attach_ifindex: Option<u32>,
    /// Whether this program is loaded.
    pub loaded: bool,
    /// Whether this program is attached.
    pub attached: bool,
    /// Program FD (file descriptor).
    pub fd: Option<i32>,
}

impl ProgDescriptor {
    /// Create a new program descriptor.
    pub fn new(
        name: impl Into<String>,
        prog_type: ProgType,
        attach_type: Option<AttachType>,
    ) -> Self {
        Self {
            name: name.into(),
            prog_type,
            attach_type,
            attach_ifindex: None,
            loaded: false,
            attached: false,
            fd: None,
        }
    }

    /// Set the attach interface index.
    pub fn with_attach_ifindex(mut self, ifindex: u32) -> Self {
        self.attach_ifindex = Some(ifindex);
        self
    }
}

/// eBPF map flags.
pub mod map_flags {
    /// No pre-allocation of map entries.
    pub const NO_PREALLOC: u32 = 1 << 0;
    /// Clear on map lookup.
    pub const CLEAR_ON_LOOKUP: u32 = 1 << 1;
    /// No free on map delete.
    pub const NO_FREE_ON_DELETE: u32 = 1 << 2;
    /// Zero-seeded hash.
    pub const ZERO_SEEDED_HASH: u32 = 1 << 3;
}

/// eBPF program flags.
pub mod prog_flags {
    /// Freestable BPF program.
    pub const FREESTABLE: u32 = 1 << 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_type_display() {
        assert_eq!(MapType::Hash.to_string(), "hash");
        assert_eq!(MapType::Array.to_string(), "array");
        assert_eq!(MapType::PerCpuHash.to_string(), "percpu_hash");
    }

    #[test]
    fn test_prog_type_display() {
        assert_eq!(ProgType::Xdp.to_string(), "xdp");
        assert_eq!(ProgType::SchedAct.to_string(), "sched_act");
        assert_eq!(ProgType::CgroupSkb.to_string(), "cgroup_skb");
    }

    #[test]
    fn test_map_descriptor_new() {
        let desc = MapDescriptor::new("test_map", MapType::Hash, 4, 8, 1024);
        assert_eq!(desc.name, "test_map");
        assert_eq!(desc.map_type, MapType::Hash);
        assert_eq!(desc.key_size, 4);
        assert_eq!(desc.value_size, 8);
        assert_eq!(desc.max_entries, 1024);
        assert!(!desc.pin);
    }

    #[test]
    fn test_map_descriptor_with_flags() {
        let desc = MapDescriptor::new("test_map", MapType::Hash, 4, 8, 1024)
            .with_flags(map_flags::NO_PREALLOC);
        assert_eq!(desc.flags, map_flags::NO_PREALLOC);
    }

    #[test]
    fn test_map_descriptor_with_pin() {
        let desc = MapDescriptor::new("test_map", MapType::Hash, 4, 8, 1024)
            .with_pin("/sys/fs/bpf/test_map");
        assert!(desc.pin);
        assert_eq!(desc.pin_path, Some("/sys/fs/bpf/test_map".to_string()));
    }

    #[test]
    fn test_prog_descriptor_new() {
        let desc = ProgDescriptor::new("test_prog", ProgType::Xdp, Some(AttachType::Xdp));
        assert_eq!(desc.name, "test_prog");
        assert_eq!(desc.prog_type, ProgType::Xdp);
        assert_eq!(desc.attach_type, Some(AttachType::Xdp));
        assert!(!desc.loaded);
        assert!(!desc.attached);
    }

    #[test]
    fn test_prog_descriptor_with_attach_ifindex() {
        let desc = ProgDescriptor::new("test_prog", ProgType::Xdp, Some(AttachType::Xdp))
            .with_attach_ifindex(1);
        assert_eq!(desc.attach_ifindex, Some(1));
    }
}
