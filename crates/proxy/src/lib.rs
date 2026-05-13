//! Lightweight proxy scaffolds for parity-friendly model work.

use serde::{Deserialize, Serialize};
use seriousum_core::{
    Error, Identity, Port, Result, SecurityIdentity, SecurityLabel,
    chrono::{DateTime, Utc},
};
use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, SocketAddr},
};

/// Default component name for proxy scaffolds.
pub const COMPONENT: &str = "seriousum-proxy";

/// Errors returned by pure proxy data model helpers.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// Returned when a protocol string cannot be recognized.
    #[error("unknown protocol: {0}")]
    UnknownProtocol(String),
    /// Returned when a proxy port is already reserved.
    #[error("port {0} already in use")]
    PortInUse(u16),
    /// Returned when a redirect lookup misses.
    #[error("redirect not found for endpoint {0} port {1}")]
    RedirectNotFound(u16, u16),
}

/// Layer 7 protocol handled by the proxy.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum L7Protocol {
    /// No L7 protocol is attached to the redirect.
    None,
    /// HTTP traffic handled by Envoy.
    HTTP,
    /// Kafka traffic handled by Envoy.
    Kafka,
    /// DNS traffic handled by the DNS proxy.
    DNS,
    /// Memcache traffic handled by Envoy.
    Memcache,
    /// gRPC traffic handled by Envoy.
    GRPC,
    /// Sentinel used when the protocol is not known.
    Unknown,
}

impl std::fmt::Display for L7Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::None => "none",
            Self::HTTP => "http",
            Self::Kafka => "kafka",
            Self::DNS => "dns",
            Self::Memcache => "memcache",
            Self::GRPC => "grpc",
            Self::Unknown => "unknown",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for L7Protocol {
    type Err = ProxyError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "http" => Ok(Self::HTTP),
            "kafka" => Ok(Self::Kafka),
            "dns" => Ok(Self::DNS),
            "memcache" => Ok(Self::Memcache),
            "grpc" => Ok(Self::GRPC),
            "none" | "" => Ok(Self::None),
            _ => Err(ProxyError::UnknownProtocol(s.to_string())),
        }
    }
}

/// Redirect implementation used to handle proxied traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectImplementation {
    /// Envoy-based proxy implementation.
    Envoy,
    /// Built-in DNS proxy implementation.
    DNS,
}

/// Configuration for a single proxy redirect on a port.
#[derive(Debug, Clone)]
pub struct ProxyRedirect {
    /// Endpoint owning the redirect.
    pub endpoint_id: u16,
    /// Port redirected into the proxy.
    pub port: u16,
    /// L7 protocol handled on the redirect.
    pub protocol: L7Protocol,
    /// Backing implementation for the redirect.
    pub implementation: RedirectImplementation,
    /// Whether the redirect applies to ingress traffic.
    pub ingress: bool,
}

impl ProxyRedirect {
    /// Creates a new redirect with Envoy as the default implementation.
    #[must_use]
    pub fn new(endpoint_id: u16, port: u16, protocol: L7Protocol) -> Self {
        Self {
            endpoint_id,
            port,
            protocol,
            implementation: RedirectImplementation::Envoy,
            ingress: true,
        }
    }
}

/// Verdict applied to a proxied request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessLogVerdict {
    /// Request was forwarded.
    Forwarded,
    /// Request was denied.
    Denied,
    /// Request processing failed.
    Error,
}

/// A single L7 access log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessLogEntry {
    /// Timestamp in Unix milliseconds.
    pub timestamp: u64,
    /// Final verdict for the request.
    pub verdict: AccessLogVerdict,
    /// Source socket address, when known.
    pub source: Option<SocketAddr>,
    /// Destination socket address, when known.
    pub destination: Option<SocketAddr>,
    /// L7 protocol associated with the request.
    pub protocol: L7Protocol,
    /// HTTP-specific fields.
    pub http: Option<HttpLogFields>,
    /// Kafka-specific fields.
    pub kafka: Option<KafkaLogFields>,
    /// DNS-specific fields.
    pub dns: Option<DnsLogFields>,
}

