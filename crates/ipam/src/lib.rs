//! IPAM (IP Address Management) subsystem for Cilium.
//!
//! This module provides IP address allocation and management for pods and other endpoints,
//! supporting both IPv4 and IPv6 addresses with bitmap-based allocation strategies.

#![allow(clippy::unused_async, clippy::should_implement_trait)]

use anyhow::anyhow;
use dashmap::DashMap;
use ipnet::IpNet;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, RwLock as StdRwLock};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

/// Error type for IPAM operations.
#[derive(Debug, Error)]
pub enum IpamError {
    #[error("IPv4 allocation disabled")]
    Ipv4Disabled,

    #[error("IPv6 allocation disabled")]
    Ipv6Disabled,

    #[error("no IPAM pool provided for IP: {0}")]
    NoPoolProvided(String),

    #[error("invalid IP address: {0}")]
    InvalidIp(String),

    #[error("pool exhausted")]
    PoolExhausted,

    #[error("IP already allocated: {0}")]
    IpAlreadyAllocated(String),

    #[error("IP not found: {0}")]
    IpNotFound(String),

    #[error("expiration timer already registered")]
    TimerExists,

    #[error("no expiration timer registered")]
    NoTimer,

    #[error("UUID mismatch")]
    UuidMismatch,

    #[error("invalid CIDR: {0}")]
    InvalidCidr(String),

    #[error("allocator lock poisoned")]
    LockPoisoned,

    #[error("allocator not configured: {0}")]
    AllocatorUnavailable(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type for IPAM operations.
pub type IpamResult<T> = std::result::Result<T, IpamError>;

/// Result type for core allocator operations.
pub type Result<T> = IpamResult<T>;

/// Error type for the pure data-model allocators.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, thiserror::Error)]
pub enum IPAMError {
    #[error("IP already allocated: {0}")]
    AlreadyAllocated(String),
    #[error("IP not allocated: {0}")]
    NotAllocated(String),
    #[error("IP out of pool range: {0}")]
    OutOfRange(String),
    #[error("Pool exhausted")]
    Exhausted,
    #[error("Invalid CIDR: {0}")]
    InvalidCidr(String),
}

/// IP address family (IPv4 or IPv6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Family {
    IPv4,
    IPv6,
}

impl std::fmt::Display for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Family::IPv4 => write!(f, "ipv4"),
            Family::IPv6 => write!(f, "ipv6"),
        }
    }
}

use serde::{Deserialize, Serialize};

/// Address family alias for the pure IPAM allocator data model.
pub type AddressFamily = Family;

/// Supported IPAM backends.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IPAMMode {
    Kubernetes,
    /// ENI-backed AWS IPAM.
    AWS,
    Azure,
    GKE,
    AlibabaCloud,
    Delegated,
    HostScope,
    ClusterPoolV2,
}

/// Request to allocate an address for an owner.
#[derive(Debug, Clone)]
pub struct AllocationRequest {
    /// Requested address family.
    pub family: AddressFamily,
    /// Allocation owner, typically in pod namespace/name form.
    pub owner: String,
    /// Whether the allocation should be tracked with expiration state.
    pub expiration: bool,
}

/// Pool name for IP allocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pool(String);

impl Pool {
    /// Creates a new pool with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the default pool.
    pub fn default() -> Self {
        Self("default".to_string())
    }

    /// Returns the string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for Pool {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Pool {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Result of an IP allocation operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationResult {
    /// The allocated IP address.
    pub ip: IpAddr,

    /// Gateway IP for this allocation, when provided by the backing network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gw: Option<IpAddr>,

    /// The pod CIDR or subnet that produced this allocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cidr: Option<IpNet>,

    /// Generic interface identifier used by cloud-specific allocators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_index: Option<u32>,

    /// Backward-compatible alias for the interface index field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_number: Option<u32>,

    /// Backward-compatible alias for the gateway IP field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_ip: Option<IpAddr>,

    /// MAC address of the master interface for this allocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_mac: Option<[u8; 6]>,

    /// Interface name for this allocation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_name: Option<String>,

    /// The pool from which the IP was allocated.
    pub pool_name: Pool,

    /// UUID of the expiration timer (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_uuid: Option<String>,

    /// Whether to skip masquerade for this IP.
    #[serde(default)]
    pub skip_masquerade: bool,
}

impl AllocationResult {
    /// Creates a new allocation result with minimal information.
    pub fn new(ip: IpAddr, pool: Pool) -> Self {
        Self {
            ip,
            gw: None,
            cidr: None,
            interface_index: None,
            interface_number: None,
            gateway_ip: None,
            master_mac: None,
            interface_name: None,
            pool_name: pool,
            expiration_uuid: None,
            skip_masquerade: false,
        }
    }

    /// Sets the allocation CIDR.
    pub fn with_cidr(mut self, cidr: IpNet) -> Self {
        self.cidr = Some(cidr);
        self
    }

    /// Sets the first CIDR from a list of directly reachable CIDRs.
    pub fn with_cidrs(mut self, cidrs: &[IpNet]) -> Self {
        self.cidr = cidrs.first().copied();
        self
    }

    /// Sets the gateway IP.
    pub fn with_gateway(mut self, gateway: IpAddr) -> Self {
        self.gw = Some(gateway);
        self.gateway_ip = Some(gateway);
        self
    }

    /// Sets the skip masquerade flag.
    pub fn with_skip_masquerade(mut self, skip: bool) -> Self {
        self.skip_masquerade = skip;
        self
    }
}

/// Allocation map keyed by IP address.
pub type AllocationMap = HashMap<IpAddr, String>;

/// Allocates and releases IPs from a backing address range.
pub trait Allocator: Send + Sync {
    /// Allocates a specific IP for the provided owner.
    fn allocate(&self, ip: IpAddr, owner: &str) -> Result<AllocationResult>;

    /// Allocates the next available IP for the provided owner.
    fn allocate_next(&self, owner: &str) -> Result<AllocationResult>;

    /// Releases a previously allocated IP.
    fn release(&self, ip: IpAddr, owner: &str) -> Result<()>;

    /// Returns all current allocations and a one-line status string.
    fn dump(&self) -> (AllocationMap, String);

    /// Reports whether the allocator can serve requests.
    fn healthy(&self) -> bool;
}

