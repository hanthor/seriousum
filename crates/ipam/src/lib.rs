//! IPAM (IP Address Management) subsystem for Cilium.
//!
//! This module provides IP address allocation and management for pods and other endpoints,
//! supporting both IPv4 and IPv6 addresses with bitmap-based allocation strategies.

use anyhow::anyhow;
use dashmap::DashMap;
use ipnet::IpNet;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
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

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type for IPAM operations.
pub type IpamResult<T> = std::result::Result<T, IpamError>;

/// IP address family (IPv4 or IPv6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// The pool from which the IP was allocated.
    pub pool_name: Pool,

    /// List of CIDRs to which this IP has direct access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cidrs: Option<Vec<IpNet>>,

    /// Primary MAC address of the interface (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_mac: Option<String>,

    /// Gateway IP for this allocation (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_ip: Option<IpAddr>,

    /// UUID of the expiration timer (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_uuid: Option<String>,

    /// Interface number (ENI mode only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_number: Option<String>,

    /// Whether to skip masquerade for this IP.
    #[serde(default)]
    pub skip_masquerade: bool,
}

impl AllocationResult {
    /// Creates a new allocation result with minimal information.
    pub fn new(ip: IpAddr, pool: Pool) -> Self {
        Self {
            ip,
            pool_name: pool,
            cidrs: None,
            primary_mac: None,
            gateway_ip: None,
            expiration_uuid: None,
            interface_number: None,
            skip_masquerade: false,
        }
    }

    /// Sets the CIDR list.
    pub fn with_cidrs(mut self, cidrs: Vec<IpNet>) -> Self {
        self.cidrs = Some(cidrs);
        self
    }

    /// Sets the gateway IP.
    pub fn with_gateway(mut self, gateway: IpAddr) -> Self {
        self.gateway_ip = Some(gateway);
        self
    }

    /// Sets the skip masquerade flag.
    pub fn with_skip_masquerade(mut self, skip: bool) -> Self {
        self.skip_masquerade = skip;
        self
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
                if host_bits < 128 { "2^" } else { "more than 2^63" }
            )));
        }

        let max_ips = 1u64 << host_bits;
        if max_ips > 1_000_000 {
            return Err(IpamError::InvalidCidr(format!(
                "network too large: {} would require {} entries",
                network, max_ips
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
                return Ok(self.offset_to_ip(offset)?);
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
            if is_allocated {
                if let Ok(ip) = self.offset_to_ip(offset) {
                    f(ip);
                }
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
            self.owners.insert((pool.clone(), addr.to_string()), owner.to_string());
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
        self.owners.insert((pool.clone(), addr.to_string()), owner.to_string());
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
            status.push_str(&format!(
                "Pool {}: {}/{} allocated, ",
                pool, count, capacity
            ));

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
        self.owners.insert((pool.clone(), ip.to_string()), owner.to_string());
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
            let key = if pool.as_str() != "default" {
                format!("{}/{}", pool, ip)
            } else {
                ip.clone()
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

        self.expiration_timers.insert(key.clone(), (uuid.clone(), tx));

        let pool_clone = pool.clone();
        let ipam_self = self.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(timeout) => {
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
        ipam.exclude_ip(ip, pool.clone(), "reserved".to_string()).await;
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
        assert_eq!(ipv4.unwrap().ip.is_ipv4(), true);
        assert!(ipv6.is_some());
        assert_eq!(ipv6.unwrap().ip.is_ipv6(), true);
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
        ipam.stop_expiration_timer(ip, pool.clone(), &uuid).await.unwrap();
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
        let result = ipam.allocate_next_family(Family::IPv6, "pod-1", Pool::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ipv6_only_ipam() {
        let ipam = Ipam::ipv6_only();
        let result = ipam.allocate_next_family(Family::IPv4, "pod-1", Pool::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_allocation_result_builder() {
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        let pool = Pool::default();
        let cidrs = vec!["10.0.0.0/24".parse().unwrap()];
        let gateway: IpAddr = "10.0.0.1".parse().unwrap();
        let result = AllocationResult::new(ip, pool)
            .with_cidrs(cidrs)
            .with_gateway(gateway)
            .with_skip_masquerade(true);
        assert_eq!(result.ip, ip);
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
}