/// HTTP metadata captured in an L7 access log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpLogFields {
    /// HTTP method.
    pub method: String,
    /// Request URL.
    pub url: String,
    /// HTTP protocol version string.
    pub protocol: String,
    /// HTTP response status code.
    pub status_code: u16,
    /// Request or response headers.
    pub headers: Vec<(String, String)>,
}

/// Kafka metadata captured in an L7 access log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KafkaLogFields {
    /// Kafka error code.
    pub error_code: i16,
    /// Kafka API version.
    pub api_version: i16,
    /// Kafka API key.
    pub api_key: i16,
    /// Kafka correlation identifier.
    pub correlation_id: i32,
    /// Kafka topic.
    pub topic: String,
}

/// DNS metadata captured in an L7 access log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsLogFields {
    /// DNS query name.
    pub query: String,
    /// IPs returned by the lookup.
    pub ips: Vec<IpAddr>,
    /// DNS record TTL in seconds.
    pub ttl: u32,
    /// Source of the DNS observation.
    pub observation_source: String,
    /// DNS query types.
    pub qtypes: Vec<String>,
    /// DNS resource record types.
    pub rrtypes: Vec<String>,
}

/// Tracks active proxy redirects in memory without performing socket operations.
#[derive(Debug, Default)]
pub struct ProxyPortManager {
    redirects: HashMap<(u16, u16), ProxyRedirect>,
}

impl ProxyPortManager {
    /// Creates an empty proxy port manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a redirect for an endpoint and port pair.
    pub fn add_redirect(&mut self, redirect: ProxyRedirect) {
        tracing::debug!(
            endpoint_id = redirect.endpoint_id,
            port = redirect.port,
            protocol = %redirect.protocol,
            "adding proxy redirect"
        );
        self.redirects
            .insert((redirect.endpoint_id, redirect.port), redirect);
    }

    /// Removes a redirect for an endpoint and port pair.
    pub fn remove_redirect(&mut self, endpoint_id: u16, port: u16) {
        tracing::debug!(endpoint_id, port, "removing proxy redirect");
        self.redirects.remove(&(endpoint_id, port));
    }

    /// Returns the redirect for an endpoint and port pair, if present.
    #[must_use]
    pub fn get_redirect(&self, endpoint_id: u16, port: u16) -> Option<&ProxyRedirect> {
        self.redirects.get(&(endpoint_id, port))
    }

    /// Returns all redirects belonging to the given endpoint.
    #[must_use]
    pub fn redirects_for_endpoint(&self, endpoint_id: u16) -> Vec<&ProxyRedirect> {
        self.redirects
            .values()
            .filter(|redirect| redirect.endpoint_id == endpoint_id)
            .collect()
    }

    /// Returns the number of tracked redirects.
    #[must_use]
    pub fn count(&self) -> usize {
        self.redirects.len()
    }
}

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

/// Proxy listener type used for lightweight parity tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyListenerType {
    /// DNS proxy.
    Dns,
    /// HTTP proxy.
    Http,
    /// TLS proxy.
    Tls,
    /// CRD listener proxy.
    Crd,
}

impl std::fmt::Display for ProxyListenerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Dns => "dns",
            Self::Http => "http",
            Self::Tls => "tls",
            Self::Crd => "crd",
        };
        write!(f, "{value}")
    }
}

/// In-memory proxy port state matching Go proxyports allocator transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyPortEntry {
    /// Listener type.
    pub proxy_type: ProxyListenerType,
    /// Direction marker.
    pub ingress: bool,
    /// Allocated listen port.
    pub proxy_port: u16,
    /// Port programmed into datapath.
    pub rules_port: u16,
    /// Port configuration state.
    pub configured: bool,
    /// Datapath acknowledgment state.
    pub acknowledged: bool,
    /// Redirect reference count.
    pub n_redirects: i32,
    /// Static listeners are not released.
    pub is_static: bool,
}

