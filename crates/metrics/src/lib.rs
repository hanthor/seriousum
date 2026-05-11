//! Lightweight metrics scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_api::VersionInfo;
use seriousum_core::{Error, Result};
use std::collections::BTreeMap;

/// Default component name for metrics scaffolds.
pub const COMPONENT: &str = "seriousum-metrics";

/// Metric kind used by the scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    /// A monotonically increasing counter.
    Counter,
    /// A point-in-time gauge.
    Gauge,
}

/// Individual metric sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricSample {
    /// Metric name.
    pub name: String,

    /// Metric kind.
    pub kind: MetricKind,

    /// Metric value.
    pub value: u64,

    /// Metric labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

impl MetricSample {
    /// Creates a new counter sample.
    #[must_use]
    pub fn counter(name: impl Into<String>, value: u64) -> Self {
        Self {
            name: name.into(),
            kind: MetricKind::Counter,
            value,
            labels: BTreeMap::new(),
        }
    }

    /// Adds a label to the sample.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Compact metrics model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsModel {
    /// Component name.
    pub component: String,

    /// Collected metric samples.
    pub samples: Vec<MetricSample>,

    /// Version metadata for the scaffold.
    pub version: VersionInfo,
}

impl MetricsModel {
    /// Creates a new metrics model.
    #[must_use]
    pub fn new(component: impl Into<String>) -> Self {
        Self {
            component: component.into(),
            samples: Vec::new(),
            version: VersionInfo::current(),
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(COMPONENT).with_sample(
            MetricSample::counter("scaffold_ready", 1).with_label("component", COMPONENT),
        )
    }

    /// Adds a metric sample.
    #[must_use]
    pub fn with_sample(mut self, sample: MetricSample) -> Self {
        self.samples.push(sample);
        self
    }

    /// Returns the sum of all sample values.
    #[must_use]
    pub fn total_value(&self) -> u64 {
        self.samples.iter().map(|sample| sample.value).sum()
    }

    /// Returns the number of collected samples.
    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Returns a concise summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} samples={} total={}",
            self.component,
            self.sample_count(),
            self.total_value()
        )
    }

    /// Validates the metrics model.
    pub fn validate(&self) -> Result<()> {
        if self.samples.is_empty() {
            return Err(Error::Metrics(String::from(
                "metrics model must contain at least one sample",
            )));
        }

        Ok(())
    }
}

impl Default for MetricsModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable metrics report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsReport {
    /// Component name.
    pub component: String,

    /// Metrics model.
    pub metrics: MetricsModel,

    /// Whether the model contains useful samples.
    pub ready: bool,
}

impl MetricsReport {
    /// Builds a report from a metrics model.
    #[must_use]
    pub fn new(metrics: MetricsModel) -> Self {
        let ready = !metrics.samples.is_empty();
        Self {
            component: COMPONENT.to_owned(),
            metrics,
            ready,
        }
    }
}

/// Returns the standard metrics scaffold report.
#[must_use]
pub fn scaffold() -> MetricsReport {
    MetricsReport::new(MetricsModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_has_samples() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.ready);
        assert_eq!(report.metrics.version, VersionInfo::current());
        assert_eq!(report.metrics.sample_count(), 1);
        assert_eq!(report.metrics.total_value(), 1);
    }

    #[test]
    fn validate_rejects_empty_metrics() {
        let metrics = MetricsModel::new(COMPONENT);

        let error = metrics.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Metrics(_)));
    }
}
