//! Cilium metrics and monitoring infrastructure.
//!
//! Ported from `pkg/metrics` and `pkg/monitor`.
//!
//! Provides:
//! - Metric types (Counter, Gauge, Histogram) with label vectors
//! - Metric metadata and enable/disable control
//! - Monitor event types for datapath notifications
//! - Performance counter management
//! - Label validation and constraints

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

// Re-export serde macros for use in nested modules

/// Default component name for metrics.
pub const COMPONENT: &str = "seriousum-metrics";

/// Metrics error type.
#[derive(Debug, Error)]
pub enum Error {
    #[error("metric {name} not found")]
    MetricNotFound { name: String },

    #[error("label validation failed: {0}")]
    LabelValidation(String),

    #[error("invalid label count: expected {expected}, got {got}")]
    InvalidLabelCount { expected: usize, got: usize },

    #[error("invalid label value for {label}: {value}")]
    InvalidLabelValue { label: String, value: String },

    #[error("metric disabled: {0}")]
    MetricDisabled(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Metric kind enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    /// A monotonically increasing counter.
    Counter,
    /// A point-in-time gauge.
    Gauge,
    /// A distribution histogram with buckets.
    Histogram,
}

/// Metric options, extended Prometheus compatible configuration.
///
/// Ported from `pkg/metrics/metric.Opts` and related types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricOpts {
    /// Namespace component of the fully-qualified metric name.
    pub namespace: String,

    /// Subsystem component of the fully-qualified metric name.
    pub subsystem: String,

    /// Name component of the fully-qualified metric name.
    pub name: String,

    /// Help text describing the metric.
    pub help: String,

    /// Constant labels attached to all samples of this metric.
    pub const_labels: HashMap<String, String>,

    /// Configuration name for enabling/disabling the metric.
    pub config_name: Option<String>,

    /// If true, metric is disabled by default and must be explicitly enabled.
    pub disabled: bool,
}

impl MetricOpts {
    /// Creates a new metric options builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            namespace: String::new(),
            subsystem: String::new(),
            name: name.into(),
            help: String::new(),
            const_labels: HashMap::new(),
            config_name: None,
            disabled: false,
        }
    }

    /// Sets the namespace.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Sets the subsystem.
    pub fn with_subsystem(mut self, subsystem: impl Into<String>) -> Self {
        self.subsystem = subsystem.into();
        self
    }

    /// Sets the help text.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// Adds a constant label.
    pub fn with_const_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.const_labels.insert(key.into(), value.into());
        self
    }

    /// Sets the configuration name.
    pub fn with_config_name(mut self, config_name: impl Into<String>) -> Self {
        self.config_name = Some(config_name.into());
        self
    }

    /// Marks the metric as disabled by default.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Returns the fully-qualified metric name.
    pub fn fq_name(&self) -> String {
        let parts: Vec<&str> = [
            (!self.namespace.is_empty()).then_some(self.namespace.as_str()),
            (!self.subsystem.is_empty()).then_some(self.subsystem.as_str()),
            Some(self.name.as_str()),
        ]
        .iter()
        .filter_map(|p| *p)
        .collect();

        parts.join("_")
    }

    /// Returns the configuration name for this metric.
    pub fn get_config_name(&self) -> String {
        self.config_name.clone().unwrap_or_else(|| self.fq_name())
    }
}

impl Default for MetricOpts {
    fn default() -> Self {
        Self::new("metric")
    }
}

/// A metric value representing a single observation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricValue(pub f64);

impl MetricValue {
    pub fn new(value: f64) -> Self {
        Self(value)
    }

    pub fn zero() -> Self {
        Self(0.0)
    }

    pub fn get(&self) -> f64 {
        self.0
    }
}

impl From<u64> for MetricValue {
    #[allow(clippy::cast_precision_loss)]
    fn from(v: u64) -> Self {
        Self(v as f64)
    }
}

impl From<i64> for MetricValue {
    #[allow(clippy::cast_precision_loss)]
    fn from(v: i64) -> Self {
        Self(v as f64)
    }
}

impl From<f64> for MetricValue {
    fn from(v: f64) -> Self {
        Self(v)
    }
}

/// Label value specification.
///
/// Represents a set of allowed values for a particular label name.
/// Ported from `pkg/metrics/metric.Values`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelValues(pub std::collections::HashSet<String>);

impl LabelValues {
    /// Creates a new label values set.
    pub fn new(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(
            values
                .into_iter()
                .map(std::convert::Into::into)
                .collect::<std::collections::HashSet<_>>(),
        )
    }

    /// Checks if a value is allowed.
    pub fn contains(&self, value: &str) -> bool {
        self.0.is_empty() || self.0.contains(value)
    }

    /// Returns true if the set is empty (no constraints).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns all values.
    pub fn all(&self) -> Vec<String> {
        let mut vals: Vec<_> = self.0.iter().cloned().collect();
        vals.sort();
        vals
    }
}

impl From<Vec<&str>> for LabelValues {
    fn from(vals: Vec<&str>) -> Self {
        Self::new(vals)
    }
}

impl From<Vec<String>> for LabelValues {
    fn from(vals: Vec<String>) -> Self {
        Self::new(vals)
    }
}

/// Label definition with name and allowed values.
///
/// Ported from `pkg/metrics/metric.Label`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub values: LabelValues,
}

impl Label {
    pub fn new(name: impl Into<String>, values: LabelValues) -> Self {
        Self {
            name: name.into(),
            values,
        }
    }

    /// Creates a label with no value constraints.
    pub fn unconstrained(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: LabelValues(std::collections::HashSet::new()),
        }
    }

    /// Creates a label with specific allowed values.
    pub fn with_values(
        name: impl Into<String>,
        values: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            values: LabelValues::new(values),
        }
    }
}

/// Label set defining the label dimensions for a vectorized metric.
///
/// Ported from `pkg/metrics/metric.Labels`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels(pub Vec<Label>);

impl Labels {
    pub fn new(labels: Vec<Label>) -> Self {
        Self(labels)
    }

