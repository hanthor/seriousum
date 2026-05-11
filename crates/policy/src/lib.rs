//! Lightweight policy scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Identity, Port, Result, SecurityIdentity, SecurityLabel};
use std::collections::BTreeMap;

/// Default component name for policy scaffolds.
pub const COMPONENT: &str = "seriousum-policy";

/// High-level policy lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyStatus {
    /// A policy draft that is not yet enforced.
    Draft,
    /// Policy rules are enforced.
    Enforced,
    /// Policy is temporarily disabled or stale.
    Disabled,
}

/// Compact policy model for a single policy bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyModel {
    /// Policy bundle name.
    pub name: String,

    /// Policy revision counter.
    pub revision: u64,

    /// Policy lifecycle state.
    pub status: PolicyStatus,

    /// Identities selected by the policy.
    pub identities: Vec<Identity>,

    /// Ports covered by the policy.
    pub ports: Vec<Port>,

    /// Key/value metadata for future selectors and tags.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl PolicyModel {
    /// Creates a new policy model.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            revision: 1,
            status: PolicyStatus::Draft,
            identities: vec![Identity::new(
                SecurityIdentity::world(),
                [SecurityLabel::new("policy", "scaffold")],
            )],
            ports: vec![Port::cilium_agent()],
            labels: BTreeMap::from([(String::from("tier"), String::from("baseline"))]),
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("policy scaffold").enforce()
    }

    /// Marks the policy as enforced.
    #[must_use]
    pub fn enforce(mut self) -> Self {
        self.status = PolicyStatus::Enforced;
        self
    }

    /// Marks the policy as disabled.
    #[must_use]
    pub fn disable(mut self) -> Self {
        self.status = PolicyStatus::Disabled;
        self
    }

    /// Adds an identity selector.
    #[must_use]
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identities.push(identity);
        self
    }

    /// Adds a port selector.
    #[must_use]
    pub fn with_port(mut self, port: Port) -> Self {
        self.ports.push(port);
        self
    }

    /// Sets a metadata label.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Returns a short summary suitable for logging.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} rev={} identities={} ports={}",
            self.name,
            self.revision,
            self.identities.len(),
            self.ports.len()
        )
    }

    /// Validates the policy model.
    pub fn validate(&self) -> Result<()> {
        if self.ports.is_empty() {
            return Err(Error::Policy(String::from(
                "policy must include at least one port",
            )));
        }

        if matches!(self.status, PolicyStatus::Enforced) && self.identities.is_empty() {
            return Err(Error::Policy(String::from(
                "enforced policy requires a selector",
            )));
        }

        Ok(())
    }
}

impl Default for PolicyModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable policy report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyReport {
    /// Component name.
    pub component: String,

    /// Underlying policy model.
    pub policy: PolicyModel,

    /// Whether the policy is actively enforced.
    pub enforced: bool,
}

impl PolicyReport {
    /// Builds a report from a policy model.
    #[must_use]
    pub fn new(policy: PolicyModel) -> Self {
        let enforced = matches!(policy.status, PolicyStatus::Enforced);
        Self {
            component: COMPONENT.to_owned(),
            policy,
            enforced,
        }
    }
}

/// Returns the standard policy scaffold report.
#[must_use]
pub fn scaffold() -> PolicyReport {
    PolicyReport::new(PolicyModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_reports_enforced_policy() {
        let report = scaffold();

        assert!(report.enforced);
        assert_eq!(report.component, COMPONENT);
        assert_eq!(report.policy.version, VersionInfo::current());
        assert_eq!(report.policy.status, PolicyStatus::Enforced);
        assert!(report.policy.validate().is_ok());
    }

    #[test]
    fn validate_rejects_policies_without_ports() {
        let policy = PolicyModel::new("empty").disable();
        let policy = PolicyModel {
            ports: Vec::new(),
            ..policy
        };

        let error = policy.validate().expect_err("validation should fail");
        assert!(error.is_policy());
    }

    #[test]
    fn serializes_with_identity_and_labels() {
        let json = serde_json::to_value(scaffold()).expect("policy report serializes");
        assert_eq!(json["policy"]["identities"][0]["id"], 4);
        assert_eq!(json["policy"]["labels"]["tier"], "baseline");
    }
}