fn ip_in_cidr(cidr: &IpNet, ip: IpAddr) -> bool {
    match (cidr, ip) {
        (IpNet::V4(net), IpAddr::V4(addr)) => net.contains(&addr),
        (IpNet::V6(net), IpAddr::V6(addr)) => net.contains(&addr),
        _ => false,
    }
}

fn ip_to_integer(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(addr) => u128::from(u32::from(addr)),
        IpAddr::V6(addr) => u128::from_be_bytes(addr.octets()),
    }
}

fn integer_to_ip(value: u128, is_ipv4: bool) -> IpAddr {
    if is_ipv4 {
        IpAddr::V4(Ipv4Addr::from(value as u32))
    } else {
        IpAddr::V6(Ipv6Addr::from(value))
    }
}

fn usable_ip_bounds(cidr: &IpNet) -> (u128, u128, bool) {
    match cidr {
        IpNet::V4(net) => {
            let network = u128::from(u32::from(net.network()));
            let host_bits = 32_u32.saturating_sub(u32::from(net.prefix_len()));
            let size_minus_one = if host_bits == 32 {
                u128::from(u32::MAX)
            } else {
                (1_u128 << host_bits) - 1
            };
            let mut start = network;
            let mut end = network + size_minus_one;
            if size_minus_one >= 2 {
                start += 1;
                end -= 1;
            }
            (start, end, true)
        }
        IpNet::V6(net) => {
            let network = u128::from_be_bytes(net.network().octets());
            let host_bits = 128_u32.saturating_sub(u32::from(net.prefix_len()));
            let size_minus_one = if host_bits == 128 {
                u128::MAX
            } else {
                (1_u128 << host_bits) - 1
            };
            let mut start = network;
            let mut end = network + size_minus_one;
            if size_minus_one >= 2 {
                start += 1;
                end -= 1;
            }
            (start, end, false)
        }
    }
}

fn ip_is_usable(cidr: &IpNet, ip: IpAddr) -> bool {
    if !ip_in_cidr(cidr, ip) {
        return false;
    }

    let (start, end, _) = usable_ip_bounds(cidr);
    let ip_value = ip_to_integer(ip);
    ip_value >= start && ip_value <= end
}

/// Allocates pod IPs directly from a single node CIDR.
#[derive(Debug, Clone)]
pub struct HostScopeAllocator {
    cidr: IpNet,
    allocated: Arc<StdRwLock<HashMap<IpAddr, String>>>,
}

impl HostScopeAllocator {
    /// Creates a new host-scoped allocator for the provided CIDR.
    pub fn new(cidr: IpNet) -> Self {
        Self {
            cidr,
            allocated: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }
}

impl Allocator for HostScopeAllocator {
    fn allocate(&self, ip: IpAddr, owner: &str) -> Result<AllocationResult> {
        if !ip_is_usable(&self.cidr, ip) {
            return Err(IpamError::InvalidIp(format!(
                "IP {ip} is outside allocatable range {}",
                self.cidr
            )));
        }

        let mut allocated = self
            .allocated
            .write()
            .map_err(|_| IpamError::LockPoisoned)?;
        if allocated.contains_key(&ip) {
            return Err(IpamError::IpAlreadyAllocated(ip.to_string()));
        }

        allocated.insert(ip, owner.to_string());
        debug!(ip = %ip, owner = %owner, cidr = %self.cidr, "allocated host-scoped IP");
        Ok(AllocationResult::new(ip, Pool::default()).with_cidr(self.cidr))
    }

    fn allocate_next(&self, owner: &str) -> Result<AllocationResult> {
        let (start, end, is_ipv4) = usable_ip_bounds(&self.cidr);
        let mut allocated = self
            .allocated
            .write()
            .map_err(|_| IpamError::LockPoisoned)?;

        for candidate in start..=end {
            let ip = integer_to_ip(candidate, is_ipv4);
            if allocated.contains_key(&ip) {
                continue;
            }

            allocated.insert(ip, owner.to_string());
            debug!(ip = %ip, owner = %owner, cidr = %self.cidr, "allocated next host-scoped IP");
            return Ok(AllocationResult::new(ip, Pool::default()).with_cidr(self.cidr));
        }

        Err(IpamError::PoolExhausted)
    }

    fn release(&self, ip: IpAddr, owner: &str) -> Result<()> {
        let mut allocated = self
            .allocated
            .write()
            .map_err(|_| IpamError::LockPoisoned)?;
        if allocated.remove(&ip).is_none() {
            return Err(IpamError::IpNotFound(ip.to_string()));
        }

        debug!(ip = %ip, owner = %owner, cidr = %self.cidr, "released host-scoped IP");
        Ok(())
    }

    fn dump(&self) -> (AllocationMap, String) {
        let allocations = self
            .allocated
            .read()
            .map(|entries| entries.clone())
            .unwrap_or_default();
        (allocations, self.cidr.to_string())
    }

    fn healthy(&self) -> bool {
        self.allocated.read().is_ok()
    }
}

/// Per-node IPAM state backed by a host-scoped allocator.
pub struct HostScopeNodeIPAM {
    /// Kubernetes node name associated with this IPAM state.
    pub node_name: String,
    /// Pod CIDR currently assigned to the node.
    pub pod_cidr: Option<IpNet>,
    /// Allocator serving addresses from the assigned pod CIDR.
    pub allocator: Option<Arc<dyn Allocator>>,
}

impl HostScopeNodeIPAM {
    /// Creates a new empty node IPAM state.
    pub fn new(node_name: impl Into<String>) -> Self {
        Self {
            node_name: node_name.into(),
            pod_cidr: None,
            allocator: None,
        }
    }

    /// Assigns a pod CIDR and recreates the backing host-scoped allocator.
    pub fn set_pod_cidr(&mut self, cidr: IpNet) -> Result<()> {
        self.pod_cidr = Some(cidr);
        self.allocator = Some(Arc::new(HostScopeAllocator::new(cidr)));
        debug!(node = %self.node_name, cidr = %cidr, "configured node pod CIDR");
        Ok(())
    }

