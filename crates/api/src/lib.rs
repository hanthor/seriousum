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

pub use errors::{ApiError, ApiResult};
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
