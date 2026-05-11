//! Lightweight authentication scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{
    Error, Identity, Result, SecurityIdentity, SecurityLabel,
    chrono::{DateTime, Utc},
};

/// Default component name for authentication scaffolds.
pub const COMPONENT: &str = "seriousum-auth";

/// Authentication operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// Local/static authentication for simple harnesses.
    Local,
    /// Remote/OIDC-style authentication.
    Remote,
    /// Authentication disabled.
    Disabled,
}

/// Authentication lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    /// Authentication has been prepared but not yet used.
    Pending,
    /// Authentication is active.
    Ready,
    /// Authentication is paused or disabled.
    Suspended,
}

/// Compact authentication configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Issuer name for minted tokens.
    pub issuer: String,

    /// Audience name expected by consumers.
    pub audience: String,

    /// Token time-to-live in seconds.
    pub token_ttl_secs: u64,

    /// Selected authentication mode.
    pub mode: AuthMode,
}

impl AuthConfig {
    /// Creates a new authentication configuration.
    #[must_use]
    pub fn new(issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            token_ttl_secs: 3_600,
            mode: AuthMode::Local,
        }
    }

    /// Returns the default scaffold configuration.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("seriousum-auth", "seriousum")
    }

    /// Returns true when authentication is enabled.
    #[must_use]
    pub fn enabled(&self) -> bool {
        !matches!(self.mode, AuthMode::Disabled)
    }

    /// Validates the authentication configuration.
    pub fn validate(&self) -> Result<()> {
        if self.issuer.trim().is_empty() {
            return Err(Error::Auth(String::from("auth issuer must not be empty")));
        }

        if self.audience.trim().is_empty() {
            return Err(Error::Auth(String::from("auth audience must not be empty")));
        }

        if self.token_ttl_secs == 0 {
            return Err(Error::Auth(String::from(
                "auth token ttl must be greater than zero",
            )));
        }

        Ok(())
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Authentication session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    /// The authenticated subject.
    pub subject: Identity,

    /// Scopes granted to the session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,

    /// When the session was issued.
    pub issued_at: DateTime<Utc>,

    /// Whether the session is active.
    pub active: bool,
}

impl AuthSession {
    /// Creates a new session for the supplied subject.
    #[must_use]
    pub fn new(subject: Identity) -> Self {
        Self {
            subject,
            scopes: Vec::new(),
            issued_at: Utc::now(),
            active: true,
        }
    }

    /// Returns the default scaffold session.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(Identity::new(
            SecurityIdentity::world(),
            [SecurityLabel::new("auth", "scaffold")],
        ))
        .with_scope("read")
    }

    /// Adds a scope to the session.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Marks the session active.
    #[must_use]
    pub fn activate(mut self) -> Self {
        self.active = true;
        self
    }

    /// Marks the session inactive.
    #[must_use]
    pub fn deactivate(mut self) -> Self {
        self.active = false;
        self
    }

    /// Returns the number of scopes on the session.
    #[must_use]
    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    /// Validates the session.
    pub fn validate(&self) -> Result<()> {
        if self.active && self.scopes.is_empty() {
            return Err(Error::Auth(String::from(
                "active auth sessions must carry at least one scope",
            )));
        }

        Ok(())
    }
}

impl Default for AuthSession {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Compact authentication model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthModel {
    /// Configuration for the auth system.
    pub config: AuthConfig,

    /// Current session details.
    pub session: AuthSession,

    /// Lifecycle state.
    pub state: AuthState,
}

impl AuthModel {
    /// Creates a new model.
    #[must_use]
    pub fn new(config: AuthConfig, session: AuthSession) -> Self {
        Self {
            config,
            session,
            state: AuthState::Pending,
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(AuthConfig::scaffold(), AuthSession::scaffold()).ready()
    }

    /// Marks the model ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.state = AuthState::Ready;
        self
    }

    /// Marks the model suspended.
    #[must_use]
    pub fn suspend(mut self) -> Self {
        self.state = AuthState::Suspended;
        self
    }

    /// Returns a stable summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "issuer={} scopes={} active={}",
            self.config.issuer,
            self.session.scope_count(),
            self.session.active
        )
    }

    /// Validates the model.
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        self.session.validate()?;

        if matches!(self.state, AuthState::Ready) && !self.config.enabled() {
            return Err(Error::Auth(String::from(
                "ready auth models must not be disabled",
            )));
        }

        Ok(())
    }
}

impl Default for AuthModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable authentication report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthReport {
    /// Component name.
    pub component: String,

    /// Authentication model.
    pub auth: AuthModel,

    /// Whether the scaffold is currently authenticated.
    pub authenticated: bool,
}

impl AuthReport {
    /// Builds a report from an auth model.
    #[must_use]
    pub fn new(auth: AuthModel) -> Self {
        let authenticated =
            auth.config.enabled() && auth.session.active && matches!(auth.state, AuthState::Ready);
        Self {
            component: COMPONENT.to_owned(),
            authenticated,
            auth,
        }
    }
}

/// Returns the standard authentication scaffold report.
#[must_use]
pub fn scaffold() -> AuthReport {
    AuthReport::new(AuthModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_authenticated() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.authenticated);
        assert_eq!(report.auth.config, AuthConfig::scaffold());
        assert_eq!(report.auth.session.scope_count(), 1);
    }

    #[test]
    fn validate_rejects_empty_issuer() {
        let config = AuthConfig::new("", "audience");

        let error = config.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Auth(_)));
    }

    #[test]
    fn report_roundtrips_through_json() {
        let json = serde_json::to_string(&scaffold()).expect("report serializes");
        let decoded: AuthReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.component, COMPONENT);
        assert!(decoded.authenticated);
    }
}