    /// Allocates the next available IP for an owner from the node's pod CIDR.
    pub fn allocate_next(&self, owner: &str) -> Result<AllocationResult> {
        let allocator = self.allocator.as_ref().ok_or_else(|| {
            IpamError::AllocatorUnavailable(format!(
                "node {} has no pod CIDR allocator",
                self.node_name
            ))
        })?;

        allocator.allocate_next(owner)
    }
}

/// Allocates pod CIDRs from a fixed pool.
#[derive(Debug, Clone)]
pub struct CIDRPoolAllocator {
    pool: Vec<IpNet>,
    allocated: HashSet<IpNet>,
}

impl CIDRPoolAllocator {
    /// Creates a new CIDR allocator from the provided pool.
    pub fn new(pool: Vec<IpNet>) -> Self {
        Self {
            pool,
            allocated: HashSet::new(),
        }
    }

    /// Allocates the next available CIDR from the pool.
    pub fn allocate_next(&mut self) -> Option<IpNet> {
        let next = self
            .pool
            .iter()
            .find(|cidr| !self.allocated.contains(cidr))
            .copied()?;
        self.allocated.insert(next);
        Some(next)
    }

    /// Releases a CIDR back into the pool.
    pub fn release(&mut self, cidr: &IpNet) -> bool {
        self.allocated.remove(cidr)
    }

    /// Returns the number of CIDRs still available for allocation.
    pub fn available_count(&self) -> usize {
        self.pool
            .iter()
            .filter(|cidr| !self.allocated.contains(cidr))
            .count()
    }
}

/// A simple bit-vector IP pool allocator over a CIDR.
/// Mirrors cilium/pkg/ipam/allocator internals without kernel IO.
pub struct CIDRAllocator {
    network: IpNet,
    allocated: HashSet<IpAddr>,
    owner_map: HashMap<IpAddr, String>,
}

impl CIDRAllocator {
    /// Creates a new IP allocator for the provided CIDR.
    pub fn new(network: IpNet) -> Self {
        Self {
            network,
            allocated: Default::default(),
            owner_map: Default::default(),
        }
    }

    /// Allocates the next available IP from the pool.
    pub fn allocate_next(&mut self, owner: impl Into<String>) -> Option<IpAddr> {
        let owner = owner.into();
        for ip in self.network.hosts() {
            if !self.allocated.contains(&ip) {
                self.allocated.insert(ip);
                self.owner_map.insert(ip, owner.clone());
                return Some(ip);
            }
        }
        None
    }

    /// Allocates a specific IP from the pool.
    pub fn allocate(
        &mut self,
        ip: IpAddr,
        owner: impl Into<String>,
    ) -> std::result::Result<(), IPAMError> {
        if !self.network.contains(&ip) {
            return Err(IPAMError::OutOfRange(ip.to_string()));
        }
        if self.allocated.contains(&ip) {
            return Err(IPAMError::AlreadyAllocated(ip.to_string()));
        }
        self.allocated.insert(ip);
        self.owner_map.insert(ip, owner.into());
        Ok(())
    }

    /// Releases an IP back to the pool.
    pub fn release(&mut self, ip: &IpAddr) -> std::result::Result<(), IPAMError> {
        if !self.allocated.remove(ip) {
            return Err(IPAMError::NotAllocated(ip.to_string()));
        }
        self.owner_map.remove(ip);
        Ok(())
    }

    /// Returns whether a specific IP is currently allocated.
    pub fn is_allocated(&self, ip: &IpAddr) -> bool {
        self.allocated.contains(ip)
    }

    /// Returns the number of allocated IPs in the pool.
    pub fn count_allocated(&self) -> usize {
        self.allocated.len()
    }

    /// Returns the number of unallocated host IPs still available.
    pub fn count_available(&self) -> usize {
        self.network
            .hosts()
            .count()
            .saturating_sub(self.allocated.len())
    }

    /// Returns the owner of an allocated IP, if present.
    pub fn owner_of(&self, ip: &IpAddr) -> Option<&str> {
        self.owner_map.get(ip).map(String::as_str)
    }
}

/// Per-node IPAM state (pure data, mirrors cilium/pkg/ipam/node.go).
pub struct NodeIPAM {
    /// Kubernetes node name associated with this IPAM state.
    pub node_name: String,
    /// Named IP pools owned by this node.
    pub pools: HashMap<String, CIDRAllocator>,
    /// Pod CIDRs assigned to this node.
    pub pod_cidrs: Vec<IpNet>,
}

impl NodeIPAM {
    /// Creates a new node IPAM state container.
    pub fn new(node_name: impl Into<String>) -> Self {
        Self {
            node_name: node_name.into(),
            pools: Default::default(),
            pod_cidrs: vec![],
        }
    }

    /// Adds a named pool allocator for the supplied CIDR.
    pub fn add_pool(&mut self, name: impl Into<String>, cidr: IpNet) {
        self.pools.insert(name.into(), CIDRAllocator::new(cidr));
    }

    /// Returns the total remaining capacity across all pools.
    pub fn total_available(&self) -> usize {
        self.pools
            .values()
            .map(CIDRAllocator::count_available)
            .sum()
    }

    /// Returns the total number of allocated IPs across all pools.
    pub fn total_allocated(&self) -> usize {
        self.pools
            .values()
            .map(CIDRAllocator::count_allocated)
            .sum()
    }
}

/// Bitmap-based IP allocator using offset tracking.
#[derive(Debug, Clone)]
pub struct AllocationBitmap {
    /// Base network for this allocation range.
    network: IpNet,

    /// Bit mask: bit i is set if the i-th IP in the range is allocated.
    allocated: Arc<RwLock<Vec<bool>>>,

    /// Number of allocated IPs.
    count: Arc<RwLock<usize>>,
}

impl AllocationBitmap {
    /// Creates a new bitmap allocator for a CIDR range.
    pub fn new(network: IpNet) -> IpamResult<Self> {
        let host_bits = match network {
            IpNet::V4(net) => 32 - net.prefix_len(),
            IpNet::V6(net) => 128 - net.prefix_len(),
        };

        // Prevent overflow: cap at 64 bits for shift
        if host_bits >= 64 {
            return Err(IpamError::InvalidCidr(format!(
                "network too large: {} would require {} IPs (max 2^63)",
                network,
                if host_bits < 128 {
                    "2^"
                } else {
                    "more than 2^63"
                }
            )));
        }

        let max_ips = 1u64 << host_bits;
        if max_ips > 1_000_000 {
            return Err(IpamError::InvalidCidr(format!(
                "network too large: {network} would require {max_ips} entries"
            )));
        }

        let allocated = vec![false; max_ips as usize];

        Ok(Self {
            network,
            allocated: Arc::new(RwLock::new(allocated)),
            count: Arc::new(RwLock::new(0)),
        })
    }