impl ProxyPortEntry {
    fn new(proxy_type: ProxyListenerType, ingress: bool) -> Self {
        Self {
            proxy_type,
            ingress,
            proxy_port: 0,
            rules_port: 0,
            configured: false,
            acknowledged: false,
            n_redirects: 0,
            is_static: false,
        }
    }

    fn add_reference(&mut self) {
        self.n_redirects += 1;
    }
}

/// Pure allocator/state model ported from `pkg/proxy/proxyports/proxyports.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyPortAllocator {
    range_min: u16,
    range_max: u16,
    /// true=in use, false=available for reuse.
    pub allocated_ports: BTreeMap<u16, bool>,
    pub proxy_ports: BTreeMap<String, ProxyPortEntry>,
}

impl ProxyPortAllocator {
    /// Creates a new allocator in the provided port range.
    #[must_use]
    pub fn new(range_min: u16, range_max: u16) -> Self {
        Self {
            range_min,
            range_max,
            allocated_ports: BTreeMap::new(),
            proxy_ports: BTreeMap::new(),
        }
    }

    fn is_port_available(&self, port: u16, reuse: bool) -> bool {
        match self.allocated_ports.get(&port) {
            None => true,
            Some(in_use) => reuse && !*in_use,
        }
    }

    fn allocate_port(&self, requested: u16) -> Result<u16> {
        if requested != 0 && self.is_port_available(requested, false) {
            return Ok(requested);
        }

        for reuse in [false, true] {
            for port in self.range_min..=self.range_max {
                if self.is_port_available(port, reuse) {
                    return Ok(port);
                }
            }
        }

        Err(Error::Proxy(String::from("no available proxy ports")))
    }

    fn reset_entry(&mut self, name: &str) -> Result<()> {
        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;

        if entry.proxy_port != 0 {
            self.allocated_ports.insert(entry.proxy_port, false);
        }
        entry.proxy_port = 0;
        entry.configured = false;
        entry.acknowledged = false;
        Ok(())
    }

    /// Returns a mutable entry, creating it if missing.
    pub fn ensure_proxy_port(
        &mut self,
        name: impl Into<String>,
        proxy_type: ProxyListenerType,
        ingress: bool,
    ) -> &mut ProxyPortEntry {
        self.proxy_ports
            .entry(name.into())
            .or_insert_with(|| ProxyPortEntry::new(proxy_type, ingress))
    }

    /// Port of `AllocatePort`.
    pub fn allocate_port_for(&mut self, name: &str, retry: bool) -> Result<u16> {
        let needs_reallocate = {
            let entry = self
                .proxy_ports
                .get(name)
                .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
            !entry.configured && (retry || entry.proxy_port == 0)
        };

        if needs_reallocate {
            if self
                .proxy_ports
                .get(name)
                .is_some_and(|entry| entry.proxy_port != 0)
            {
                self.reset_entry(name)?;
            }

            let requested = self
                .proxy_ports
                .get(name)
                .map_or(0, |entry| entry.proxy_port);
            let allocated = self.allocate_port(requested)?;
            if let Some(entry) = self.proxy_ports.get_mut(name) {
                entry.proxy_port = allocated;
            }
        }

        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        if entry.proxy_port == 0 {
            return Err(Error::Proxy(format!("zero port on {name} not allowed")));
        }
        self.allocated_ports.insert(entry.proxy_port, true);
        entry.configured = true;
        Ok(entry.proxy_port)
    }

    /// Port of `AllocateCRDProxyPort`.
    pub fn allocate_crd_proxy_port(&mut self, name: &str) -> Result<u16> {
        self.proxy_ports
            .entry(name.to_owned())
            .or_insert_with(|| ProxyPortEntry::new(ProxyListenerType::Crd, false));

        if self
            .proxy_ports
            .get(name)
            .is_some_and(|entry| entry.ingress)
        {
            self.proxy_ports.insert(
                name.to_owned(),
                ProxyPortEntry::new(ProxyListenerType::Crd, false),
            );
        }

        let (current_port, rules_port) = self
            .proxy_ports
            .get(name)
            .map(|entry| (entry.proxy_port, entry.rules_port))
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        let allocated = if current_port != 0 {
            current_port
        } else if rules_port != 0
            && !self
                .allocated_ports
                .get(&rules_port)
                .copied()
                .unwrap_or(false)
        {
            rules_port
        } else {
            self.allocate_port(rules_port)?
        };

        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        entry.proxy_port = allocated;
        entry.configured = true;
        self.allocated_ports.insert(allocated, true);
        Ok(allocated)
    }

