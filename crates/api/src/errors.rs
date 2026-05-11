//! Error types and handling for the REST API.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// REST API error types.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("internal server error: {0}")]
    InternalError(String),

    #[error("unavailable: {0}")]
    ServiceUnavailable(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ApiError {
    /// Get the HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::InvalidInput(_) | Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::InternalError(_) | Self::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Get the error message.
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// Response envelope for API errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// HTTP status code.
    pub code: u16,
    /// Error message.
    pub message: String,
    /// Optional error type/category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    /// Optional additional details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ErrorResponse {
    /// Create a new error response.
    pub fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            error_type: None,
            details: None,
        }
    }

    /// Set the error type.
    pub fn with_error_type(mut self, error_type: impl Into<String>) -> Self {
        self.error_type = Some(error_type.into());
        self
    }

    /// Set additional details.
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let message = self.message();

        let error_response = ErrorResponse::new(status.as_u16(), message);

        (status, Json(error_response)).into_response()
    }
}

/// Result type alias for API operations.
pub type ApiResult<T> = Result<T, ApiError>;

// Conversion from core error types
impl From<seriousum_core::Error> for ApiError {
    fn from(err: seriousum_core::Error) -> Self {
        Self::InternalError(err.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        Self::BadRequest(format!("JSON parse error: {}", err))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::InternalError(format!("IO error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_not_found_maps_to_404() {
        let err = ApiError::NotFound("user".to_string());
        assert_eq!(err.status_code(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn api_error_invalid_input_maps_to_400() {
        let err = ApiError::InvalidInput("bad format".to_string());
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn api_error_unauthorized_maps_to_401() {
        let err = ApiError::Unauthorized("missing token".to_string());
        assert_eq!(err.status_code(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn api_error_forbidden_maps_to_403() {
        let err = ApiError::Forbidden("insufficient permissions".to_string());
        assert_eq!(err.status_code(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn api_error_conflict_maps_to_409() {
        let err = ApiError::Conflict("resource already exists".to_string());
        assert_eq!(err.status_code(), StatusCode::CONFLICT);
    }

    #[test]
    fn api_error_service_unavailable_maps_to_503() {
        let err = ApiError::ServiceUnavailable("database down".to_string());
        assert_eq!(err.status_code(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn error_response_serializes() {
        let response = ErrorResponse::new(400, "bad request")
            .with_error_type("ValidationError")
            .with_details("field 'name' is required");

        let json = serde_json::to_string(&response).expect("serializes");
        let decoded: ErrorResponse = serde_json::from_str(&json).expect("deserializes");

        assert_eq!(decoded.code, 400);
        assert_eq!(decoded.message, "bad request");
        assert_eq!(decoded.error_type.as_deref(), Some("ValidationError"));
    }

    #[test]
    fn error_response_omits_optional_fields() {
        let response = ErrorResponse::new(500, "server error");
        let json = serde_json::to_string(&response).expect("serializes");

        assert!(!json.contains("error_type"));
        assert!(!json.contains("details"));
    }
}
