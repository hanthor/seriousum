//! Lightweight identity scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Identity, Result, SecurityIdentity, SecurityLabel};
use std::collections::BTreeMap;

/// Default component name for identity scaffolds.
pub const COMPONENT: &str = "seriousum-identity";

/// Identity lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStatus {
    /// The identity is freshly observed.
    Observed,
    /// The identity is allocated to a workload.
    Allocated,
    /// The identity is no longer active.
    Released,
}

/// Compact identity model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityModel {
    /// Underlying security identity.
    pub identity: Identity,

    /// Lifecycle state.
    pub status: IdentityStatus,

    /// Owner or source of the identity.
    pub source: String,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl IdentityModel {
    /// Creates a new identity model.
    #[must_use]
    pub fn new(identity: Identity, source: impl Into<String>) -> Self {
        Self {
            identity,
            status: IdentityStatus::Observed,
            source: source.into(),
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            Identity::new(
                SecurityIdentity::world(),
                [SecurityLabel::new("identity", "scaffold")],
            ),
            "synthetic",
        )
        .allocate()
    }

    /// Marks the identity as allocated.
    #[must_use]
    pub fn allocate(mut self) -> Self {
        self.status = IdentityStatus::Allocated;
        self
    }

    /// Marks the identity as released.
    #[must_use]
    pub fn release(mut self) -> Self {
        self.status = IdentityStatus::Released;
        self
    }

    /// Returns the number of labels attached to the identity.
    #[must_use]
    pub fn label_count(&self) -> usize {
        self.identity.labels.len()
    }

    /// Returns a stable summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "identity={} labels={} source={}",
            self.identity.id,
            self.label_count(),
            self.source
        )
    }

    /// Validates the identity model.
    pub fn validate(&self) -> Result<()> {
        if self.source.trim().is_empty() {
            return Err(Error::Identity(String::from(
                "identity source must not be empty",
            )));
        }

        if self.identity.labels.is_empty() && !self.identity.id.is_reserved() {
            return Err(Error::Identity(String::from(
                "non-reserved identities must carry labels",
            )));
        }

        Ok(())
    }
}

impl Default for IdentityModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable identity report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityReport {
    /// Component name.
    pub component: String,

    /// Identity model.
    pub identity: IdentityModel,

    /// Quick lookup of labels for parity tests and serialization checks.
    pub labels: BTreeMap<String, String>,
}

impl IdentityReport {
    /// Builds a report from an identity model.
    #[must_use]
    pub fn new(identity: IdentityModel) -> Self {
        Self {
            component: COMPONENT.to_owned(),
            labels: identity.identity.labels.clone(),
            identity,
        }
    }
}

/// Returns the standard identity scaffold report.
#[must_use]
pub fn scaffold() -> IdentityReport {
    IdentityReport::new(IdentityModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_contains_labels() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.identity.version, VersionInfo::current());
        assert_eq!(report.identity.status, IdentityStatus::Allocated);
        assert_eq!(
            report.labels.get("identity").map(String::as_str),
            Some("scaffold")
        );
    }

    #[test]
    fn validate_rejects_missing_source() {
        let model = IdentityModel::new(
            Identity::new(
                SecurityIdentity::host(),
                [SecurityLabel::new("role", "node")],
            ),
            "",
        );

        let error = model.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Identity(_)));
    }
}
