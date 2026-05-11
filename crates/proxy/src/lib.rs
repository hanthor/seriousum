//! Lightweight proxy scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{
    Error, Identity, Port, Result, SecurityIdentity, SecurityLabel,
    chrono::{DateTime, Utc},
};
use std::{collections::BTreeMap, net::IpAddr};

/// Default component name for proxy scaffolds.
pub const COMPONENT: &str = "seriousum-proxy";

/// Proxy operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Plain HTTP proxying.
    Http,
    /// Raw TCP proxying.
    Tcp,
    /// Transparent proxying.
    Transparent,
}

/// Proxy lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    /// Proxy is starting.
    Pending,
    /// Proxy is ready.
    Ready,
    /// Proxy is draining.
    Draining,
}

/// Compact proxy configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Address to listen on.
    pub bind_address: IpAddr,

    /// Port to listen on.
    pub bind_port: Port,

    /// Upstream target.
    pub upstream: String,

    /// Maximum allowed connections.
    pub max_connections: u32,
}

impl ProxyConfig {
    /// Creates a new proxy configuration.
    #[must_use]
    pub fn new(bind_address: IpAddr, bind_port: Port, upstream: impl Into<String>) -> Self {
        Self {
            bind_address,
            bind_port,
            upstream: upstream.into(),
            max_connections: 1_024,
        }
    }

    /// Returns the default scaffold configuration.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            IpAddr::from([127, 0, 0, 1]),
            Port::new(1_500),
            "http://127.0.0.1:8080",
        )
    }

    /// Returns the listening socket string.
    #[must_use]
    pub fn socket_string(&self) -> String {
        format!("{}:{}", self.bind_address, self.bind_port)
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.upstream.trim().is_empty() {
            return Err(Error::Proxy(String::from(
                "proxy upstream must not be empty",
            )));
        }

        if self.bind_port.as_u16() == 0 {
            return Err(Error::Proxy(String::from(
                "proxy bind port must not be zero",
            )));
        }

        if self.max_connections == 0 {
            return Err(Error::Proxy(String::from(
                "proxy max connections must be greater than zero",
            )));
        }

        Ok(())
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Proxy session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxySession {
    /// Session identifier.
    pub session_id: String,

    /// Timestamp when the session started.
    pub started_at: DateTime<Utc>,

    /// Whether the session is active.
    pub active: bool,

    /// Metadata carried through the session.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl ProxySession {
    /// Creates a new proxy session.
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            started_at: Utc::now(),
            active: true,
            metadata: BTreeMap::new(),
        }
    }

    /// Returns the default scaffold session.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new("proxy-scaffold").with_metadata("route", "scaffold")
    }

    /// Adds metadata to the session.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Marks the session inactive.
    #[must_use]
    pub fn deactivate(mut self) -> Self {
        self.active = false;
        self
    }

    /// Validates the session.
    pub fn validate(&self) -> Result<()> {
        if self.session_id.trim().is_empty() {
            return Err(Error::Proxy(String::from(
                "proxy session id must not be empty",
            )));
        }

        Ok(())
    }
}

impl Default for ProxySession {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Compact proxy model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyModel {
    /// Proxy identity.
    pub identity: Identity,

    /// Configuration for the proxy.
    pub config: ProxyConfig,

    /// Active session details.
    pub session: ProxySession,

    /// Lifecycle state.
    pub state: ProxyState,

    /// Proxy operating mode.
    pub mode: ProxyMode,
}

impl ProxyModel {
    /// Creates a new proxy model.
    #[must_use]
    pub fn new(identity: Identity, config: ProxyConfig, session: ProxySession) -> Self {
        Self {
            identity,
            config,
            session,
            state: ProxyState::Pending,
            mode: ProxyMode::Http,
        }
    }

    /// Returns the default scaffold model.
    #[must_use]
    pub fn scaffold() -> Self {
        Self::new(
            Identity::new(
                SecurityIdentity::host(),
                [SecurityLabel::new("proxy", "scaffold")],
            ),
            ProxyConfig::scaffold(),
            ProxySession::scaffold(),
        )
        .ready()
    }

    /// Marks the model ready.
    #[must_use]
    pub fn ready(mut self) -> Self {
        self.state = ProxyState::Ready;
        self
    }

    /// Marks the model draining.
    #[must_use]
    pub fn drain(mut self) -> Self {
        self.state = ProxyState::Draining;
        self
    }

    /// Sets the proxy mode.
    #[must_use]
    pub fn with_mode(mut self, mode: ProxyMode) -> Self {
        self.mode = mode;
        self
    }

    /// Returns a stable summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "socket={} upstream={} active={}",
            self.config.socket_string(),
            self.config.upstream,
            self.session.active
        )
    }

    /// Validates the model.
    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        self.session.validate()?;

        Ok(())
    }
}

impl Default for ProxyModel {
    fn default() -> Self {
        Self::scaffold()
    }
}

/// Serializable proxy report for future API surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyReport {
    /// Component name.
    pub component: String,

    /// Proxy model.
    pub proxy: ProxyModel,

    /// Whether the proxy is healthy.
    pub healthy: bool,
}

impl ProxyReport {
    /// Builds a report from a proxy model.
    #[must_use]
    pub fn new(proxy: ProxyModel) -> Self {
        let healthy = matches!(proxy.state, ProxyState::Ready) && proxy.session.active;
        Self {
            component: COMPONENT.to_owned(),
            healthy,
            proxy,
        }
    }
}

/// Returns the standard proxy scaffold report.
#[must_use]
pub fn scaffold() -> ProxyReport {
    ProxyReport::new(ProxyModel::scaffold())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_report_is_healthy() {
        let report = scaffold();

        assert_eq!(report.component, COMPONENT);
        assert!(report.healthy);
        assert_eq!(report.proxy.config.socket_string(), "127.0.0.1:1500");
        assert_eq!(report.proxy.identity.id, SecurityIdentity::host());
    }

    #[test]
    fn validate_rejects_empty_upstream() {
        let config = ProxyConfig::new(IpAddr::from([127, 0, 0, 1]), Port::new(1_500), "");

        let error = config.validate().expect_err("validation should fail");
        assert!(matches!(error, Error::Proxy(_)));
    }

    #[test]
    fn report_roundtrips_through_json() {
        let json = serde_json::to_string(&scaffold()).expect("report serializes");
        let decoded: ProxyReport = serde_json::from_str(&json).expect("report deserializes");

        assert_eq!(decoded.component, COMPONENT);
        assert!(decoded.healthy);
    }
}