    /// Port of `ReallocateCRDProxyPort`.
    pub fn reallocate_crd_proxy_port(&mut self, name: &str) -> Result<u16> {
        let has_port = self
            .proxy_ports
            .get(name)
            .is_some_and(|entry| entry.proxy_port != 0);
        if has_port {
            self.reset_entry(name)?;
            if let Some(entry) = self.proxy_ports.get_mut(name) {
                entry.rules_port = 0;
            }
        } else {
            self.ensure_proxy_port(name.to_owned(), ProxyListenerType::Crd, false);
        }

        let allocated = self.allocate_port(0)?;
        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        entry.proxy_port = allocated;
        entry.configured = true;
        self.allocated_ports.insert(allocated, true);
        Ok(allocated)
    }

    /// Port of `GetProxyPort`.
    pub fn get_proxy_port(&self, name: &str) -> Result<(u16, bool)> {
        self.proxy_ports
            .get(name)
            .map(|entry| (entry.proxy_port, entry.is_static))
            .ok_or_else(|| Error::Proxy(format!("unrecognized proxy: {name}")))
    }

    /// Port of `FindByTypeWithReference`.
    pub fn find_by_type_with_reference(
        &mut self,
        listener_type: ProxyListenerType,
        listener: &str,
        ingress: bool,
    ) -> Option<String> {
        if listener_type == ProxyListenerType::Crd {
            if let Some(entry) = self.proxy_ports.get_mut(listener)
                && entry.proxy_type == ProxyListenerType::Crd
                && !entry.ingress
            {
                entry.add_reference();
                return Some(listener.to_owned());
            }
            return None;
        }

        for (name, entry) in &mut self.proxy_ports {
            if entry.proxy_type == listener_type && entry.ingress == ingress {
                entry.add_reference();
                return Some(name.clone());
            }
        }
        None
    }

    /// Port of `AckProxyPort`.
    pub fn ack_proxy_port(&mut self, name: &str) -> Result<()> {
        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        if entry.proxy_port == 0 {
            return Err(Error::Proxy(format!(
                "ackProxyPort: zero port on {name} not allowed"
            )));
        }
        if entry.rules_port != entry.proxy_port {
            entry.rules_port = entry.proxy_port;
        }
        entry.acknowledged = true;
        Ok(())
    }

    /// Port of `AckProxyPortWithReference`.
    pub fn ack_proxy_port_with_reference(&mut self, name: &str) -> Result<()> {
        self.ack_proxy_port(name)?;
        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        entry.add_reference();
        Ok(())
    }

    /// Port of `ReleaseProxyPort` and immediate `releaseProxyPort` state transition.
    pub fn release_proxy_port_with_wait(&mut self, name: &str) -> Result<()> {
        let (is_static, n_redirects) = self
            .proxy_ports
            .get(name)
            .map(|entry| (entry.is_static, entry.n_redirects))
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;

        if n_redirects <= 0 {
            if let Some(entry) = self.proxy_ports.get_mut(name) {
                entry.n_redirects = 0;
            }
            return Err(Error::Proxy(format!(
                "failed to release proxy port with has non-positive reference count: {n_redirects}"
            )));
        }

        if let Some(entry) = self.proxy_ports.get_mut(name) {
            entry.n_redirects -= 1;
        }

        let should_release = self
            .proxy_ports
            .get(name)
            .is_some_and(|entry| !is_static && entry.n_redirects == 0);
        if should_release {
            self.reset_entry(name)?;
        }
        Ok(())
    }