    /// Allocates a specific IP if not already allocated.
    pub async fn allocate(&self, ip: IpAddr) -> IpamResult<bool> {
        // Check if IP is in this network
        let in_network = match (self.network, ip) {
            (IpNet::V4(net), IpAddr::V4(addr)) => net.contains(&addr),
            (IpNet::V6(net), IpAddr::V6(addr)) => net.contains(&addr),
            _ => false,
        };

        if !in_network {
            return Err(IpamError::InvalidIp(format!(
                "IP {} not in network {}",
                ip, self.network
            )));
        }

        let offset = self.ip_to_offset(ip)?;
        let mut allocated = self.allocated.write().await;

        if allocated[offset] {
            return Ok(false);
        }

        allocated[offset] = true;
        drop(allocated);

        let mut count = self.count.write().await;
        *count += 1;

        Ok(true)
    }

    /// Allocates the next available IP in the range.
    pub async fn allocate_next(&self) -> IpamResult<IpAddr> {
        let allocated = self.allocated.read().await;

        for (offset, &is_allocated) in allocated.iter().enumerate() {
            if !is_allocated {
                drop(allocated);
                let mut allocated_write = self.allocated.write().await;
                allocated_write[offset] = true;
                let mut count = self.count.write().await;
                *count += 1;
                return self.offset_to_ip(offset);
            }
        }

        Err(IpamError::PoolExhausted)
    }

    /// Releases a previously allocated IP.
    pub async fn release(&self, ip: IpAddr) -> IpamResult<bool> {
        // Check if IP is in this network
        let in_network = match (self.network, ip) {
            (IpNet::V4(net), IpAddr::V4(addr)) => net.contains(&addr),
            (IpNet::V6(net), IpAddr::V6(addr)) => net.contains(&addr),
            _ => false,
        };

        if !in_network {
            return Ok(false);
        }

        let offset = self.ip_to_offset(ip)?;
        let mut allocated = self.allocated.write().await;

        if !allocated[offset] {
            return Ok(false);
        }

        allocated[offset] = false;
        drop(allocated);

        let mut count = self.count.write().await;
        *count = count.saturating_sub(1);

        Ok(true)
    }

    /// Returns whether an IP is allocated.
    pub async fn has(&self, ip: IpAddr) -> IpamResult<bool> {
        // Check if IP is in this network
        let in_network = match (self.network, ip) {
            (IpNet::V4(net), IpAddr::V4(addr)) => net.contains(&addr),
            (IpNet::V6(net), IpAddr::V6(addr)) => net.contains(&addr),
            _ => false,
        };

        if !in_network {
            return Ok(false);
        }

        let offset = self.ip_to_offset(ip)?;
        let allocated = self.allocated.read().await;
        Ok(allocated[offset])
    }

    /// Iterates over all allocated IPs and calls the provided function.
    pub async fn for_each<F>(&self, mut f: F) -> IpamResult<()>
    where
        F: FnMut(IpAddr),
    {
        let allocated = self.allocated.read().await;
        for (offset, &is_allocated) in allocated.iter().enumerate() {
            if is_allocated && let Ok(ip) = self.offset_to_ip(offset) {
                f(ip);
            }
        }
        Ok(())
    }

    /// Returns the count of allocated IPs.
    pub async fn count(&self) -> usize {
        *self.count.read().await
    }

    /// Returns the total capacity of this allocator.
    pub async fn capacity(&self) -> usize {
        self.allocated.read().await.len()
    }

    /// Converts an IP address to its offset within the network.
    fn ip_to_offset(&self, ip: IpAddr) -> IpamResult<usize> {
        match (self.network, ip) {
            (IpNet::V4(net), IpAddr::V4(addr)) => {
                let network_int = u32::from_be_bytes(net.network().octets());
                let ip_int = u32::from_be_bytes(addr.octets());
                let offset = (ip_int - network_int) as usize;
                Ok(offset)
            }
            (IpNet::V6(net), IpAddr::V6(addr)) => {
                let network_int = u128::from_be_bytes(net.network().octets());
                let ip_int = u128::from_be_bytes(addr.octets());
                let offset = (ip_int - network_int) as usize;
                Ok(offset)
            }
            _ => Err(IpamError::InvalidIp("IP family mismatch".to_string())),
        }
    }

    /// Converts an offset back to an IP address.
    fn offset_to_ip(&self, offset: usize) -> IpamResult<IpAddr> {
        match self.network {
            IpNet::V4(net) => {
                let network_int = u32::from_be_bytes(net.network().octets());
                let ip_int = network_int.saturating_add(offset as u32);
                Ok(IpAddr::V4(std::net::Ipv4Addr::from_bits(ip_int)))
            }
            IpNet::V6(net) => {
                let network_int = u128::from_be_bytes(net.network().octets());
                let ip_int = network_int.saturating_add(offset as u128);
                Ok(IpAddr::V6(std::net::Ipv6Addr::from_bits(ip_int)))
            }
        }
    }
}

/// Bitmap-based allocator implementation.
#[derive(Clone)]
pub struct BitmapAllocator {
    /// Allocators per pool.
    pools: Arc<DashMap<Pool, AllocationBitmap>>,

