//! REST API server and control-plane contract types for seriousum.
//!
//! This crate provides:
//! - Control-plane request/response envelopes with metadata
//! - Health status tracking and reporting
//! - REST API server implementation with axum
//! - Agent control endpoints (healthz, config, cluster/nodes)
//! - Endpoint management endpoints (list, get, create, update, delete)
//! - Authentication middleware and error handling
//! - OpenAPI/Swagger spec generation

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

pub use seriousum_core::{Error as CoreError, Result as CoreResult, VERSION as CORE_VERSION};

// ============================================================================
// Module definitions
// ============================================================================

pub mod errors;
pub mod handlers;
pub mod middleware;
pub mod server;
pub mod types;

// ============================================================================
// Re-exports
// ============================================================================

pub use errors::{ApiError as HandlerApiError, ApiResult};
pub use server::Server;
pub use types::*;

// ============================================================================
// Contract version
// ============================================================================

/// The current control-plane contract version.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// Core types (previously in this file)
// ============================================================================

/// A compact version descriptor shared across control-plane messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// The contract crate version.
    pub contract: String,

    /// The linked `seriousum-core` version.
    pub core: String,
}

impl VersionInfo {
    /// Returns the current version information.
    #[must_use]
    pub fn current() -> Self {
        Self {
            contract: CONTRACT_VERSION.to_owned(),
            core: CORE_VERSION.to_owned(),
        }
    }
}

/// Metadata attached to control-plane messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Contract and runtime version information.
    pub version: VersionInfo,

    /// The originating component name.
    pub component: String,

    /// Optional correlation or trace identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

impl MessageMetadata {
    /// Builds metadata for a component using the current versions.
    #[must_use]
    pub fn new(component: impl Into<String>) -> Self {
        Self {
            version: VersionInfo::current(),
            component: component.into(),
            trace_id: None,
        }
    }

    /// Adds a trace identifier to the metadata.
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

/// A reusable request envelope for control-plane messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request<T> {
    /// Request correlation identifier.
    pub id: String,

    /// Message metadata.
    pub metadata: MessageMetadata,

    /// Request payload.
    pub payload: T,
}

impl<T> Request<T> {
    /// Creates a new request envelope.
    #[must_use]
    pub fn new(id: impl Into<String>, component: impl Into<String>, payload: T) -> Self {
        Self {
            id: id.into(),
            metadata: MessageMetadata::new(component),
            payload,
        }
    }

    /// Adds a trace identifier to the request metadata.
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.metadata = self.metadata.with_trace_id(trace_id);
        self
    }
}

/// A reusable response envelope for control-plane messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response<T> {
    /// Request correlation identifier.
    pub id: String,

    /// Message metadata.
    pub metadata: MessageMetadata,

    /// Optional payload on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<T>,

    /// Optional error message on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> Response<T> {
    /// Creates a successful response envelope.
    #[must_use]
    pub fn ok(id: impl Into<String>, component: impl Into<String>, payload: T) -> Self {
        Self {
            id: id.into(),
            metadata: MessageMetadata::new(component),
            payload: Some(payload),
            error: None,
        }
    }

    /// Creates a failed response envelope.
    #[must_use]
    pub fn err(
        id: impl Into<String>,
        component: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            metadata: MessageMetadata::new(component),
            payload: None,
            error: Some(error.into()),
        }
    }

    /// Converts a `CoreResult` into a response envelope.
    #[must_use]
    pub fn from_result(
        id: impl Into<String>,
        component: impl Into<String>,
        result: CoreResult<T>,
    ) -> Self {
        match result {
            Ok(payload) => Self::ok(id, component, payload),
            Err(error) => Self::err(id, component, error.to_string()),
        }
    }

    /// Adds a trace identifier to the response metadata.
    #[must_use]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.metadata = self.metadata.with_trace_id(trace_id);
        self
    }
}

/// Health information for a control-plane component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Health has not been checked yet.
    Unknown,
    /// The component is healthy.
    Healthy,
    /// The component is partially degraded.
    Degraded,
    /// The component is unhealthy.
    Unhealthy,
}

/// A small health report suitable for API responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthReport {
    /// Current health status.
    pub status: HealthStatus,

    /// Optional human-readable details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Version metadata for the reporting component.
    pub version: VersionInfo,
}