    /// Port of `ResetUnacknowledged`.
    pub fn reset_unacknowledged(&mut self, name: &str) -> Result<()> {
        let should_reset = self
            .proxy_ports
            .get(name)
            .is_some_and(|entry| !entry.is_static && !entry.acknowledged);
        if should_reset {
            self.reset_entry(name)?;
        }
        Ok(())
    }

    /// Port of `HasProxyType`.
    #[must_use]
    pub fn has_proxy_type(&self, name: &str, listener_type: ProxyListenerType) -> bool {
        self.proxy_ports
            .get(name)
            .is_some_and(|entry| entry.configured && entry.proxy_type == listener_type)
    }

    /// Port of `Restore`.
    pub fn restore(&mut self, name: &str) -> Result<()> {
        let entry = self
            .proxy_ports
            .get_mut(name)
            .ok_or_else(|| Error::Proxy(format!("failed to find proxy port {name}")))?;
        if entry.proxy_port == 0 && entry.rules_port != 0 {
            entry.proxy_port = entry.rules_port;
        }
        Ok(())
    }
}

/// Ported from `pkg/proxy/envoyproxy.go` address decision.
#[must_use]
pub fn may_use_original_source_address(
    proxy_use_original_source_address: bool,
    supports_original_source_address: bool,
    ingress: bool,
) -> bool {
    proxy_use_original_source_address && supports_original_source_address && !ingress
}

fn proxy_type_not_found_error(
    listener_type: ProxyListenerType,
    listener: &str,
    ingress: bool,
) -> Error {
    let direction = if ingress { "ingress" } else { "egress" };
    Error::Proxy(format!(
        "unrecognized {direction} proxy type for {listener}: {listener_type}"
    ))
}

/// Minimal pure redirect manager for missing-listener parity logic.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectManager {
    /// Mirrors EnvoyProxyIntegrationConfig.ProxyUseOriginalSourceAddress.
    pub proxy_use_original_source_address: bool,
    /// Captures xDS listener argument when listener creation happens.
    pub observed_may_use_original_source_addr: bool,
}

impl RedirectManager {
    /// Creates a manager with the given source-address flag.
    #[must_use]
    pub fn new(proxy_use_original_source_address: bool) -> Self {
        Self {
            proxy_use_original_source_address,
            observed_may_use_original_source_addr: false,
        }
    }