    /// Owner tracking per pool.
    owners: Arc<DashMap<(Pool, String), String>>,
}

impl BitmapAllocator {
    /// Creates a new bitmap allocator for the given family.
    pub fn new(_family: Family) -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
            owners: Arc::new(DashMap::new()),
        }
    }

    /// Adds a pool to this allocator.
    pub async fn add_pool(&self, pool: Pool, cidr: IpNet) -> IpamResult<()> {
        let bitmap = AllocationBitmap::new(cidr)?;
        self.pools.insert(pool, bitmap);
        Ok(())
    }

    /// Allocates a specific IP or fails.
    pub async fn allocate(
        &self,
        addr: IpAddr,
        owner: &str,
        pool: &Pool,
    ) -> IpamResult<AllocationResult> {
        let bitmap = self
            .pools
            .get(pool)
            .ok_or_else(|| IpamError::NoPoolProvided(pool.to_string()))?
            .clone();

        if bitmap.allocate(addr).await? {
            self.owners
                .insert((pool.clone(), addr.to_string()), owner.to_string());
            debug!(ip = %addr, owner = %owner, pool = %pool, "allocated IP");
            Ok(AllocationResult::new(addr, pool.clone()))
        } else {
            Err(IpamError::IpAlreadyAllocated(addr.to_string()))
        }
    }

    /// Releases a previously allocated IP.
    pub async fn release(&self, addr: IpAddr, pool: &Pool) -> IpamResult<()> {
        let bitmap = self
            .pools
            .get(pool)
            .ok_or_else(|| IpamError::NoPoolProvided(pool.to_string()))?
            .clone();

        if bitmap.release(addr).await? {
            self.owners.remove(&(pool.clone(), addr.to_string()));
            debug!(ip = %addr, pool = %pool, "released IP");
        }
        Ok(())
    }

    /// Allocates the next available IP.
    pub async fn allocate_next(&self, owner: &str, pool: &Pool) -> IpamResult<AllocationResult> {
        let bitmap = self
            .pools
            .get(pool)
            .ok_or_else(|| IpamError::NoPoolProvided(pool.to_string()))?
            .clone();

        let addr = bitmap.allocate_next().await?;
        self.owners
            .insert((pool.clone(), addr.to_string()), owner.to_string());
        debug!(ip = %addr, owner = %owner, pool = %pool, "allocated next IP");
        Ok(AllocationResult::new(addr, pool.clone()))
    }

    /// Dumps all allocations and status.
    pub async fn dump(&self) -> IpamResult<(HashMap<Pool, HashMap<String, String>>, String)> {
        let mut result = HashMap::new();
        let mut status = String::new();

        for pool_ref in self.pools.iter() {
            let pool = pool_ref.key().clone();
            let bitmap = pool_ref.value().clone();
            drop(pool_ref);

            let mut pool_allocs = HashMap::new();
            bitmap
                .for_each(|ip| {
                    let owner = self
                        .owners
                        .get(&(pool.clone(), ip.to_string()))
                        .map(|o| o.value().clone())
                        .unwrap_or_default();
                    pool_allocs.insert(ip.to_string(), owner);
                })
                .await?;

            let capacity = bitmap.capacity().await;
            let count = bitmap.count().await;
            let _ = write!(status, "Pool {pool}: {count}/{capacity} allocated, ");

            result.insert(pool, pool_allocs);
        }

        Ok((result, status))
    }

    /// Returns the total capacity.
    pub async fn capacity(&self) -> IpamResult<u64> {
        let mut total = 0u64;
        for pool_ref in self.pools.iter() {
            let bitmap = pool_ref.value();
            total += bitmap.capacity().await as u64;
        }
        Ok(total)
    }
}

/// Main IPAM manager coordinating all allocation.
#[derive(Clone)]
pub struct Ipam {
    ipv4_allocator: Option<Arc<BitmapAllocator>>,
    ipv6_allocator: Option<Arc<BitmapAllocator>>,
    owners: Arc<DashMap<(Pool, String), String>>,
    excluded_ips: Arc<DashMap<(Pool, String), String>>,
    expiration_timers: Arc<DashMap<(Pool, String), (String, tokio::sync::oneshot::Sender<()>)>>,
}

impl Ipam {
    /// Creates a new IPAM manager with both IPv4 and IPv6 allocators.
    pub fn new() -> Self {
        Self {
            ipv4_allocator: Some(Arc::new(BitmapAllocator::new(Family::IPv4))),
            ipv6_allocator: Some(Arc::new(BitmapAllocator::new(Family::IPv6))),
            owners: Arc::new(DashMap::new()),
            excluded_ips: Arc::new(DashMap::new()),
            expiration_timers: Arc::new(DashMap::new()),
        }
    }

    /// Creates an IPAM manager with only IPv4 allocation.
    pub fn ipv4_only() -> Self {
        Self {
            ipv4_allocator: Some(Arc::new(BitmapAllocator::new(Family::IPv4))),
            ipv6_allocator: None,
            owners: Arc::new(DashMap::new()),
            excluded_ips: Arc::new(DashMap::new()),
            expiration_timers: Arc::new(DashMap::new()),
        }
    }

    /// Creates an IPAM manager with only IPv6 allocation.
    pub fn ipv6_only() -> Self {
        Self {
            ipv4_allocator: None,
            ipv6_allocator: Some(Arc::new(BitmapAllocator::new(Family::IPv6))),
            owners: Arc::new(DashMap::new()),
            excluded_ips: Arc::new(DashMap::new()),
            expiration_timers: Arc::new(DashMap::new()),
        }
    }

    /// Adds a pool to IPv4 allocator.
    pub async fn add_ipv4_pool(&self, pool: Pool, cidr: IpNet) -> IpamResult<()> {
        if let Some(allocator) = &self.ipv4_allocator {
            allocator.add_pool(pool, cidr).await
        } else {
            Err(IpamError::Ipv4Disabled)
        }
    }

    /// Adds a pool to IPv6 allocator.
    pub async fn add_ipv6_pool(&self, pool: Pool, cidr: IpNet) -> IpamResult<()> {
        if let Some(allocator) = &self.ipv6_allocator {
            allocator.add_pool(pool, cidr).await
        } else {
            Err(IpamError::Ipv6Disabled)
        }
    }

    /// Adds a pool to both IPv4 and IPv6 allocators.
    pub async fn add_pool(&self, pool: Pool, cidr_v4: IpNet, cidr_v6: IpNet) -> IpamResult<()> {
        if let Some(allocator) = &self.ipv4_allocator {
            allocator.add_pool(pool.clone(), cidr_v4).await?;
        }

        if let Some(allocator) = &self.ipv6_allocator {
            allocator.add_pool(pool, cidr_v6).await?;
        }

        Ok(())
    }

