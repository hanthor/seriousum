//! Lightweight IPAM scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, IpNetwork, Result};
use std::net::IpAddr;

/// Default component name for IPAM scaffolds.
pub const COMPONENT: &str = "seriousum-ipam";

/// IP allocation state for the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStatus {
    /// The pool has room for more allocations.
    Available,
    /// The pool is currently exhausted.
    Exhausted,
}

/// Compact IPAM pool model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolModel {
    /// CIDR represented by the pool.
    pub cidr: IpNetwork,

    /// Optional gateway for the pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<IpAddr>,

    /// Allocated addresses.
    pub allocated: Vec<IpAddr>,

    /// Soft capacity used by the scaffold.
    pub capacity: u32,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl PoolModel {
    /// Creates a new pool model.
    #[must_use]
    pub fn new(cidr: IpNetwork) -> Self {
        Self {
            cidr,
            gateway: None,
            allocated: Vec::new(),
            capacity: 16,
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        let mut pool = Self::new("10.0.1.0/24".parse().expect("valid scaffold pool"))
            .with_gateway("10.0.1.1".parse().expect("valid scaffold gateway"));
        pool.allocate("10.0.1.2".parse().expect("valid scaffold allocation"))
            .expect("scaffold allocation is valid");
        pool
    }

    /// Adds a gateway to the pool.
    #[must_use]
    pub fn with_gateway(mut self, gateway: IpAddr) -> Self {
        self.gateway = Some(gateway);
        self
    }

    /// Adds an allocation, returning an error on invalid input.
    pub fn allocate(&mut self, address: IpAddr) -> Result<&mut Self> {
        if !self.cidr.contains(&address) {
            return Err(Error::Ipam(format!(
                "address {address} is outside {}",
                self.cidr
            )));
        }

        if self.allocated.contains(&address) {
            return Err(Error::Ipam(format!(
                "address {address} is already allocated"
            )));
        }

        self.allocated.push(address);
        Ok(self)
    }

    /// Returns the number of remaining allocations.
    #[must_use]
    pub fn available_slots(&self) -> u32 {
        self.capacity.saturating_sub(self.allocated.len() as u32)
    }

    /// Returns the current pool status.
    #[must_use]
    pub fn status(&self) -> PoolStatus {
        if self.available_slots() == 0 {
            PoolStatus::Exhausted
        } else {
            PoolStatus::Available
        }
    }

    /// Returns a concise human-readable summary.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} allocated={} capacity={}",
            self.cidr,
            self.allocated.len(),
            self.capacity
        )
    }

    /// Validates the pool model.
    pub fn validate(&self) -> Result<()> {
        if self.allocated.len() as u32 > self.capacity {
            return Err(Error::Ipam(String::from(
                "allocated addresses exceed capacity",
            )));
        }

        Ok(())
    }
}

impl Default for PoolModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable IPAM report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpamReport {
    /// Component name.
    pub component: String,

    /// IPAM pool model.
    pub pool: PoolModel,

    /// High-level status for the pool.
    pub status: PoolStatus,
}

impl IpamReport {
    /// Builds a report from a pool model.
    #[must_use]
    pub fn new(pool: PoolModel) -> Self {
        let status = pool.status();
        Self {
            component: COMPONENT.to_owned(),
            pool,
            status,
        }
    }
}

/// Returns the standard IPAM scaffold report.
#[must_use]
pub fn scaffold() -> IpamReport {
    IpamReport::new(PoolModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_available() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.status, PoolStatus::Available);
        assert_eq!(report.pool.version, VersionInfo::current());
        assert_eq!(report.pool.allocated.len(), 1);
    }

    #[test]
    fn allocate_rejects_out_of_pool_address() {
        let mut pool = PoolModel::new("10.0.2.0/24".parse().expect("valid pool"));

        let error = pool
            .allocate("10.0.3.10".parse().expect("valid address"))
            .expect_err("allocation should fail");
        assert!(matches!(error, Error::Ipam(_)));
    }
}
