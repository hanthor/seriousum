//! Middleware for authentication and other cross-cutting concerns.

use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use tracing::debug;

/// Authentication provider trait.
pub trait AuthProvider: Send + Sync {
    /// Validate an authorization token.
    /// Returns Ok(true) if valid, Ok(false) if invalid, Err if there's an error.
    fn validate_token(&self, token: &str) -> Result<bool, String>;
}

/// Simple bearer token authentication provider.
pub struct BearerTokenAuth {
    /// Valid tokens
    tokens: Arc<Vec<String>>,
}

impl BearerTokenAuth {
    /// Create a new bearer token auth provider.
    pub fn new(tokens: Vec<String>) -> Self {
        Self {
            tokens: Arc::new(tokens),
        }
    }

    /// Check if a token is valid.
    pub fn is_valid(&self, token: &str) -> bool {
        self.tokens.iter().any(|t| t == token)
    }
}

impl AuthProvider for BearerTokenAuth {
    fn validate_token(&self, token: &str) -> Result<bool, String> {
        Ok(self.is_valid(token))
    }
}

/// No-op authentication provider (always allows).
pub struct NoOpAuth;

impl AuthProvider for NoOpAuth {
    fn validate_token(&self, _token: &str) -> Result<bool, String> {
        Ok(true)
    }
}

/// Extract bearer token from Authorization header.
pub fn extract_bearer_token(auth_header: &str) -> Option<String> {
    if auth_header.starts_with("Bearer ") {
        Some(auth_header[7..].to_string())
    } else {
        None
    }
}

/// Middleware for authentication.
pub async fn auth_middleware(
    request: Request,
    next: Next,
) -> Result<Response, String> {
    // Get authorization header
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    debug!("auth middleware: header present: {}", !auth_header.is_empty());

    // For now, just pass through. In a real implementation, we'd validate the token here.
    Ok(next.run(request).await)
}

/// Middleware for request logging.
pub async fn logging_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();

    debug!("request: {} {}", method, uri);

    let response = next.run(request).await;

    let status = response.status();
    debug!("response: {} {} -> {}", method, uri, status);

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_auth_validates_token() {
        let auth = BearerTokenAuth::new(vec!["token-1".to_string(), "token-2".to_string()]);

        assert!(auth.is_valid("token-1"));
        assert!(auth.is_valid("token-2"));
        assert!(!auth.is_valid("token-3"));
    }

    #[test]
    fn bearer_token_auth_via_trait() {
        let auth: Arc<dyn AuthProvider> = Arc::new(BearerTokenAuth::new(vec!["test".to_string()]));

        let result = auth.validate_token("test");
        assert_eq!(result, Ok(true));

        let result = auth.validate_token("invalid");
        assert_eq!(result, Ok(false));
    }

    #[test]
    fn noop_auth_always_validates() {
        let auth = NoOpAuth;

        assert_eq!(auth.validate_token("anything"), Ok(true));
        assert_eq!(auth.validate_token(""), Ok(true));
    }

    #[test]
    fn extract_bearer_token_parses_valid_header() {
        let token = extract_bearer_token("Bearer my-secret-token");
        assert_eq!(token.as_deref(), Some("my-secret-token"));
    }

    #[test]
    fn extract_bearer_token_returns_none_for_invalid_header() {
        assert_eq!(extract_bearer_token("Basic xyz"), None);
        assert_eq!(extract_bearer_token(""), None);
        assert_eq!(extract_bearer_token("Bearer"), None);
    }
}