    /// Allocates a specific IP.
    pub async fn allocate_ip(
        &self,
        ip: IpAddr,
        owner: &str,
        pool: Pool,
    ) -> IpamResult<AllocationResult> {
        let family = if ip.is_ipv4() {
            Family::IPv4
        } else {
            Family::IPv6
        };

        let allocator = match family {
            Family::IPv4 => self
                .ipv4_allocator
                .as_ref()
                .ok_or(IpamError::Ipv4Disabled)?,
            Family::IPv6 => self
                .ipv6_allocator
                .as_ref()
                .ok_or(IpamError::Ipv6Disabled)?,
        };

        if let Some(reason) = self.excluded_ips.get(&(pool.clone(), ip.to_string())) {
            return Err(IpamError::Other(anyhow!(
                "IP {} is excluded: {}",
                ip,
                reason.value()
            )));
        }

        let result = allocator.allocate(ip, owner, &pool).await?;
        self.owners
            .insert((pool.clone(), ip.to_string()), owner.to_string());
        Ok(result)
    }

    /// Allocates the next available IP of the given family.
    pub async fn allocate_next_family(
        &self,
        family: Family,
        owner: &str,
        pool: Pool,
    ) -> IpamResult<AllocationResult> {
        let allocator = match family {
            Family::IPv4 => self
                .ipv4_allocator
                .as_ref()
                .ok_or(IpamError::Ipv4Disabled)?,
            Family::IPv6 => self
                .ipv6_allocator
                .as_ref()
                .ok_or(IpamError::Ipv6Disabled)?,
        };

        let result = allocator.allocate_next(owner, &pool).await?;
        self.owners
            .insert((pool.clone(), result.ip.to_string()), owner.to_string());
        Ok(result)
    }

    /// Allocates both IPv4 and IPv6 addresses (if available).
    pub async fn allocate_next(
        &self,
        owner: &str,
        pool: Pool,
    ) -> IpamResult<(Option<AllocationResult>, Option<AllocationResult>)> {
        let mut ipv4_result = None;
        let mut ipv6_result = None;

        if let Some(allocator) = &self.ipv4_allocator {
            match allocator.allocate_next(owner, &pool).await {
                Ok(result) => ipv4_result = Some(result),
                Err(e) => warn!("IPv4 allocation failed: {}", e),
            }
        }

        if let Some(allocator) = &self.ipv6_allocator {
            match allocator.allocate_next(owner, &pool).await {
                Ok(result) => ipv6_result = Some(result),
                Err(e) => {
                    warn!("IPv6 allocation failed: {}", e);
                    if let Some(ref result) = ipv4_result {
                        let _ = self.release_ip(result.ip, pool.clone()).await;
                    }
                    return Err(e);
                }
            }
        }

        Ok((ipv4_result, ipv6_result))
    }

    /// Releases a previously allocated IP.
    pub async fn release_ip(&self, ip: IpAddr, pool: Pool) -> IpamResult<()> {
        let family = if ip.is_ipv4() {
            Family::IPv4
        } else {
            Family::IPv6
        };

        let allocator = match family {
            Family::IPv4 => self
                .ipv4_allocator
                .as_ref()
                .ok_or(IpamError::Ipv4Disabled)?,
            Family::IPv6 => self
                .ipv6_allocator
                .as_ref()
                .ok_or(IpamError::Ipv6Disabled)?,
        };

        allocator.release(ip, &pool).await?;
        self.owners.remove(&(pool.clone(), ip.to_string()));

        if let Some((_, (_, tx))) = self.expiration_timers.remove(&(pool, ip.to_string())) {
            let _ = tx.send(());
        }

        Ok(())
    }

    /// Dumps all allocations.
    pub async fn dump(&self) -> IpamResult<(HashMap<String, String>, HashMap<String, String>)> {
        let mut ipv4_allocs = HashMap::new();
        let mut ipv6_allocs = HashMap::new();

        for owner_ref in self.owners.iter() {
            let (pool, ip) = owner_ref.key();
            let owner = owner_ref.value();
            let key = if pool.as_str() == "default" {
                ip.clone()
            } else {
                format!("{pool}/{ip}")
            };

            if ip.parse::<std::net::Ipv4Addr>().is_ok() {
                ipv4_allocs.insert(key, owner.clone());
            } else {
                ipv6_allocs.insert(key, owner.clone());
            }
        }

        Ok((ipv4_allocs, ipv6_allocs))
    }

    /// Adds an excluded IP.
    pub async fn exclude_ip(&self, ip: IpAddr, pool: Pool, reason: String) {
        self.excluded_ips.insert((pool, ip.to_string()), reason);
    }

    /// Starts an expiration timer for an allocated IP.
    pub async fn start_expiration_timer(
        &self,
        ip: IpAddr,
        pool: Pool,
        timeout: std::time::Duration,
    ) -> IpamResult<String> {
        let key = (pool.clone(), ip.to_string());

        if self.expiration_timers.contains_key(&key) {
            return Err(IpamError::TimerExists);
        }

        let uuid = Uuid::new_v4().to_string();
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();

        self.expiration_timers
            .insert(key.clone(), (uuid.clone(), tx));

        let pool_clone = pool.clone();
        let ipam_self = self.clone();

        tokio::spawn(async move {
            tokio::select! {
                () = tokio::time::sleep(timeout) => {
                    if let Err(e) = ipam_self.release_ip(ip, pool_clone).await {
                        warn!("failed to release IP after expiration: {}", e);
                    }
                }
                _ = &mut rx => {
                    // Timer was cancelled
                }
            }
        });

        Ok(uuid)
    }

    /// Stops an expiration timer.
    pub async fn stop_expiration_timer(
        &self,
        ip: IpAddr,
        pool: Pool,
        uuid: &str,
    ) -> IpamResult<()> {
        let key = (pool, ip.to_string());

        if let Some((_, (stored_uuid, tx))) = self.expiration_timers.remove(&key) {
            if stored_uuid != uuid {
                return Err(IpamError::UuidMismatch);
            }
            let _ = tx.send(());
            Ok(())
        } else {
            Err(IpamError::NoTimer)
        }
    }
}

impl Default for Ipam {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_creation() {
        let pool = Pool::new("test-pool");
        assert_eq!(pool.as_str(), "test-pool");
        let default_pool = Pool::default();
        assert_eq!(default_pool.as_str(), "default");
    }