impl HealthReport {
    /// Builds a healthy report.
    #[must_use]
    pub fn healthy(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Healthy,
            message: Some(message.into()),
            version: VersionInfo::current(),
        }
    }

    /// Builds an unhealthy report.
    #[must_use]
    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Unhealthy,
            message: Some(message.into()),
            version: VersionInfo::current(),
        }
    }
}

/// Endpoint metadata parsed from API specification paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiPathEndpoint {
    /// HTTP method.
    pub method: String,
    /// HTTP path.
    pub path: String,
    /// Endpoint description.
    pub description: String,
}

/// Canonical API flag name to endpoint mapping.
pub type PathSet = BTreeMap<String, ApiPathEndpoint>;

/// API configuration parsing errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ApiConfigError {
    /// Unsupported wildcard syntax in an allowed API flag.
    #[error("Unsupported API wildcard")]
    UnknownWildcard,
    /// Unknown API flag.
    #[error("Unknown API flag")]
    UnknownFlag,
}

/// Minimal API path item used for parity with Cilium's `spec.PathItem`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecPathItem {
    /// DELETE operation description.
    pub delete: Option<String>,
    /// GET operation description.
    pub get: Option<String>,
    /// PATCH operation description.
    pub patch: Option<String>,
    /// POST operation description.
    pub post: Option<String>,
    /// PUT operation description.
    pub put: Option<String>,
}

fn lock_mutex<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

fn read_lock<'a, T>(lock: &'a RwLock<T>) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

fn write_lock<'a, T>(lock: &'a RwLock<T>) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(err) => err.into_inner(),
    }
}

fn pascalize(value: &str) -> String {
    match value {
        "bgp" => "BGP".to_owned(),
        "id" => "ID".to_owned(),
        "ip" => "IP".to_owned(),
        "ipam" => "IPAM".to_owned(),
        "lrp" => "LRP".to_owned(),
        _ => {
            let mut chars = value.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!(
                "{}{}",
                first.to_uppercase(),
                chars.as_str().to_ascii_lowercase()
            )
        }
    }
}

fn path_to_flag_suffix(path: &str) -> String {
    path.trim_start_matches('/')
        .split('/')
        .flat_map(|segment| segment.split('-'))
        .map(|word| word.trim_matches(|c| c == '{' || c == '}'))
        .map(pascalize)
        .collect::<String>()
}

/// Parses API specification paths into canonical Cilium API flags.
#[must_use]
pub fn parse_spec_paths(paths: &BTreeMap<String, SpecPathItem>) -> PathSet {
    let mut results = PathSet::new();
    for (path, item) in paths {
        let suffix = path_to_flag_suffix(path);
        let ops = [
            ("Delete", "DELETE", item.delete.as_deref()),
            ("Get", "GET", item.get.as_deref()),
            ("Patch", "PATCH", item.patch.as_deref()),
            ("Post", "POST", item.post.as_deref()),
            ("Put", "PUT", item.put.as_deref()),
        ];
        for (flag_prefix, method, description) in ops {
            if description.is_some() {
                let flag = format!("{flag_prefix}{suffix}");
                results.insert(
                    flag,
                    ApiPathEndpoint {
                        method: method.to_owned(),
                        path: path.clone(),
                        description: description.unwrap_or_default().to_owned(),
                    },
                );
            }
        }
    }
    results
}

/// Generates denied API endpoint flags from all paths and allowed flags.
pub fn generate_denied_api_endpoints(
    all_paths: &PathSet,
    allowed: &[String],
) -> Result<PathSet, ApiConfigError> {
    let mut denied = all_paths.clone();
    let mut wildcard_prefixes = Vec::new();
    for opt in allowed {
        match opt.find('*') {
            None => {}
            Some(index) if index == opt.len() - 1 => {
                let prefix = opt.trim_end_matches('*');
                if prefix.is_empty() {
                    return Ok(PathSet::new());
                }
                wildcard_prefixes.push(prefix.to_owned());
                continue;
            }
            Some(_) => return Err(ApiConfigError::UnknownWildcard),
        }
        if denied.remove(opt).is_none() {
            return Err(ApiConfigError::UnknownFlag);
        }
    }
    for prefix in wildcard_prefixes {
        denied.retain(|flag, _| !flag.starts_with(&prefix));
    }
    Ok(denied)
}