    /// Returns the label names in order.
    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|l| l.name.clone()).collect()
    }

    /// Validates label values against the defined constraints.
    pub fn validate_values(&self, values: &[&str]) -> Result<()> {
        if self.0.len() != values.len() {
            return Err(Error::InvalidLabelCount {
                expected: self.0.len(),
                got: values.len(),
            });
        }

        for (i, label) in self.0.iter().enumerate() {
            if !label.values.contains(values[i]) {
                return Err(Error::InvalidLabelValue {
                    label: label.name.clone(),
                    value: values[i].to_string(),
                });
            }
        }

        Ok(())
    }

    /// Validates a label map against the defined constraints.
    pub fn validate_map(&self, labels: &HashMap<String, String>) -> std::result::Result<(), Error> {
        let names = self.names();
        for name in names {
            if !labels.contains_key(&name) {
                return Err(Error::LabelValidation(format!("missing label: {name}")));
            }
            let value = labels.get(&name).unwrap();
            if let Some(label) = self.0.iter().find(|l| l.name == name)
                && !label.values.contains(value)
            {
                return Err(Error::InvalidLabelValue {
                    label: name,
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Base metric metadata trait.
///
/// Ported from `pkg/metrics/metric.WithMetadata`.
pub trait WithMetadata {
    /// Returns whether this metric is enabled.
    fn is_enabled(&self) -> bool;

    /// Sets the enabled state.
    fn set_enabled(&mut self, enabled: bool);

    /// Returns the metric options.
    fn opts(&self) -> &MetricOpts;

    /// Returns the kind of metric.
    fn kind(&self) -> MetricKind;
}

/// Base metric structure providing common functionality.
#[derive(Debug, Clone)]
pub struct MetricBase {
    pub enabled: bool,
    pub opts: MetricOpts,
    pub kind: MetricKind,
}

impl MetricBase {
    pub fn new(opts: MetricOpts, kind: MetricKind) -> Self {
        Self {
            enabled: !opts.disabled,
            opts,
            kind,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

impl WithMetadata for MetricBase {
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn opts(&self) -> &MetricOpts {
        &self.opts
    }

    fn kind(&self) -> MetricKind {
        self.kind
    }
}

/// Counter metric type.
///
/// Ported from `pkg/metrics/metric.Counter` and `pkg/metrics/metric.counter`.
#[derive(Debug, Clone)]
pub struct Counter {
    pub base: MetricBase,
    pub value: Arc<std::sync::atomic::AtomicU64>,
}

impl Counter {
    pub fn new(opts: MetricOpts) -> Self {
        Self {
            base: MetricBase::new(opts, MetricKind::Counter),
            value: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Increments the counter by 1.
    pub fn inc(&self) {
        if self.base.enabled {
            self.value.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Adds a delta to the counter.
    pub fn add(&self, delta: u64) {
        if self.base.enabled {
            self.value
                .fetch_add(delta, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Returns the current counter value.
    pub fn get(&self) -> u64 {
        self.value.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Resets the counter to zero.
    pub fn reset(&self) {
        self.value.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

impl WithMetadata for Counter {
    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
        if !enabled {
            self.reset();
        }
    }

    fn opts(&self) -> &MetricOpts {
        self.base.opts()
    }

    fn kind(&self) -> MetricKind {
        self.base.kind()
    }
}

/// Vectorized counter metric type.
///
/// Ported from `pkg/metrics/metric.counterVec`.
#[derive(Debug, Clone)]
pub struct CounterVec {
    pub base: MetricBase,
    pub labels: Labels,
    pub counters: Arc<DashMap<Vec<String>, Counter>>,
}

impl CounterVec {
    pub fn new(opts: MetricOpts, labels: Labels) -> Self {
        Self {
            base: MetricBase::new(opts, MetricKind::Counter),
            labels,
            counters: Arc::new(DashMap::new()),
        }
    }

    /// Gets or creates a counter for the given label values.
    pub fn with_label_values(&self, values: &[&str]) -> Result<Counter> {
        self.labels.validate_values(values)?;

        let key = values
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();

        if let Some(counter) = self.counters.get(&key) {
            return Ok(counter.clone());
        }

        let counter = Counter::new(self.base.opts.clone());
        self.counters.insert(key, counter.clone());
        Ok(counter)
    }

    /// Gets or creates a counter for the given label map.
    pub fn with_labels(&self, labels_map: &HashMap<String, String>) -> Result<Counter> {
        self.labels.validate_map(labels_map)?;

        let names = self.labels.names();
        let mut key = Vec::new();
        for name in names {
            key.push(
                labels_map
                    .get(&name)
                    .ok_or_else(|| Error::LabelValidation(format!("missing label: {name}")))?
                    .clone(),
            );
        }

        if let Some(counter) = self.counters.get(&key) {
            return Ok(counter.clone());
        }

        let counter = Counter::new(self.base.opts.clone());
        self.counters.insert(key, counter.clone());
        Ok(counter)
    }

    /// Returns all active counters.
    pub fn all(&self) -> Vec<(Vec<String>, u64)> {
        self.counters
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().get()))
            .collect()
    }

    /// Deletes a counter by label values.
    pub fn delete_label_values(&self, values: &[&str]) -> bool {
        self.labels.validate_values(values).ok();
        let key = values
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        self.counters.remove(&key).is_some()
    }

    /// Resets all counters.
    pub fn reset(&self) {
        self.counters.clear();
    }
}

impl WithMetadata for CounterVec {
    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
        if !enabled {
            self.reset();
        }
    }

    fn opts(&self) -> &MetricOpts {
        self.base.opts()
    }

    fn kind(&self) -> MetricKind {
        self.base.kind()
    }
}

/// Gauge metric type.
///
/// Ported from `pkg/metrics/metric.Gauge` and `pkg/metrics/metric.gauge`.
#[derive(Debug, Clone)]
pub struct Gauge {
    pub base: MetricBase,
    pub value: Arc<std::sync::atomic::AtomicU64>,
}

impl Gauge {
    pub fn new(opts: MetricOpts) -> Self {
        Self {
            base: MetricBase::new(opts, MetricKind::Gauge),
            value: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Sets the gauge to a specific value.
    pub fn set(&self, value: u64) {
        if self.base.enabled {
            self.value.store(value, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Increments the gauge by 1.
    pub fn inc(&self) {
        if self.base.enabled {
            self.value.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Decrements the gauge by 1.
    pub fn dec(&self) {
        if self.base.enabled {
            self.value.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Adds a delta to the gauge.
    pub fn add(&self, delta: u64) {
        if self.base.enabled {
            self.value
                .fetch_add(delta, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Subtracts a delta from the gauge.
    pub fn sub(&self, delta: u64) {
        if self.base.enabled {
            self.value
                .fetch_sub(delta, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Returns the current gauge value.
    pub fn get(&self) -> u64 {
        self.value.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl WithMetadata for Gauge {
    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }

    fn opts(&self) -> &MetricOpts {
        self.base.opts()
    }

    fn kind(&self) -> MetricKind {
        self.base.kind()
    }
}

/// Vectorized gauge metric type.
///
/// Ported from `pkg/metrics/metric.gaugeVec`.
#[derive(Debug, Clone)]
pub struct GaugeVec {
    pub base: MetricBase,
    pub labels: Labels,
    pub gauges: Arc<DashMap<Vec<String>, Gauge>>,
}

impl GaugeVec {
    pub fn new(opts: MetricOpts, labels: Labels) -> Self {
        Self {
            base: MetricBase::new(opts, MetricKind::Gauge),
            labels,
            gauges: Arc::new(DashMap::new()),
        }
    }

    /// Gets or creates a gauge for the given label values.
    pub fn with_label_values(&self, values: &[&str]) -> Result<Gauge> {
        self.labels.validate_values(values)?;

        let key = values
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();

        if let Some(gauge) = self.gauges.get(&key) {
            return Ok(gauge.clone());
        }

        let gauge = Gauge::new(self.base.opts.clone());
        self.gauges.insert(key, gauge.clone());
        Ok(gauge)
    }

    /// Gets or creates a gauge for the given label map.
    pub fn with_labels(&self, labels_map: &HashMap<String, String>) -> Result<Gauge> {
        self.labels.validate_map(labels_map)?;
        let names = self.labels.names();
        let mut key = Vec::new();
        for name in names {
            key.push(
                labels_map
                    .get(&name)
                    .ok_or_else(|| Error::LabelValidation(format!("missing label: {name}")))?
                    .clone(),
            );
        }

        if let Some(gauge) = self.gauges.get(&key) {
            return Ok(gauge.clone());
        }

        let gauge = Gauge::new(self.base.opts.clone());
        self.gauges.insert(key, gauge.clone());
        Ok(gauge)
    }

    /// Returns all active gauges.
    pub fn all(&self) -> Vec<(Vec<String>, u64)> {
        self.gauges
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().get()))
            .collect()
    }

    /// Deletes a gauge by label values.
    pub fn delete_label_values(&self, values: &[&str]) -> bool {
        self.labels.validate_values(values).ok();
        let key = values
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        self.gauges.remove(&key).is_some()
    }

    /// Resets all gauges.
    pub fn reset(&self) {
        self.gauges.clear();
    }
}

impl WithMetadata for GaugeVec {
    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }

    fn opts(&self) -> &MetricOpts {
        self.base.opts()
    }

    fn kind(&self) -> MetricKind {
        self.base.kind()
    }
}

/// Histogram bucket definition.
#[derive(Debug, Clone, PartialEq)]
pub struct HistogramBucket {
    pub le: f64, // upper bound (inclusive)
    pub count: u64,
}

/// Histogram metric type.
///
/// Ported from `pkg/metrics/metric.Histogram`.
#[derive(Debug, Clone)]
pub struct Histogram {
    pub base: MetricBase,
    pub buckets: Arc<Vec<f64>>,
    pub bucket_counts: Arc<DashMap<usize, u64>>,
    pub sum: Arc<std::sync::atomic::AtomicU64>,
    pub count: Arc<std::sync::atomic::AtomicU64>,
}

impl Histogram {
    pub fn new(opts: MetricOpts, buckets: Vec<f64>) -> Self {
        let buckets = Arc::new({
            let mut b = buckets;
            b.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            b
        });

        Self {
            base: MetricBase::new(opts, MetricKind::Histogram),
            buckets,
            bucket_counts: Arc::new(DashMap::new()),
            sum: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Observes a value in the histogram.
    pub fn observe(&self, value: f64) {
        if !self.base.enabled {
            return;
        }

        // Find the appropriate bucket
        for (i, &bucket) in self.buckets.iter().enumerate() {
            if value <= bucket {
                self.bucket_counts
                    .entry(i)
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
        }

        // Always increment +Inf bucket
        let inf_bucket = self.buckets.len();
        self.bucket_counts
            .entry(inf_bucket)
            .and_modify(|c| *c += 1)
            .or_insert(1);

        // Update sum and count
        let bits = value.to_bits();
        self.sum
            .fetch_add(bits, std::sync::atomic::Ordering::SeqCst);
        self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Returns the number of observations.
    pub fn count(&self) -> u64 {
        self.count.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Resets the histogram.
    pub fn reset(&self) {
        self.bucket_counts.clear();
        self.sum.store(0, std::sync::atomic::Ordering::SeqCst);
        self.count.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

impl WithMetadata for Histogram {
    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
        if !enabled {
            self.reset();
        }
    }

    fn opts(&self) -> &MetricOpts {
        self.base.opts()
    }

    fn kind(&self) -> MetricKind {
        self.base.kind()
    }
}

/// Vectorized histogram metric type.
///
/// Ported from `pkg/metrics/metric.histogramVec`.
#[derive(Debug, Clone)]
pub struct HistogramVec {
    pub base: MetricBase,
    pub labels: Labels,
    pub buckets: Arc<Vec<f64>>,
    pub histograms: Arc<DashMap<Vec<String>, Histogram>>,
}

impl HistogramVec {
    pub fn new(opts: MetricOpts, labels: Labels, buckets: Vec<f64>) -> Self {
        Self {
            base: MetricBase::new(opts, MetricKind::Histogram),
            labels,
            buckets: Arc::new(buckets),
            histograms: Arc::new(DashMap::new()),
        }
    }

    /// Gets or creates a histogram for the given label values.
    pub fn with_label_values(&self, values: &[&str]) -> Result<Histogram> {
        self.labels.validate_values(values)?;

        let key = values
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();

        if let Some(hist) = self.histograms.get(&key) {
            return Ok(hist.clone());
        }

        let hist = Histogram::new(self.base.opts.clone(), (*self.buckets).clone());
        self.histograms.insert(key, hist.clone());
        Ok(hist)
    }

    /// Gets or creates a histogram for the given label map.
    pub fn with_labels(&self, labels_map: &HashMap<String, String>) -> Result<Histogram> {
        self.labels.validate_map(labels_map)?;
        let names = self.labels.names();
        let mut key = Vec::new();
        for name in names {
            key.push(
                labels_map
                    .get(&name)
                    .ok_or_else(|| Error::LabelValidation(format!("missing label: {name}")))?
                    .clone(),
            );
        }

        if let Some(hist) = self.histograms.get(&key) {
            return Ok(hist.clone());
        }

        let hist = Histogram::new(self.base.opts.clone(), (*self.buckets).clone());
        self.histograms.insert(key, hist.clone());
        Ok(hist)
    }

    /// Resets all histograms.
    pub fn reset(&self) {
        self.histograms.clear();
    }
}

impl WithMetadata for HistogramVec {
    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
        if !enabled {
            self.reset();
        }
    }

    fn opts(&self) -> &MetricOpts {
        self.base.opts()
    }

    fn kind(&self) -> MetricKind {
        self.base.kind()
    }
}

/// In-memory Prometheus-style metric primitives and registry types.
///
/// These types model pure registration and collection behavior without exposing
/// an HTTP endpoint or scrape handler.
pub mod registry {
    use std::collections::HashMap;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicI64, AtomicU64, Ordering},
    };

    /// A label key-value pair attached to a metric.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct Label {
        /// Label key.
        pub key: String,
        /// Label value.
        pub value: String,
    }

    impl Label {
        /// Creates a new label pair.
        pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
            Self {
                key: key.into(),
                value: value.into(),
            }
        }
    }

    /// Common metric metadata.
    #[derive(Debug, Clone)]
    pub struct MetricDesc {
        /// Metric name.
        pub name: String,
        /// Human-readable help text.
        pub help: String,
        /// Ordered label names expected by the metric.
        pub label_names: Vec<String>,
    }

    impl MetricDesc {
        /// Creates a new metric descriptor.
        pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                help: help.into(),
                label_names: vec![],
            }
        }

        /// Attaches label names to the descriptor.
        pub fn with_labels<I, S>(mut self, labels: I) -> Self
        where
            I: IntoIterator<Item = S>,
            S: Into<String>,
        {
            self.label_names = labels.into_iter().map(Into::into).collect();
            self
        }
    }

    /// Errors returned by the in-memory metrics registry.
    #[derive(Debug, thiserror::Error)]
    pub enum MetricsError {
        /// Returned when a metric name is registered twice.
        #[error("metric already registered: {0}")]
        AlreadyRegistered(String),
        /// Returned when a requested metric does not exist.
        #[error("metric not found: {0}")]
        NotFound(String),
    }

    /// A monotonically increasing counter.
    #[derive(Debug, Clone)]
    pub struct Counter {
        desc: MetricDesc,
        value: Arc<AtomicU64>,
    }

    impl Counter {
        /// Creates a new counter.
        pub fn new(desc: MetricDesc) -> Self {
            Self {
                desc,
                value: Arc::new(AtomicU64::new(0)),
            }
        }

        /// Increments the counter by one.
        pub fn inc(&self) {
            self.value.fetch_add(1, Ordering::Relaxed);
        }

        /// Adds an arbitrary delta to the counter.
        pub fn add(&self, n: u64) {
            self.value.fetch_add(n, Ordering::Relaxed);
        }

        /// Returns the current counter value.
        pub fn get(&self) -> u64 {
            self.value.load(Ordering::Relaxed)
        }

        /// Returns the metric descriptor.
        pub fn desc(&self) -> &MetricDesc {
            &self.desc
        }
    }

    /// A gauge that can go up and down.
    #[derive(Debug, Clone)]
    pub struct Gauge {
        desc: MetricDesc,
        value: Arc<AtomicI64>,
    }

    impl Gauge {
        /// Creates a new gauge.
        pub fn new(desc: MetricDesc) -> Self {
            Self {
                desc,
                value: Arc::new(AtomicI64::new(0)),
            }
        }

        /// Sets the gauge to an exact value.
        pub fn set(&self, v: i64) {
            self.value.store(v, Ordering::Relaxed);
        }

        /// Increments the gauge by one.
        pub fn inc(&self) {
            self.value.fetch_add(1, Ordering::Relaxed);
        }

        /// Decrements the gauge by one.
        pub fn dec(&self) {
            self.value.fetch_sub(1, Ordering::Relaxed);
        }

        /// Adds a signed delta to the gauge.
        pub fn add(&self, n: i64) {
            self.value.fetch_add(n, Ordering::Relaxed);
        }

        /// Returns the current gauge value.
        pub fn get(&self) -> i64 {
            self.value.load(Ordering::Relaxed)
        }

        /// Returns the metric descriptor.
        pub fn desc(&self) -> &MetricDesc {
            &self.desc
        }
    }

    /// A histogram with configurable buckets.
    #[derive(Debug, Clone)]
    pub struct Histogram {
        desc: MetricDesc,
        buckets: Vec<f64>,
        counts: Arc<Mutex<Vec<u64>>>,
        sum: Arc<AtomicU64>,
        total_count: Arc<AtomicU64>,
    }

    impl Histogram {
        /// Creates a histogram with explicit bucket upper bounds.
        pub fn new(desc: MetricDesc, mut buckets: Vec<f64>) -> Self {
            buckets.sort_by(f64::total_cmp);
            let bucket_count = buckets.len();
            Self {
                desc,
                buckets,
                counts: Arc::new(Mutex::new(vec![0; bucket_count])),
                sum: Arc::new(AtomicU64::new(0.0f64.to_bits())),
                total_count: Arc::new(AtomicU64::new(0)),
            }
        }

        /// Returns the default Prometheus latency buckets in seconds.
        pub fn default_buckets() -> Vec<f64> {
            vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]
        }

        /// Records an observation.
        pub fn observe(&self, v: f64) {
            self.total_count.fetch_add(1, Ordering::Relaxed);
            self.add_to_sum(v);

            if let Some(index) = self.buckets.iter().position(|&upper| v <= upper) {
                let mut counts = self.counts.lock().unwrap();
                counts[index] += 1;
            }
        }

        /// Returns the total number of observations.
        pub fn count(&self) -> u64 {
            self.total_count.load(Ordering::Relaxed)
        }

        /// Returns the sum of all observed values.
        pub fn sum(&self) -> f64 {
            f64::from_bits(self.sum.load(Ordering::Relaxed))
        }

        /// Returns the metric descriptor.
        pub fn desc(&self) -> &MetricDesc {
            &self.desc
        }

        /// Returns `(upper_bound, cumulative_count)` pairs.
        pub fn bucket_counts(&self) -> Vec<(f64, u64)> {
            let counts = self.counts.lock().unwrap();
            let mut cumulative = 0u64;
            self.buckets
                .iter()
                .zip(counts.iter())
                .map(|(&upper, &count)| {
                    cumulative += count;
                    (upper, cumulative)
                })
                .collect()
        }

        fn add_to_sum(&self, value: f64) {
            let mut current = self.sum.load(Ordering::Relaxed);
            loop {
                let next = (f64::from_bits(current) + value).to_bits();
                match self.sum.compare_exchange_weak(
                    current,
                    next,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(observed) => current = observed,
                }
            }
        }
    }

    /// Metric variant for type-erased registry storage.
    #[derive(Debug, Clone)]
    pub enum Metric {
        /// Counter metric.
        Counter(Counter),
        /// Gauge metric.
        Gauge(Gauge),
        /// Histogram metric.
        Histogram(Histogram),
    }

    impl Metric {
        /// Returns the metric name.
        pub fn name(&self) -> &str {
            match self {
                Self::Counter(counter) => &counter.desc().name,
                Self::Gauge(gauge) => &gauge.desc().name,
                Self::Histogram(histogram) => &histogram.desc().name,
            }
        }
    }

    /// An in-memory metric registry with no Prometheus HTTP endpoint.
    #[derive(Debug, Default)]
    pub struct Registry {
        metrics: HashMap<String, Metric>,
    }

    impl Registry {
        /// Creates a new empty registry.
        pub fn new() -> Self {
            Self::default()
        }

        /// Registers a counter metric.
        pub fn register_counter(&mut self, desc: MetricDesc) -> Result<Counter, MetricsError> {
            let name = desc.name.clone();
            if self.metrics.contains_key(&name) {
                return Err(MetricsError::AlreadyRegistered(name));
            }
            let counter = Counter::new(desc);
            tracing::debug!(metric = %counter.desc().name, "registering counter");
            self.metrics.insert(
                counter.desc().name.clone(),
                Metric::Counter(counter.clone()),
            );
            Ok(counter)
        }

        /// Registers a gauge metric.
        pub fn register_gauge(&mut self, desc: MetricDesc) -> Result<Gauge, MetricsError> {
            let name = desc.name.clone();
            if self.metrics.contains_key(&name) {
                return Err(MetricsError::AlreadyRegistered(name));
            }
            let gauge = Gauge::new(desc);
            tracing::debug!(metric = %gauge.desc().name, "registering gauge");
            self.metrics
                .insert(gauge.desc().name.clone(), Metric::Gauge(gauge.clone()));
            Ok(gauge)
        }

        /// Registers a histogram metric.
        pub fn register_histogram(
            &mut self,
            desc: MetricDesc,
            buckets: Vec<f64>,
        ) -> Result<Histogram, MetricsError> {
            let name = desc.name.clone();
            if self.metrics.contains_key(&name) {
                return Err(MetricsError::AlreadyRegistered(name));
            }
            let histogram = Histogram::new(desc, buckets);
            tracing::debug!(metric = %histogram.desc().name, "registering histogram");
            self.metrics.insert(
                histogram.desc().name.clone(),
                Metric::Histogram(histogram.clone()),
            );
            Ok(histogram)
        }

        /// Looks up a metric by name.
        pub fn get(&self, name: &str) -> Option<&Metric> {
            self.metrics.get(name)
        }

        /// Returns the number of registered metrics.
        pub fn len(&self) -> usize {
            self.metrics.len()
        }

        /// Returns true when the registry is empty.
        pub fn is_empty(&self) -> bool {
            self.metrics.is_empty()
        }

        /// Collects a snapshot of counter and gauge values by metric name.
        pub fn snapshot(&self) -> HashMap<String, i64> {
            self.metrics
                .iter()
                .filter_map(|(name, metric)| {
                    let value = match metric {
                        Metric::Counter(counter) => {
                            Some(i64::try_from(counter.get()).unwrap_or(i64::MAX))
                        }
                        Metric::Gauge(gauge) => Some(gauge.get()),
                        Metric::Histogram(_) => None,
                    };
                    value.map(|value| (name.clone(), value))
                })
                .collect()
        }
    }
}

/// Common Cilium metric names mirrored from `pkg/metrics`.
pub mod names {
    /// Endpoint count gauge name.
    pub const ENDPOINT_COUNT: &str = "cilium_endpoint_count";
    /// Policy regeneration counter name.
    pub const POLICY_REGENERATION_TOTAL: &str = "cilium_policy_regeneration_total";
    /// Identity count gauge name.
    pub const IDENTITY_COUNT: &str = "cilium_identity_count";
    /// Proxy redirect count gauge name.
    pub const PROXY_REDIRECTS: &str = "cilium_proxy_redirects";
    /// Drop count counter name.
    pub const DROP_COUNT: &str = "cilium_drop_count_total";
    /// Forward count counter name.
    pub const FORWARD_COUNT: &str = "cilium_forward_count_total";
    /// BPF map operations counter name.
    pub const BPF_MAP_OPS: &str = "cilium_bpf_map_ops_total";
    /// Kubernetes event received counter name.
    pub const K8S_EVENT_RECEIVED: &str = "cilium_k8s_event_received_total";
}

// ============================================================================
// Monitor event types (ported from pkg/monitor/api/types.go)
// ============================================================================

/// Monitor event type constants.
///
/// Ported from `pkg/monitor/api/types.go`.
pub mod monitor {
    /// Unspecified message type (reserved).
    pub const MESSAGE_TYPE_UNSPEC: u8 = 0;

    /// Drop notification (BPF datapath).
    pub const MESSAGE_TYPE_DROP: u8 = 1;

    /// Debug message (BPF datapath).
    pub const MESSAGE_TYPE_DEBUG: u8 = 2;

    /// Capture message (BPF datapath).
    pub const MESSAGE_TYPE_CAPTURE: u8 = 3;

    /// Trace notification (BPF datapath).
    pub const MESSAGE_TYPE_TRACE: u8 = 4;

    /// Policy verdict notification (BPF datapath).
    pub const MESSAGE_TYPE_POLICY_VERDICT: u8 = 5;

    /// Trace socket notification (BPF datapath).
    pub const MESSAGE_TYPE_TRACE_SOCK: u8 = 7;

    /// Access log (L7 proxy).
    pub const MESSAGE_TYPE_ACCESS_LOG: u8 = 129;

    /// Agent notification.
    pub const MESSAGE_TYPE_AGENT: u8 = 130;

    /// MessageType enumeration.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum MessageType {
        Unspec,
        Drop,
        Debug,
        Capture,
        Trace,
        PolicyVerdict,
        TraceSock,
        AccessLog,
        Agent,
    }

    impl MessageType {
        pub fn to_u8(self) -> u8 {
            match self {
                Self::Unspec => MESSAGE_TYPE_UNSPEC,
                Self::Drop => MESSAGE_TYPE_DROP,
                Self::Debug => MESSAGE_TYPE_DEBUG,
                Self::Capture => MESSAGE_TYPE_CAPTURE,
                Self::Trace => MESSAGE_TYPE_TRACE,
                Self::PolicyVerdict => MESSAGE_TYPE_POLICY_VERDICT,
                Self::TraceSock => MESSAGE_TYPE_TRACE_SOCK,
                Self::AccessLog => MESSAGE_TYPE_ACCESS_LOG,
                Self::Agent => MESSAGE_TYPE_AGENT,
            }
        }

        pub fn from_u8(val: u8) -> Option<Self> {
            match val {
                MESSAGE_TYPE_UNSPEC => Some(Self::Unspec),
                MESSAGE_TYPE_DROP => Some(Self::Drop),
                MESSAGE_TYPE_DEBUG => Some(Self::Debug),
                MESSAGE_TYPE_CAPTURE => Some(Self::Capture),
                MESSAGE_TYPE_TRACE => Some(Self::Trace),
                MESSAGE_TYPE_POLICY_VERDICT => Some(Self::PolicyVerdict),
                MESSAGE_TYPE_TRACE_SOCK => Some(Self::TraceSock),
                MESSAGE_TYPE_ACCESS_LOG => Some(Self::AccessLog),
                MESSAGE_TYPE_AGENT => Some(Self::Agent),
                _ => None,
            }
        }

        pub fn as_str(self) -> &'static str {
            match self {
                Self::Unspec => "unspec",
                Self::Drop => "drop",
                Self::Debug => "debug",
                Self::Capture => "capture",
                Self::Trace => "trace",
                Self::PolicyVerdict => "policy-verdict",
                Self::TraceSock => "trace-sock",
                Self::AccessLog => "l7",
                Self::Agent => "agent",
            }
        }
    }

    impl std::fmt::Display for MessageType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.as_str())
        }
    }

    /// Message type filter.
    #[derive(Debug, Clone, Default)]
    pub struct MessageTypeFilter(pub Vec<MessageType>);

    impl MessageTypeFilter {
        pub fn new() -> Self {
            Self::default()
        }

        #[allow(clippy::should_implement_trait)]
        pub fn add(mut self, msg_type: MessageType) -> Self {
            if !self.0.contains(&msg_type) {
                self.0.push(msg_type);
            }
            self
        }

        pub fn contains(&self, msg_type: MessageType) -> bool {
            if self.0.is_empty() {
                return true; // Empty filter matches all
            }
            self.0.contains(&msg_type)
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        pub fn len(&self) -> usize {
            self.0.len()
        }

        pub fn all() -> Self {
            Self(vec![
                MessageType::Drop,
                MessageType::Debug,
                MessageType::Capture,
                MessageType::Trace,
                MessageType::PolicyVerdict,
                MessageType::TraceSock,
                MessageType::AccessLog,
                MessageType::Agent,
            ])
        }
    }

    /// Drop reason enumeration.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum DropReason {
        InvalidSourceMac = 1,
        InvalidDestMac = 2,
        InvalidSourceIp = 3,
        PolicyDenied = 4,
        Unknown,
    }

    impl DropReason {
        pub fn to_u32(self) -> u32 {
            match self {
                Self::InvalidSourceMac => 1,
                Self::InvalidDestMac => 2,
                Self::InvalidSourceIp => 3,
                Self::PolicyDenied => 4,
                Self::Unknown => u32::MAX,
            }
        }

        pub fn from_u32(val: u32) -> Self {
            match val {
                1 => Self::InvalidSourceMac,
                2 => Self::InvalidDestMac,
                3 => Self::InvalidSourceIp,
                4 => Self::PolicyDenied,
                _ => Self::Unknown,
            }
        }
    }
}

/// Ring buffer event notification.
///
/// Ported from `pkg/monitor` ring buffer event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingBufferEvent {
    pub timestamp: u64,
    pub message_type: u8,
    pub data: Vec<u8>,
}

impl RingBufferEvent {
    pub fn new(message_type: u8, data: Vec<u8>) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            message_type,
            data,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========== MetricOpts Tests ==========

    #[test]
    fn test_metric_opts_new() {
        let opts = MetricOpts::new("test_metric");
        assert_eq!(opts.name, "test_metric");
        assert_eq!(opts.namespace, "");
        assert_eq!(opts.subsystem, "");
        assert!(!opts.disabled);
    }

    #[test]
    fn test_metric_opts_fq_name() {
        let opts = MetricOpts::new("metric")
            .with_namespace("cilium")
            .with_subsystem("endpoint");
        assert_eq!(opts.fq_name(), "cilium_endpoint_metric");
    }

    #[test]
    fn test_metric_opts_fq_name_partial() {
        let opts = MetricOpts::new("metric").with_namespace("cilium");
        assert_eq!(opts.fq_name(), "cilium_metric");
    }

    // ========== Label Tests ==========

    #[test]
    fn test_label_values_contains() {
        let values = LabelValues::new(vec!["a", "b", "c"]);
        assert!(values.contains("a"));
        assert!(values.contains("b"));
        assert!(!values.contains("d"));
    }

    #[test]
    fn test_label_values_unconstrained() {
        let values: LabelValues = LabelValues::new(Vec::<String>::new());
        assert!(values.is_empty());
        assert!(values.contains("anything"));
    }

    #[test]
    fn test_labels_validate_values_success() {
        let labels = Labels::new(vec![
            Label::with_values("method", vec!["GET", "POST"]),
            Label::with_values("status", vec!["200", "404"]),
        ]);

        assert!(labels.validate_values(&["GET", "200"]).is_ok());
        assert!(labels.validate_values(&["POST", "404"]).is_ok());
    }

    #[test]
    fn test_labels_validate_values_failure() {
        let labels = Labels::new(vec![
            Label::with_values("method", vec!["GET", "POST"]),
            Label::with_values("status", vec!["200", "404"]),
        ]);

        assert!(labels.validate_values(&["DELETE", "200"]).is_err());
        assert!(labels.validate_values(&["GET"]).is_err()); // Wrong count
    }

    #[test]
    fn test_labels_names() {
        let labels = Labels::new(vec![
            Label::unconstrained("method"),
            Label::unconstrained("status"),
        ]);
        assert_eq!(labels.names(), vec!["method", "status"]);
    }

    // ========== Counter Tests ==========

    #[test]
    fn test_counter_new() {
        let counter = Counter::new(MetricOpts::new("test_counter"));
        assert!(counter.is_enabled());
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_counter_inc() {
        let counter = Counter::new(MetricOpts::new("test_counter"));
        counter.inc();
        assert_eq!(counter.get(), 1);
        counter.inc();
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_counter_add() {
        let counter = Counter::new(MetricOpts::new("test_counter"));
        counter.add(5);
        assert_eq!(counter.get(), 5);
        counter.add(3);
        assert_eq!(counter.get(), 8);
    }

    #[test]
    fn test_counter_disabled() {
        let counter = Counter::new(MetricOpts::new("test_counter").disabled());
        assert!(!counter.is_enabled());
        counter.inc();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_counter_reset() {
        let counter = Counter::new(MetricOpts::new("test_counter"));
        counter.add(42);
        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    // ========== CounterVec Tests ==========

    #[test]
    fn test_counter_vec_new() {
        let labels = Labels::new(vec![Label::unconstrained("method")]);
        let vec = CounterVec::new(MetricOpts::new("requests"), labels);
        assert!(vec.is_enabled());
    }

    #[test]
    fn test_counter_vec_with_label_values() {
        let labels = Labels::new(vec![
            Label::with_values("method", vec!["GET", "POST"]),
            Label::with_values("status", vec!["200", "404"]),
        ]);
        let vec = CounterVec::new(MetricOpts::new("requests"), labels);

        let counter1 = vec.with_label_values(&["GET", "200"]).unwrap();
        let counter2 = vec.with_label_values(&["POST", "404"]).unwrap();

        counter1.inc();
        counter2.add(5);

        assert_eq!(counter1.get(), 1);
        assert_eq!(counter2.get(), 5);
    }

    #[test]
    fn test_counter_vec_validation() {
        let labels = Labels::new(vec![Label::with_values("method", vec!["GET", "POST"])]);
        let vec = CounterVec::new(MetricOpts::new("requests"), labels);

        assert!(vec.with_label_values(&["GET"]).is_ok());
        assert!(vec.with_label_values(&["DELETE"]).is_err());
        assert!(vec.with_label_values(&["GET", "POST"]).is_err());
    }

    #[test]
    fn test_counter_vec_delete() {
        let labels = Labels::new(vec![Label::unconstrained("method")]);
        let vec = CounterVec::new(MetricOpts::new("requests"), labels);

        let counter = vec.with_label_values(&["GET"]).unwrap();
        counter.add(10);

        assert!(vec.delete_label_values(&["GET"]));
        assert_eq!(vec.all().len(), 0);
    }

    // ========== Gauge Tests ==========

    #[test]
    fn test_gauge_new() {
        let gauge = Gauge::new(MetricOpts::new("test_gauge"));
        assert!(gauge.is_enabled());
        assert_eq!(gauge.get(), 0);
    }

    #[test]
    fn test_gauge_set() {
        let gauge = Gauge::new(MetricOpts::new("test_gauge"));
        gauge.set(42);
        assert_eq!(gauge.get(), 42);
        gauge.set(100);
        assert_eq!(gauge.get(), 100);
    }

    #[test]
    fn test_gauge_inc_dec() {
        let gauge = Gauge::new(MetricOpts::new("test_gauge"));
        gauge.set(10);
        gauge.inc();
        assert_eq!(gauge.get(), 11);
        gauge.dec();
        assert_eq!(gauge.get(), 10);
    }

    #[test]
    fn test_gauge_add_sub() {
        let gauge = Gauge::new(MetricOpts::new("test_gauge"));
        gauge.add(5);
        assert_eq!(gauge.get(), 5);
        gauge.sub(2);
        assert_eq!(gauge.get(), 3);
    }

    // ========== GaugeVec Tests ==========

    #[test]
    fn test_gauge_vec_with_label_values() {
        let labels = Labels::new(vec![Label::unconstrained("pod")]);
        let vec = GaugeVec::new(MetricOpts::new("pod_memory"), labels);

        let gauge1 = vec.with_label_values(&["pod1"]).unwrap();
        let gauge2 = vec.with_label_values(&["pod2"]).unwrap();

        gauge1.set(1024);
        gauge2.set(2048);

        assert_eq!(gauge1.get(), 1024);
        assert_eq!(gauge2.get(), 2048);
    }

    // ========== Histogram Tests ==========

    #[test]
    fn test_histogram_new() {
        let hist = Histogram::new(
            MetricOpts::new("test_histogram"),
            vec![0.1, 0.5, 1.0, 5.0, 10.0],
        );
        assert!(hist.is_enabled());
        assert_eq!(hist.count(), 0);
    }

    #[test]
    fn test_histogram_observe() {
        let hist = Histogram::new(
            MetricOpts::new("test_histogram"),
            vec![0.1, 0.5, 1.0, 5.0, 10.0],
        );

        hist.observe(0.05);
        hist.observe(0.3);
        hist.observe(2.0);

        assert_eq!(hist.count(), 3);
    }

    #[test]
    fn test_histogram_reset() {
        let hist = Histogram::new(
            MetricOpts::new("test_histogram"),
            vec![0.1, 0.5, 1.0, 5.0, 10.0],
        );

        hist.observe(0.5);
        hist.reset();
        assert_eq!(hist.count(), 0);
    }

    // ========== HistogramVec Tests ==========

    #[test]
    fn test_histogram_vec_with_label_values() {
        let labels = Labels::new(vec![Label::unconstrained("endpoint")]);
        let vec = HistogramVec::new(MetricOpts::new("latency"), labels, vec![0.1, 0.5, 1.0, 5.0]);

        let hist1 = vec.with_label_values(&["ep1"]).unwrap();
        let hist2 = vec.with_label_values(&["ep2"]).unwrap();

        hist1.observe(0.3);
        hist2.observe(2.0);

        assert_eq!(hist1.count(), 1);
        assert_eq!(hist2.count(), 1);
    }

    // ========== Monitor Tests ==========

    #[test]
    fn test_message_type_to_from_u8() {
        assert_eq!(monitor::MessageType::Drop.to_u8(), 1);
        assert_eq!(
            monitor::MessageType::from_u8(1),
            Some(monitor::MessageType::Drop)
        );
        assert_eq!(monitor::MessageType::from_u8(u8::MAX), None);
    }

    #[test]
    fn test_message_type_as_str() {
        assert_eq!(monitor::MessageType::Drop.as_str(), "drop");
        assert_eq!(monitor::MessageType::Agent.as_str(), "agent");
        assert_eq!(monitor::MessageType::Trace.as_str(), "trace");
    }

    #[test]
    fn test_message_type_filter_add() {
        let filter = monitor::MessageTypeFilter::new()
            .add(monitor::MessageType::Drop)
            .add(monitor::MessageType::Trace);

        assert_eq!(filter.len(), 2);
        assert!(filter.contains(monitor::MessageType::Drop));
        assert!(filter.contains(monitor::MessageType::Trace));
        assert!(!filter.contains(monitor::MessageType::Debug));
    }

    #[test]
    fn test_message_type_filter_empty_matches_all() {
        let filter = monitor::MessageTypeFilter::new();
        assert!(filter.contains(monitor::MessageType::Drop));
        assert!(filter.contains(monitor::MessageType::Agent));
    }

    #[test]
    fn test_drop_reason_conversions() {
        assert_eq!(monitor::DropReason::PolicyDenied.to_u32(), 4);
        assert_eq!(
            monitor::DropReason::from_u32(4),
            monitor::DropReason::PolicyDenied
        );
        assert_eq!(
            monitor::DropReason::from_u32(u32::MAX),
            monitor::DropReason::Unknown
        );
    }

    #[test]
    fn test_ring_buffer_event_new() {
        let event = RingBufferEvent::new(monitor::MESSAGE_TYPE_DROP, vec![1, 2, 3, 4]);
        assert_eq!(event.message_type, monitor::MESSAGE_TYPE_DROP);
        assert_eq!(event.data, vec![1, 2, 3, 4]);
        assert!(event.timestamp > 0);
    }

    // ========== Edge cases ==========

    #[test]
    fn test_counter_vec_all() {
        let labels = Labels::new(vec![Label::unconstrained("label")]);
        let vec = CounterVec::new(MetricOpts::new("test"), labels);

        vec.with_label_values(&["a"]).unwrap().add(10);
        vec.with_label_values(&["b"]).unwrap().add(20);

        let all = vec.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_metric_opts_with_multiple_builders() {
        let opts = MetricOpts::new("metric")
            .with_namespace("ns")
            .with_subsystem("sub")
            .with_help("help text")
            .with_config_name("custom_config");

        assert_eq!(opts.fq_name(), "ns_sub_metric");
        assert_eq!(opts.get_config_name(), "custom_config");
        assert_eq!(opts.help, "help text");
    }

    #[test]
    fn test_gauge_vec_delete() {
        let labels = Labels::new(vec![Label::unconstrained("id")]);
        let vec = GaugeVec::new(MetricOpts::new("gauge"), labels);

        let g = vec.with_label_values(&["id1"]).unwrap();
        g.set(100);

        assert!(vec.delete_label_values(&["id1"]));
        assert_eq!(vec.all().len(), 0);
    }

    #[test]
    fn test_labels_validate_map() {
        let labels = Labels::new(vec![
            Label::with_values("method", vec!["GET", "POST"]),
            Label::with_values("status", vec!["200", "404"]),
        ]);

        let mut map = HashMap::new();
        map.insert("method".to_string(), "GET".to_string());
        map.insert("status".to_string(), "200".to_string());

        assert!(labels.validate_map(&map).is_ok());

        map.insert("method".to_string(), "DELETE".to_string());
        assert!(labels.validate_map(&map).is_err());
    }
}

#[cfg(test)]
mod registry_tests {
    use super::names;
    use super::registry::{Counter, Gauge, Histogram, MetricDesc, Registry};

    #[test]
    fn test_counter_inc_add() {
        let counter = Counter::new(MetricDesc::new("hits", "Total hits"));
        counter.inc();
        counter.inc();
        counter.add(3);
        assert_eq!(counter.get(), 5);
    }

    #[test]
    fn test_gauge_set_inc_dec() {
        let gauge = Gauge::new(MetricDesc::new("conn", "Active connections"));
        gauge.set(10);
        gauge.inc();
        gauge.dec();
        gauge.dec();
        assert_eq!(gauge.get(), 9);
        gauge.add(-4);
        assert_eq!(gauge.get(), 5);
    }

    #[test]
    fn test_histogram_observe() {
        let histogram = Histogram::new(
            MetricDesc::new("latency", "Request latency"),
            vec![0.1, 0.5, 1.0],
        );
        histogram.observe(0.05);
        histogram.observe(0.2);
        histogram.observe(0.8);
        assert_eq!(histogram.count(), 3);
        let buckets = histogram.bucket_counts();
        assert_eq!(buckets[0], (0.1, 1));
        assert_eq!(buckets[1], (0.5, 2));
        assert_eq!(buckets[2], (1.0, 3));
    }

    #[test]
    fn test_registry_register_and_snapshot() {
        let mut registry = Registry::new();
        let counter = registry
            .register_counter(MetricDesc::new(names::ENDPOINT_COUNT, "Endpoints"))
            .unwrap();
        let gauge = registry
            .register_gauge(MetricDesc::new(names::IDENTITY_COUNT, "Identities"))
            .unwrap();
        counter.add(5);
        gauge.set(3);
        let snapshot = registry.snapshot();
        assert_eq!(snapshot[names::ENDPOINT_COUNT], 5);
        assert_eq!(snapshot[names::IDENTITY_COUNT], 3);
    }

    #[test]
    fn test_registry_duplicate_registration_error() {
        let mut registry = Registry::new();
        registry
            .register_counter(MetricDesc::new("foo", "bar"))
            .unwrap();
        assert!(
            registry
                .register_counter(MetricDesc::new("foo", "bar"))
                .is_err()
        );
        assert!(
            registry
                .register_gauge(MetricDesc::new("foo", "bar"))
                .is_err()
        );
    }

    #[test]
    fn test_counter_is_clone_shared() {
        let counter = Counter::new(MetricDesc::new("x", "x"));
        let counter_clone = counter.clone();
        counter.add(7);
        assert_eq!(counter_clone.get(), 7);
    }
}