    #[tokio::test]
    async fn test_bitmap_allocate() {
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        let bitmap = AllocationBitmap::new(cidr).unwrap();
        let ip1: IpAddr = "10.0.0.1".parse().unwrap();
        let result = bitmap.allocate(ip1).await.unwrap();
        assert!(result);
        let result2 = bitmap.allocate(ip1).await.unwrap();
        assert!(!result2);
    }

    #[tokio::test]
    async fn test_bitmap_allocate_out_of_range() {
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        let bitmap = AllocationBitmap::new(cidr).unwrap();
        let ip_out: IpAddr = "10.0.1.1".parse().unwrap();
        let result = bitmap.allocate(ip_out).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bitmap_allocate_next() {
        let cidr: IpNet = "10.0.0.0/30".parse().unwrap();
        let bitmap = AllocationBitmap::new(cidr).unwrap();
        let ip1 = bitmap.allocate_next().await.unwrap();
        assert_eq!(ip1.to_string(), "10.0.0.0");
        let ip2 = bitmap.allocate_next().await.unwrap();
        assert_eq!(ip2.to_string(), "10.0.0.1");
    }

    #[tokio::test]
    async fn test_bitmap_release() {
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        let bitmap = AllocationBitmap::new(cidr).unwrap();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        bitmap.allocate(ip).await.unwrap();
        assert!(bitmap.has(ip).await.unwrap());
        bitmap.release(ip).await.unwrap();
        assert!(!bitmap.has(ip).await.unwrap());
    }

    #[tokio::test]
    async fn test_ipam_allocate_ip() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let result = ipam.allocate_ip(ip, "pod-1", pool).await.unwrap();
        assert_eq!(result.ip, ip);
    }