/// Parses API paths and returns endpoints that should be denied.
pub fn allowed_flags_to_denied_paths(
    paths: &BTreeMap<String, SpecPathItem>,
    allowed: &[String],
) -> Result<PathSet, ApiConfigError> {
    generate_denied_api_endpoints(&parse_spec_paths(paths), allowed)
}

/// Metrics API observed by the API limiter.
pub trait MetricsApi: Send + Sync {
    /// Observe an API operation's rate-limit wait duration.
    fn observe_rate_limit(&self, operation: &str, duration: Duration);
}

#[derive(Debug)]
struct ApiLimiterState {
    tokens: f64,
    last: Instant,
}

/// Token-bucket API limiter.
pub struct ApiLimiter<M: MetricsApi> {
    metrics: Arc<M>,
    rate_limit: f64,
    burst: f64,
    state: Mutex<ApiLimiterState>,
}

impl<M: MetricsApi> ApiLimiter<M> {
    /// Creates a new API limiter with rate and burst settings.
    #[must_use]
    pub fn new(metrics: Arc<M>, rate_limit: f64, burst: usize) -> Self {
        let safe_rate_limit = if rate_limit.is_finite() && rate_limit > 0.0 {
            rate_limit
        } else {
            f64::EPSILON
        };
        Self {
            metrics,
            rate_limit: safe_rate_limit,
            burst: burst as f64,
            state: Mutex::new(ApiLimiterState {
                tokens: burst as f64,
                last: Instant::now(),
            }),
        }
    }

    fn reserve_delay(&self) -> Duration {
        let mut state = lock_mutex(&self.state);
        let now = Instant::now();
        let elapsed = now.duration_since(state.last).as_secs_f64();
        state.tokens = (state.tokens + elapsed * self.rate_limit).min(self.burst);
        state.last = now;
        state.tokens -= 1.0;
        if state.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64((-state.tokens) / self.rate_limit)
        }
    }

    /// Applies rate limiting for an operation, waiting as needed.
    pub fn limit(&self, operation: &str) {
        self.limit_with_cancel(operation, false);
    }

    /// Applies rate limiting and optionally simulates a canceled wait context.
    pub fn limit_with_cancel(&self, operation: &str, cancel_wait: bool) {
        let delay = self.reserve_delay();
        if delay.is_zero() {
            return;
        }
        self.metrics.observe_rate_limit(operation, delay);
        if cancel_wait {
            let mut state = lock_mutex(&self.state);
            state.tokens += 1.0;
            return;
        }
        thread::sleep(delay);
    }
}

/// API operation identifier type.
pub type Operation = String;

/// Simulates fixed delays for API operations.
pub struct DelaySimulator<Op = Operation>
where
    Op: Eq + Hash + Clone,
{
    delays: RwLock<HashMap<Op, Duration>>,
}

impl<Op> DelaySimulator<Op>
where
    Op: Eq + Hash + Clone,
{
    /// Creates a new delay simulator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            delays: RwLock::new(HashMap::new()),
        }
    }

    /// Sets simulated delay for one operation. Zero removes the delay.
    pub fn set_delay(&self, op: Op, delay: Duration) {
        let mut delays = write_lock(&self.delays);
        if delay.is_zero() {
            delays.remove(&op);
        } else {
            delays.insert(op, delay);
        }
    }

    /// Returns configured delay for an operation, if any.
    #[must_use]
    pub fn configured_delay(&self, op: &Op) -> Option<Duration> {
        read_lock(&self.delays).get(op).copied()
    }

    /// Sleeps according to configured delay for an operation.
    pub fn delay(&self, op: &Op) {
        if let Some(delay) = self.configured_delay(op) {
            thread::sleep(delay);
        }
    }
}

impl<Op> Default for DelaySimulator<Op>
where
    Op: Eq + Hash + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Mock API metrics implementation used by tests.
pub struct MockMetrics {
    api_call: RwLock<HashMap<String, f64>>,
    rate_limit: RwLock<HashMap<String, Duration>>,
}

impl MockMetrics {
    /// Creates a new mock metrics backend.
    #[must_use]
    pub fn new() -> Self {
        Self {
            api_call: RwLock::new(HashMap::new()),
            rate_limit: RwLock::new(HashMap::new()),
        }
    }

