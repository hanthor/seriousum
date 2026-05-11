//! Lightweight BGP scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Result};
use std::net::{IpAddr, Ipv4Addr};

/// Default component name for BGP scaffolds.
pub const COMPONENT: &str = "seriousum-bgp";

/// Simple BGP session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// BGP is not yet configured.
    Idle,
    /// BGP is attempting to connect.
    Connect,
    /// BGP is exchanging updates.
    Established,
    /// BGP is unavailable.
    Down,
}

/// Compact BGP neighbor model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpNeighbor {
    /// Peer address.
    pub peer: IpAddr,

    /// Remote autonomous system number.
    pub remote_asn: u32,

    /// Current session state.
    pub state: SessionState,

    /// Number of announced prefixes.
    pub prefixes: u32,
}

impl BgpNeighbor {
    /// Creates a new neighbor entry.
    #[must_use]
    pub fn new(peer: IpAddr, remote_asn: u32) -> Self {
        Self {
            peer,
            remote_asn,
            state: SessionState::Idle,
            prefixes: 0,
        }
    }

    /// Marks the neighbor as established.
    #[must_use]
    pub fn established(mut self) -> Self {
        self.state = SessionState::Established;
        self.prefixes = 1;
        self
    }
}

/// Compact BGP model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpModel {
    /// Component name.
    pub component: String,

    /// Local router identifier.
    pub router_id: Ipv4Addr,

    /// Local autonomous system number.
    pub local_asn: u32,

    /// Configured BGP neighbors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub neighbors: Vec<BgpNeighbor>,
}

impl BgpModel {
    /// Creates a new BGP model.
    #[must_use]
    pub fn new(router_id: Ipv4Addr, local_asn: u32) -> Self {
        Self {
            component: COMPONENT.to_owned(),
            router_id,
            local_asn,
            neighbors: Vec::new(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(Ipv4Addr::new(10, 0, 0, 1), 65_000).with_neighbor(
            BgpNeighbor::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 65_001).established(),
        )
    }

    /// Adds a neighbor.
    #[must_use]
    pub fn with_neighbor(mut self, neighbor: BgpNeighbor) -> Self {
        self.neighbors.push(neighbor);
        self
    }

    /// Returns the number of established neighbors.
    #[must_use]
    pub fn established_neighbors(&self) -> usize {
        self.neighbors
            .iter()
            .filter(|neighbor| matches!(neighbor.state, SessionState::Established))
            .count()
    }

    /// Returns a concise summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} neighbors={} established={}",
            self.component,
            self.neighbors.len(),
            self.established_neighbors()
        )
    }

    /// Validates the BGP model.
    pub fn validate(&self) -> Result<()> {
        if self.local_asn == 0 {
            return Err(Error::Bgp(String::from("local ASN must be non-zero")));
        }

        if self.neighbors.is_empty() {
            return Err(Error::Bgp(String::from(
                "bgp model must contain at least one neighbor",
            )));
        }

        Ok(())
    }
}

impl Default for BgpModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable BGP report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgpReport {
    /// Component name.
    pub component: String,

    /// BGP model.
    pub bgp: BgpModel,

    /// Whether at least one neighbor is established.
    pub established: bool,
}

impl BgpReport {
    /// Builds a report from a BGP model.
    #[must_use]
    pub fn new(bgp: BgpModel) -> Self {
        let established = bgp.established_neighbors() > 0;
        Self {
            component: COMPONENT.to_owned(),
            bgp,
            established,
        }
    }
}

/// Returns the standard BGP scaffold report.
#[must_use]
pub fn scaffold() -> BgpReport {
    BgpReport::new(BgpModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_established() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.established);
        assert_eq!(report.bgp.local_asn, 65_000);
        assert_eq!(report.bgp.established_neighbors(), 1);
    }

    #[test]
    fn validate_rejects_zero_asn() {
        let model = BgpModel::new(Ipv4Addr::new(10, 0, 0, 1), 0).with_neighbor(BgpNeighbor::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            65_001,
        ));

        let error = model.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Bgp(_)));
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = scaffold();
        let encoded = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: BgpReport =
            serde_json::from_str(&encoded).expect("deserialization should succeed");

        assert_eq!(decoded, report);
    }
}