    #[tokio::test]
    async fn test_ipam_allocate_next() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr_v4: IpNet = "10.0.0.0/24".parse().unwrap();
        let cidr_v6: IpNet = "fd00::/120".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr_v4).await.unwrap();
        ipam.add_ipv6_pool(pool.clone(), cidr_v6).await.unwrap();
        let (ipv4, ipv6) = ipam.allocate_next("pod-1", pool).await.unwrap();
        assert!(ipv4.is_some());
        assert!(ipv6.is_some());
    }

    #[tokio::test]
    async fn test_ipam_release() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        ipam.allocate_ip(ip, "pod-1", pool.clone()).await.unwrap();
        ipam.release_ip(ip, pool).await.unwrap();
    }

    #[tokio::test]
    async fn test_ipam_excluded_ip() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        ipam.exclude_ip(ip, pool.clone(), "reserved".to_string())
            .await;
        let result = ipam.allocate_ip(ip, "pod-1", pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ipam_dump() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();
        let ip1: IpAddr = "10.0.0.1".parse().unwrap();
        let ip2: IpAddr = "10.0.0.2".parse().unwrap();
        ipam.allocate_ip(ip1, "pod-1", pool.clone()).await.unwrap();
        ipam.allocate_ip(ip2, "pod-2", pool).await.unwrap();
        let (ipv4_allocs, _) = ipam.dump().await.unwrap();
        assert_eq!(ipv4_allocs.len(), 2);
    }

    #[tokio::test]
    async fn test_ipam_dual_stack() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr_v4: IpNet = "10.0.0.0/24".parse().unwrap();
        let cidr_v6: IpNet = "fd00::/120".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr_v4).await.unwrap();
        ipam.add_ipv6_pool(pool.clone(), cidr_v6).await.unwrap();
        let (ipv4, ipv6) = ipam.allocate_next("pod-1", pool.clone()).await.unwrap();
        assert!(ipv4.is_some());
        assert!(ipv4.unwrap().ip.is_ipv4());
        assert!(ipv6.is_some());
        assert!(ipv6.unwrap().ip.is_ipv6());
    }

    #[tokio::test]
    async fn test_expiration_timer() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        ipam.allocate_ip(ip, "pod-1", pool.clone()).await.unwrap();
        let _uuid = ipam
            .start_expiration_timer(ip, pool.clone(), std::time::Duration::from_millis(100))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let (allocs, _) = ipam.dump().await.unwrap();
        assert_eq!(allocs.len(), 0);
    }

    #[tokio::test]
    async fn test_stop_expiration_timer() {
        let ipam = Ipam::new();
        let pool = Pool::default();
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool.clone(), cidr).await.unwrap();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        ipam.allocate_ip(ip, "pod-1", pool.clone()).await.unwrap();
        let uuid = ipam
            .start_expiration_timer(ip, pool.clone(), std::time::Duration::from_secs(10))
            .await
            .unwrap();
        ipam.stop_expiration_timer(ip, pool.clone(), &uuid)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (allocs, _) = ipam.dump().await.unwrap();
        assert_eq!(allocs.len(), 1);
    }

    #[tokio::test]
    async fn test_bitmap_count() {
        let cidr: IpNet = "10.0.0.0/24".parse().unwrap();
        let bitmap = AllocationBitmap::new(cidr).unwrap();
        let ip1: IpAddr = "10.0.0.1".parse().unwrap();
        let ip2: IpAddr = "10.0.0.2".parse().unwrap();
        bitmap.allocate(ip1).await.unwrap();
        assert_eq!(bitmap.count().await, 1);
        bitmap.allocate(ip2).await.unwrap();
        assert_eq!(bitmap.count().await, 2);
        bitmap.release(ip1).await.unwrap();
        assert_eq!(bitmap.count().await, 1);
    }

    #[tokio::test]
    async fn test_ipv4_only_ipam() {
        let ipam = Ipam::ipv4_only();
        let result = ipam
            .allocate_next_family(Family::IPv6, "pod-1", Pool::default())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ipv6_only_ipam() {
        let ipam = Ipam::ipv6_only();
        let result = ipam
            .allocate_next_family(Family::IPv4, "pod-1", Pool::default())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_allocation_result_builder() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let pool = Pool::default();
        let cidrs = vec!["10.0.0.0/24".parse().unwrap()];
        let gateway: IpAddr = "10.0.0.1".parse().unwrap();
        let result = AllocationResult::new(ip, pool)
            .with_cidrs(&cidrs)
            .with_gateway(gateway)
            .with_skip_masquerade(true);
        assert_eq!(result.ip, ip);
        assert_eq!(result.gw, Some(gateway));
        assert_eq!(result.gateway_ip, Some(gateway));
        assert!(result.skip_masquerade);
    }

    #[tokio::test]
    async fn test_multiple_pools() {
        let ipam = Ipam::new();
        let pool1 = Pool::new("pool-1");
        let pool2 = Pool::new("pool-2");
        let cidr1: IpNet = "10.0.0.0/24".parse().unwrap();
        let cidr2: IpNet = "10.1.0.0/24".parse().unwrap();
        ipam.add_ipv4_pool(pool1.clone(), cidr1).await.unwrap();
        ipam.add_ipv4_pool(pool2.clone(), cidr2).await.unwrap();
        let ip1: IpAddr = "10.0.0.1".parse().unwrap();
        let ip2: IpAddr = "10.1.0.1".parse().unwrap();
        ipam.allocate_ip(ip1, "pod-1", pool1).await.unwrap();
        ipam.allocate_ip(ip2, "pod-2", pool2).await.unwrap();
        let (allocs, _) = ipam.dump().await.unwrap();
        assert_eq!(allocs.len(), 2);
    }

    #[test]
    fn test_host_scope_allocate_and_release() {
        let alloc = HostScopeAllocator::new("10.0.0.0/24".parse().unwrap());
        let r1 = alloc.allocate_next("pod-1").unwrap();
        assert!(r1.ip.to_string().starts_with("10.0.0."));
        assert_ne!(r1.ip.to_string(), "10.0.0.0");
        assert_eq!(r1.cidr, Some("10.0.0.0/24".parse().unwrap()));
        let r2 = alloc.allocate_next("pod-2").unwrap();
        assert_ne!(r1.ip, r2.ip);
        alloc.release(r1.ip, "pod-1").unwrap();
        let r3 = alloc.allocate_next("pod-3").unwrap();
        assert!(r3.ip == r1.ip || r3.ip != r2.ip);
        let (dump, status) = alloc.dump();
        assert_eq!(status, "10.0.0.0/24");
        assert_eq!(dump.get(&r2.ip), Some(&"pod-2".to_string()));
    }

    #[test]
    fn test_host_scope_allocate_specific_ip() {
        let alloc = HostScopeAllocator::new("10.0.0.0/30".parse().unwrap());
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let result = alloc.allocate(ip, "pod-1").unwrap();
        assert_eq!(result.ip, ip);
        assert_eq!(result.cidr, Some("10.0.0.0/30".parse().unwrap()));
        assert_eq!(alloc.dump().0.get(&ip), Some(&"pod-1".to_string()));
        assert!(matches!(
            alloc.allocate(ip, "pod-2"),
            Err(IpamError::IpAlreadyAllocated(_))
        ));
        assert!(
            alloc
                .allocate("10.0.0.0".parse().unwrap(), "network")
                .is_err()
        );
        assert!(
            alloc
                .release("10.0.0.2".parse().unwrap(), "missing")
                .is_err()
        );
    }

    #[test]
    fn test_host_scope_node_ipam_allocate_next() {
        let mut node_ipam = HostScopeNodeIPAM::new("worker-1");
        assert!(node_ipam.allocate_next("pod-1").is_err());
        node_ipam
            .set_pod_cidr("10.2.0.0/30".parse().unwrap())
            .unwrap();
        let result = node_ipam.allocate_next("pod-1").unwrap();
        assert_eq!(result.ip.to_string(), "10.2.0.1");
        assert_eq!(node_ipam.pod_cidr, Some("10.2.0.0/30".parse().unwrap()));
    }

    #[test]
    fn test_cidr_pool_allocator_allocate_and_release() {
        let mut alloc = CIDRPoolAllocator::new(vec![
            "10.0.0.0/24".parse().unwrap(),
            "10.0.1.0/24".parse().unwrap(),
        ]);
        assert_eq!(alloc.available_count(), 2);
        let c1 = alloc.allocate_next().unwrap();
        let c2 = alloc.allocate_next().unwrap();
        assert_ne!(c1, c2);
        assert_eq!(alloc.available_count(), 0);
        assert!(alloc.allocate_next().is_none());
        assert!(alloc.release(&c1));
        assert_eq!(alloc.available_count(), 1);
        assert!(alloc.allocate_next().is_some());
    }

    #[test]
    fn test_cidr_allocator_allocate_next() {
        let net: IpNet = "10.0.0.0/30".parse().unwrap();
        let mut alloc = CIDRAllocator::new(net);
        let ip1 = alloc.allocate_next("pod-a").unwrap();
        let ip2 = alloc.allocate_next("pod-b").unwrap();
        assert_ne!(ip1, ip2);
        assert!(alloc.allocate_next("pod-c").is_none());
    }

    #[test]
    fn test_cidr_allocator_allocate_specific() {
        let net: IpNet = "10.0.0.0/24".parse().unwrap();
        let mut alloc = CIDRAllocator::new(net);
        let ip: IpAddr = "10.0.0.5".parse().unwrap();
        alloc.allocate(ip, "owner-1").unwrap();
        assert!(alloc.is_allocated(&ip));
        assert_eq!(alloc.owner_of(&ip), Some("owner-1"));
        assert!(alloc.allocate(ip, "owner-2").is_err());
    }

    #[test]
    fn test_cidr_allocator_release() {
        let net: IpNet = "10.0.0.0/29".parse().unwrap();
        let mut alloc = CIDRAllocator::new(net);
        let ip = alloc.allocate_next("pod").unwrap();
        assert_eq!(alloc.count_allocated(), 1);
        alloc.release(&ip).unwrap();
        assert_eq!(alloc.count_allocated(), 0);
        assert!(alloc.release(&ip).is_err());
    }

    #[test]
    fn test_node_ipam_pools() {
        let mut node = NodeIPAM::new("node-1");
        node.add_pool("default", "192.168.1.0/24".parse().unwrap());
        node.add_pool("secondary", "172.16.0.0/24".parse().unwrap());
        assert!(node.total_available() > 0);
        assert_eq!(node.total_allocated(), 0);
    }

    #[test]
    fn test_ipam_out_of_range() {
        let net: IpNet = "10.0.0.0/24".parse().unwrap();
        let mut alloc = CIDRAllocator::new(net);
        let bad_ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(alloc.allocate(bad_ip, "x").is_err());
    }
}