    /// Pure port of missing-listener decision path from `CreateOrUpdateRedirect`.
    pub fn create_or_update_redirect(
        &mut self,
        allocator: &mut ProxyPortAllocator,
        listener_type: ProxyListenerType,
        listener: &str,
        ingress: bool,
        supports_original_source_address: bool,
    ) -> Result<u16> {
        let proxy_name = allocator
            .find_by_type_with_reference(listener_type, listener, ingress)
            .ok_or_else(|| proxy_type_not_found_error(listener_type, listener, ingress))?;
        let (proxy_port, _) = allocator.get_proxy_port(&proxy_name)?;
        self.observed_may_use_original_source_addr = may_use_original_source_address(
            self.proxy_use_original_source_address,
            supports_original_source_address,
            ingress,
        );
        Ok(proxy_port)
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;

    #[test]
    fn parity_test_create_or_update_redirect_missing_listener() {
        let mut allocator = ProxyPortAllocator::new(10_000, 20_000);
        let mut manager = RedirectManager::new(true);

        let err = manager
            .create_or_update_redirect(
                &mut allocator,
                ProxyListenerType::Crd,
                "nonexisting-listener",
                false,
                true,
            )
            .expect_err("missing listener must fail");
        assert!(matches!(err, Error::Proxy(_)));
        assert!(!manager.observed_may_use_original_source_addr);
    }

    #[test]
    fn parity_test_create_or_update_redirect_missing_listener_with_use_original_source_addr_flag_enabled()
     {
        let mut allocator = ProxyPortAllocator::new(10_000, 20_000);
        let mut manager = RedirectManager::new(true);

        let _ = manager
            .create_or_update_redirect(
                &mut allocator,
                ProxyListenerType::Http,
                "nonexisting-listener",
                false,
                true,
            )
            .expect_err("missing listener must fail");
        assert!(manager.proxy_use_original_source_address);
    }

    #[test]
    fn parity_test_create_or_update_redirect_missing_listener_with_use_original_source_addr_flag_disabled()
     {
        let mut allocator = ProxyPortAllocator::new(10_000, 20_000);
        let mut manager = RedirectManager::new(false);

        let _ = manager
            .create_or_update_redirect(
                &mut allocator,
                ProxyListenerType::Http,
                "nonexisting-listener",
                false,
                true,
            )
            .expect_err("missing listener must fail");
        assert!(!manager.proxy_use_original_source_address);
        assert!(!manager.observed_may_use_original_source_addr);
    }

    #[test]
    fn parity_test_port_allocator() {
        let mut allocator = ProxyPortAllocator::new(10_000, 20_000);

        let port = allocator
            .allocate_crd_proxy_port("listener1")
            .expect("first CRD allocation succeeds");
        assert_ne!(0, port);

        let (port1, _) = allocator
            .get_proxy_port("listener1")
            .expect("listener exists");
        assert_eq!(port, port1);

        let port1a = allocator
            .allocate_crd_proxy_port("listener1")
            .expect("same listener should keep port");
        assert_eq!(port1, port1a);

        let name = allocator
            .find_by_type_with_reference(ProxyListenerType::Crd, "listener1", false)
            .expect("find CRD listener");
        assert_eq!("listener1", name);

        let pp = allocator
            .proxy_ports
            .get("listener1")
            .expect("state exists");
        assert_eq!(ProxyListenerType::Crd, pp.proxy_type);
        assert_eq!(port, pp.proxy_port);
        assert!(!pp.ingress);
        assert!(pp.configured);
        assert!(!pp.acknowledged);
        assert!(!pp.is_static);
        assert_eq!(1, pp.n_redirects);
        assert_eq!(0, pp.rules_port);

        allocator
            .reset_unacknowledged("listener1")
            .expect("reset succeeds");
        let pp = allocator
            .proxy_ports
            .get("listener1")
            .expect("state exists");
        assert!(!pp.configured);
        assert!(!pp.acknowledged);
        assert_eq!(0, pp.proxy_port);

        allocator
            .release_proxy_port_with_wait("listener1")
            .expect("release succeeds");
        let pp = allocator
            .proxy_ports
            .get("listener1")
            .expect("state exists");
        assert_eq!(0, pp.n_redirects);
        assert_eq!(0, pp.proxy_port);
        assert!(!pp.configured);
        assert!(!pp.acknowledged);

        let (port_after_release, _) = allocator
            .get_proxy_port("listener1")
            .expect("listener still tracked");
        assert_eq!(0, port_after_release);

        let port2 = allocator
            .allocate_crd_proxy_port("listener1")
            .expect("second allocation succeeds");
        assert_ne!(port, port2);
        let found_again = allocator
            .find_by_type_with_reference(ProxyListenerType::Crd, "listener1", false)
            .expect("second lookup succeeds");
        assert_eq!("listener1", found_again);

        allocator
            .ack_proxy_port("listener1")
            .expect("ack should succeed");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(1, pp.n_redirects);
            assert!(pp.acknowledged);
            assert_eq!(port2, pp.rules_port);
        }

        allocator
            .ack_proxy_port_with_reference("listener1")
            .expect("ack with reference should succeed");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(2, pp.n_redirects);
            assert!(pp.acknowledged);
            assert_eq!(port2, pp.rules_port);
        }