    /// Returns aggregated API-call duration for an operation and status.
    #[must_use]
    pub fn api_call(&self, operation: &str, status: &str) -> f64 {
        let key = format!("operation={operation}, status={status}");
        read_lock(&self.api_call)
            .get(&key)
            .copied()
            .unwrap_or_default()
    }

    /// Records API-call duration.
    pub fn observe_api_call(&self, operation: &str, status: &str, duration: f64) {
        let key = format!("operation={operation}, status={status}");
        let mut api_call = write_lock(&self.api_call);
        let entry = api_call.entry(key).or_default();
        *entry += duration;
    }

    /// Returns aggregated rate-limit wait duration for an operation.
    #[must_use]
    pub fn rate_limit(&self, operation: &str) -> Duration {
        read_lock(&self.rate_limit)
            .get(operation)
            .copied()
            .unwrap_or_default()
    }

    /// Records rate-limit wait duration.
    pub fn observe_rate_limit(&self, operation: &str, duration: Duration) {
        let mut rate_limit = write_lock(&self.rate_limit);
        let entry = rate_limit.entry(operation.to_owned()).or_default();
        *entry += duration;
    }
}

impl Default for MockMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsApi for MockMetrics {
    fn observe_rate_limit(&self, operation: &str, duration: Duration) {
        MockMetrics::observe_rate_limit(self, operation, duration);
    }
}

#[cfg(test)]
mod parity_tests {
    use super::{
        ApiConfigError, ApiLimiter, DelaySimulator, MockMetrics, SpecPathItem,
        allowed_flags_to_denied_paths, parse_spec_paths,
    };
    use std::{
        collections::BTreeMap,
        sync::Arc,
        time::{Duration, Instant},
    };

