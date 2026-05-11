//! Lightweight FQDN scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{Error, Result};

/// Default component name for FQDN scaffolds.
pub const COMPONENT: &str = "seriousum-fqdn";

/// DNS record type used by the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    /// An IPv4 address record.
    A,
    /// An IPv6 address record.
    Aaaa,
    /// A canonical name record.
    Cname,
    /// A free-form text record.
    Txt,
}

/// Compact DNS record model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FqdnRecord {
    /// Record name.
    pub name: String,

    /// Record type.
    pub kind: RecordKind,

    /// Record values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,

    /// Time-to-live in seconds.
    pub ttl: u32,
}

impl FqdnRecord {
    /// Creates a new DNS record.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        kind: RecordKind,
        values: impl IntoIterator<Item = impl Into<String>>,
        ttl: u32,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            values: values.into_iter().map(Into::into).collect(),
            ttl,
        }
    }
}

/// Resolution status for the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FqdnStatus {
    /// Records are waiting to be observed.
    Pending,
    /// Records are ready.
    Ready,
    /// Records exist but some entries may be stale.
    Degraded,
}

/// Compact FQDN model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FqdnModel {
    /// Component name.
    pub component: String,

    /// DNS zone handled by the scaffold.
    pub zone: String,

    /// Current resolution status.
    pub status: FqdnStatus,

    /// Managed records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<FqdnRecord>,
}

impl FqdnModel {
    /// Creates a new FQDN model.
    #[must_use]
    pub fn new(zone: impl Into<String>) -> Self {
        Self {
            component: COMPONENT.to_owned(),
            zone: zone.into(),
            status: FqdnStatus::Pending,
            records: Vec::new(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("scaffold.example")
            .with_record(FqdnRecord::new(
                "health.scaffold.example",
                RecordKind::A,
                ["127.0.0.1"],
                60,
            ))
            .with_record(FqdnRecord::new(
                "service.scaffold.example",
                RecordKind::Cname,
                ["health.scaffold.example"],
                60,
            ))
            .ready()
    }

    /// Adds a DNS record.
    #[must_use]
    pub fn with_record(mut self, record: FqdnRecord) -> Self {
        self.records.push(record);
        self
    }

    /// Marks the model as ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.status = FqdnStatus::Ready;
        self
    }

    /// Returns the number of records.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Returns a concise summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} zone={} records={} status={:?}",
            self.component,
            self.zone,
            self.record_count(),
            self.status
        )
    }

    /// Validates the FQDN model.
    pub fn validate(&self) -> Result<()> {
        if self.zone.trim().is_empty() {
            return Err(Error::Fqdn(String::from("zone must not be empty")));
        }

        if self.records.is_empty() {
            return Err(Error::Fqdn(String::from(
                "fqdn model must contain at least one record",
            )));
        }

        if self
            .records
            .iter()
            .any(|record| record.name.trim().is_empty() || record.values.is_empty())
        {
            return Err(Error::Fqdn(String::from(
                "records must have names and values",
            )));
        }

        Ok(())
    }
}

impl Default for FqdnModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable FQDN report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FqdnReport {
    /// Component name.
    pub component: String,

    /// FQDN model.
    pub fqdn: FqdnModel,

    /// Whether the model is ready.
    pub ready: bool,
}

impl FqdnReport {
    /// Builds a report from an FQDN model.
    #[must_use]
    pub fn new(fqdn: FqdnModel) -> Self {
        let ready = matches!(fqdn.status, FqdnStatus::Ready) && !fqdn.records.is_empty();
        Self {
            component: COMPONENT.to_owned(),
            fqdn,
            ready,
        }
    }
}

/// Returns the standard FQDN scaffold report.
#[must_use]
pub fn scaffold() -> FqdnReport {
    FqdnReport::new(FqdnModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_ready() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.ready);
        assert_eq!(report.fqdn.record_count(), 2);
        assert!(matches!(report.fqdn.status, FqdnStatus::Ready));
    }

    #[test]
    fn validate_rejects_missing_zone() {
        let model = FqdnModel::new("").with_record(FqdnRecord::new(
            "invalid",
            RecordKind::Txt,
            ["value"],
            30,
        ));

        let error = model.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Fqdn(_)));
    }

    #[test]
    fn report_round_trips_through_json() {
        let report = scaffold();
        let encoded = serde_json::to_string(&report).expect("serialization should succeed");
        let decoded: FqdnReport =
            serde_json::from_str(&encoded).expect("deserialization should succeed");

        assert_eq!(decoded, report);
    }
}