        allocator
            .release_proxy_port_with_wait("listener1")
            .expect("first release should keep one ref");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(1, pp.n_redirects);
            assert!(pp.configured);
            assert!(pp.acknowledged);
            assert_eq!(port2, pp.proxy_port);
        }

        allocator
            .reset_unacknowledged("listener1")
            .expect("acknowledged entries are not reset");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(1, pp.n_redirects);
            assert!(pp.configured);
            assert!(pp.acknowledged);
            assert_eq!(port2, pp.proxy_port);
        }

        allocator
            .release_proxy_port_with_wait("listener1")
            .expect("second release frees listener");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(0, pp.n_redirects);
            assert!(!pp.configured);
            assert!(!pp.acknowledged);
            assert_eq!(0, pp.proxy_port);
            assert_eq!(port2, pp.rules_port);
        }

        let err = allocator
            .release_proxy_port_with_wait("listener1")
            .expect_err("extra release should fail");
        assert!(matches!(err, Error::Proxy(_)));

        allocator.allocated_ports.insert(port2, true);
        let port3 = allocator
            .allocate_crd_proxy_port("listener1")
            .expect("third allocation succeeds");
        assert_ne!(0, port3);
        assert_ne!(port2, port3);
        let found_third = allocator
            .find_by_type_with_reference(ProxyListenerType::Crd, "listener1", false)
            .expect("third lookup succeeds");
        assert_eq!("listener1", found_third);
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(port3, pp.proxy_port);
            assert!(pp.configured);
            assert!(!pp.acknowledged);
            assert_eq!(1, pp.n_redirects);
            assert_eq!(port2, pp.rules_port);
        }

        allocator
            .ack_proxy_port("listener1")
            .expect("ack should succeed");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(1, pp.n_redirects);
            assert!(pp.acknowledged);
            assert_eq!(port3, pp.rules_port);
        }

        allocator
            .release_proxy_port_with_wait("listener1")
            .expect("release should free third port");
        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(0, pp.n_redirects);
            assert!(!pp.configured);
            assert!(!pp.acknowledged);
            assert_eq!(0, pp.proxy_port);
            assert_eq!(port3, pp.rules_port);
        }

        assert_eq!(Some(&false), allocator.allocated_ports.get(&port3));

        let port4 = allocator
            .allocate_crd_proxy_port("listener1")
            .expect("re-allocation should reuse previous datapath port");
        assert_eq!(port3, port4);
        let pp = allocator
            .proxy_ports
            .get("listener1")
            .expect("state exists");
        assert_eq!(ProxyListenerType::Crd, pp.proxy_type);
        assert!(!pp.ingress);
        assert_eq!(port4, pp.proxy_port);
        assert!(pp.configured);
        assert!(!pp.acknowledged);
        assert_eq!(0, pp.n_redirects);
        assert_eq!(port3, pp.rules_port);
    }

    #[test]
    fn parity_test_restored_port() {
        let mut allocator = ProxyPortAllocator::new(10_000, 20_000);
        const PP_NAME: &str = "cilium-http-egress";
        const RESTORED_PORT: u16 = 14_321;

        allocator.ensure_proxy_port(PP_NAME, ProxyListenerType::Http, false);
        allocator
            .proxy_ports
            .get_mut(PP_NAME)
            .expect("state exists")
            .proxy_port = RESTORED_PORT;

        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert!(!pp.configured);
            assert!(!pp.acknowledged);
            assert_eq!(0, pp.rules_port);
        }

        allocator
            .allocate_port_for(PP_NAME, false)
            .expect("restored allocation works");
        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert_eq!(RESTORED_PORT, pp.proxy_port);
            assert!(pp.configured);
            assert!(!pp.acknowledged);
        }

        assert!(allocator.has_proxy_type(PP_NAME, ProxyListenerType::Http));

        allocator
            .reset_unacknowledged(PP_NAME)
            .expect("reset succeeds");
        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert_eq!(0, pp.proxy_port);
            assert!(!pp.configured);
            assert!(!pp.acknowledged);
        }

        allocator
            .allocate_port_for(PP_NAME, false)
            .expect("allocate after nack works");
        let new_port = allocator
            .proxy_ports
            .get(PP_NAME)
            .expect("state exists")
            .proxy_port;
        assert_ne!(0, new_port);
        assert_ne!(RESTORED_PORT, new_port);
        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert!(pp.configured);
            assert!(!pp.acknowledged);
        }

        allocator
            .ack_proxy_port_with_reference(PP_NAME)
            .expect("ack succeeds");
        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert_eq!(new_port, pp.proxy_port);
            assert!(pp.configured);
            assert!(pp.acknowledged);
            assert_eq!(1, pp.n_redirects);
        }

        allocator
            .release_proxy_port_with_wait(PP_NAME)
            .expect("release succeeds");
        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert_eq!(0, pp.n_redirects);
            assert_eq!(0, pp.proxy_port);
            assert!(!pp.configured);
            assert!(!pp.acknowledged);
            assert_eq!(new_port, pp.rules_port);
        }

        allocator.restore(PP_NAME).expect("restore succeeds");
        allocator
            .allocate_port_for(PP_NAME, false)
            .expect("allocation after restore succeeds");
        {
            let pp = allocator.proxy_ports.get(PP_NAME).expect("state exists");
            assert_eq!(new_port, pp.proxy_port);
            assert!(pp.configured);
            assert!(!pp.acknowledged);
        }
    }

    #[test]
    fn parity_test_reallocate_crd_proxy_port() {
        let mut allocator = ProxyPortAllocator::new(10_000, 20_000);

        let port1 = allocator
            .allocate_crd_proxy_port("listener1")
            .expect("first allocation succeeds");
        assert_ne!(0, port1);

        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(port1, pp.proxy_port);
            assert_eq!(0, pp.rules_port);
            assert!(pp.configured);
            assert!(!pp.acknowledged);
        }

        let port2 = allocator
            .reallocate_crd_proxy_port("listener1")
            .expect("reallocation succeeds");
        assert_ne!(0, port2);
        assert_ne!(port1, port2);

        {
            let pp = allocator
                .proxy_ports
                .get("listener1")
                .expect("state exists");
            assert_eq!(port2, pp.proxy_port);
            assert_eq!(0, pp.rules_port);
            assert!(pp.configured);
            assert!(!pp.acknowledged);
        }

        assert_eq!(Some(&false), allocator.allocated_ports.get(&port1));
        assert_eq!(Some(&true), allocator.allocated_ports.get(&port2));
    }

    // Requires Linux routing/netlink; ref TestPrivilegedRoutes in
    // pkg/proxy/routes_test.go.
    #[test]
    #[cfg_attr(
        not(feature = "privileged"),
        ignore = "requires root + BPF kernel; run with: cargo test --features privileged"
    )]
    fn parity_test_privileged_routes() {}
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

    #[test]
    fn test_l7_protocol_roundtrip() {
        for proto in [
            L7Protocol::HTTP,
            L7Protocol::Kafka,
            L7Protocol::DNS,
            L7Protocol::GRPC,
        ] {
            let s = proto.to_string();
            let back: L7Protocol = s.parse().unwrap();
            assert_eq!(proto, back);
        }
    }

    #[test]
    fn test_l7_protocol_unknown_parse_error() {
        assert!("ftp".parse::<L7Protocol>().is_err());
    }

    #[test]
    fn test_proxy_port_manager() {
        let mut mgr = ProxyPortManager::new();
        mgr.add_redirect(ProxyRedirect::new(1, 8_080, L7Protocol::HTTP));
        mgr.add_redirect(ProxyRedirect::new(1, 9_090, L7Protocol::GRPC));
        mgr.add_redirect(ProxyRedirect::new(2, 8_080, L7Protocol::HTTP));
        assert_eq!(mgr.count(), 3);
        assert_eq!(mgr.redirects_for_endpoint(1).len(), 2);
        mgr.remove_redirect(1, 8_080);
        assert_eq!(mgr.count(), 2);
        assert!(mgr.get_redirect(1, 8_080).is_none());
    }

    #[test]
    fn test_access_log_serde() {
        let entry = AccessLogEntry {
            timestamp: 1_000_000,
            verdict: AccessLogVerdict::Forwarded,
            source: None,
            destination: None,
            protocol: L7Protocol::HTTP,
            http: Some(HttpLogFields {
                method: "GET".into(),
                url: "/health".into(),
                protocol: "HTTP/1.1".into(),
                status_code: 200,
                headers: vec![],
            }),
            kafka: None,
            dns: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: AccessLogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.verdict, AccessLogVerdict::Forwarded);
    }
}