    #[test]
    fn parity_test_parse_spec_paths() {
        let mut paths = BTreeMap::new();
        paths.insert(
            "/bgp".to_owned(),
            SpecPathItem {
                get: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/endpoint/{id}".to_owned(),
            SpecPathItem {
                put: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/lrp/{id}/foo".to_owned(),
            SpecPathItem {
                delete: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/cgroup-metadata-dump".to_owned(),
            SpecPathItem {
                post: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/endpoint/{id}/config".to_owned(),
            SpecPathItem {
                patch: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/ipam/{ip}".to_owned(),
            SpecPathItem {
                put: Some(String::new()),
                patch: Some(String::new()),
                ..Default::default()
            },
        );

        let got = parse_spec_paths(&paths);
        assert_eq!(got["GetBGP"].method, "GET");
        assert_eq!(got["GetBGP"].path, "/bgp");
        assert_eq!(got["PutEndpointID"].method, "PUT");
        assert_eq!(got["PatchEndpointIDConfig"].path, "/endpoint/{id}/config");
        assert_eq!(got["DeleteLRPIDFoo"].method, "DELETE");
        assert_eq!(got["PostCgroupMetadataDump"].path, "/cgroup-metadata-dump");
        assert_eq!(got["PatchIPAMIP"].method, "PATCH");
        assert_eq!(got["PutIPAMIP"].method, "PUT");
    }

    #[test]
    fn parity_test_allowed_flags_to_denied_paths() {
        let mut paths = BTreeMap::new();
        paths.insert(
            "/endpoint".to_owned(),
            SpecPathItem {
                get: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/endpoint/{id}".to_owned(),
            SpecPathItem {
                put: Some(String::new()),
                ..Default::default()
            },
        );
        paths.insert(
            "/endpoint/{id}/config".to_owned(),
            SpecPathItem {
                patch: Some(String::new()),
                ..Default::default()
            },
        );

        let deny_all = allowed_flags_to_denied_paths(&paths, &[]).expect("deny-all works");
        assert_eq!(deny_all.len(), 3);

        let allow_all =
            allowed_flags_to_denied_paths(&paths, &["*".to_owned()]).expect("allow-all works");
        assert!(allow_all.is_empty());

        let allow_gets = allowed_flags_to_denied_paths(&paths, &["Get*".to_owned()])
            .expect("wildcard allow works");
        assert_eq!(allow_gets.len(), 2);
        assert!(!allow_gets.contains_key("GetEndpoint"));

        let invalid_flag = allowed_flags_to_denied_paths(&paths, &["NoSuchOption".to_owned()]);
        assert_eq!(invalid_flag, Err(ApiConfigError::UnknownFlag));

        let invalid_empty = allowed_flags_to_denied_paths(&paths, &[String::new()]);
        assert_eq!(invalid_empty, Err(ApiConfigError::UnknownFlag));

        let invalid_prefix = allowed_flags_to_denied_paths(&paths, &["*foo".to_owned()]);
        assert_eq!(invalid_prefix, Err(ApiConfigError::UnknownWildcard));

        let invalid_multi = allowed_flags_to_denied_paths(&paths, &["foo*bar*".to_owned()]);
        assert_eq!(invalid_multi, Err(ApiConfigError::UnknownWildcard));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestOperation {
        One,
        Two,
    }

    #[test]
    fn parity_test_set_delay() {
        let delay_simulator = DelaySimulator::<TestOperation>::new();

        delay_simulator.set_delay(TestOperation::One, Duration::from_secs(1));

        assert_eq!(
            delay_simulator.configured_delay(&TestOperation::One),
            Some(Duration::from_secs(1))
        );
        assert_eq!(delay_simulator.configured_delay(&TestOperation::Two), None);
    }

    #[test]
    fn parity_test_rate_limit_burst() {
        let metrics = Arc::new(MockMetrics::new());
        let limiter = ApiLimiter::new(metrics.clone(), 1.0, 10);

        for _ in 0..10 {
            limiter.limit("test");
        }
        assert_eq!(metrics.rate_limit("test"), Duration::ZERO);

        limiter.limit_with_cancel("test", true);
        assert_ne!(metrics.rate_limit("test"), Duration::ZERO);
    }

    #[test]
    fn parity_test_rate_limit_wait() {
        let metrics = Arc::new(MockMetrics::new());
        let limiter = ApiLimiter::new(metrics.clone(), 100.0, 1);

        limiter.limit("test");
        assert_eq!(metrics.rate_limit("test"), Duration::ZERO);

        let start = Instant::now();
        for _ in 0..15 {
            limiter.limit("test");
        }
        let measured = start.elapsed();
        let accounted = metrics.rate_limit("test");
        assert!(
            measured <= accounted * 2,
            "waited longer than expected (expected {accounted:?} (+/-100%), measured {measured:?})"
        );
    }

    #[test]
    fn parity_test_mock() {
        let api = MockMetrics::new();
        api.observe_api_call("DescribeNetworkInterfaces", "success", 2.0);
        assert!((api.api_call("DescribeNetworkInterfaces", "success") - 2.0).abs() < f64::EPSILON);

        api.observe_rate_limit("DescribeNetworkInterfaces", Duration::from_secs(1));
        api.observe_rate_limit("DescribeNetworkInterfaces", Duration::from_secs(1));
        assert_eq!(
            api.rate_limit("DescribeNetworkInterfaces"),
            Duration::from_secs(2)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Ping {
        value: String,
    }

    #[test]
    fn request_round_trips_through_json() {
        let request = Request::new(
            "req-1",
            "operator",
            Ping {
                value: "hello".to_owned(),
            },
        )
        .with_trace_id("trace-7");

        let json = serde_json::to_string(&request).expect("request serializes");
        let decoded: Request<Ping> = serde_json::from_str(&json).expect("request deserializes");

        assert_eq!(decoded.id, "req-1");
        assert_eq!(decoded.metadata.component, "operator");
        assert_eq!(decoded.metadata.trace_id.as_deref(), Some("trace-7"));
        assert_eq!(decoded.payload.value, "hello");
    }

    #[test]
    fn response_from_result_maps_core_error() {
        let response = Response::<Ping>::from_result(
            "req-2",
            "cli",
            Err(CoreError::Api("missing route".to_owned())),
        );

        assert_eq!(response.id, "req-2");
        assert!(response.payload.is_none());
        assert_eq!(response.error.as_deref(), Some("API error: missing route"));
    }

    #[test]
    fn health_report_uses_current_version() {
        let report = HealthReport::healthy("ready");

        assert_eq!(report.status, HealthStatus::Healthy);
        assert_eq!(report.message.as_deref(), Some("ready"));
        assert_eq!(report.version.contract, CONTRACT_VERSION);
        assert_eq!(report.version.core, CORE_VERSION);
    }
}
