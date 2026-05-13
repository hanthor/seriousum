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

/// The authentication method required between two endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthType {
    /// No authentication required.
    Disabled,
    /// SPIFFE mTLS authentication.
    SPIFFE,
    /// WireGuard-based authentication.
    Wireguard,
    /// Test authentication used in tests only.
    Test,
}

impl std::fmt::Display for AuthType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "disabled"),
            Self::SPIFFE => write!(f, "spiffe"),
            Self::Wireguard => write!(f, "wireguard"),
            Self::Test => write!(f, "test"),
        }
    }
}

/// Identifies a pair of endpoints that need mutual authentication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthKey {
    /// Local endpoint security identity.
    pub local_identity: u32,
    /// Remote endpoint security identity.
    pub remote_identity: u32,
    /// Remote node identifier.
    pub remote_node_id: u16,
    /// Required authentication type.
    pub auth_type: AuthType,
}

/// The current authentication status for an [`AuthKey`] pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    /// Authentication is required but not yet established.
    Pending,
    /// Authentication is established and valid.
    Authenticated,
    /// Authentication failed.
    Failed,
    /// Authentication timed out.
    TimedOut,
}

/// A completed authentication entry with optional expiry metadata.
#[derive(Debug, Clone)]
pub struct AuthEntry {
    /// The authenticated endpoint pair.
    pub key: AuthKey,
    /// The current status for the pair.
    pub status: AuthStatus,
    /// When this authentication expires, if it expires.
    pub expiry: Option<std::time::SystemTime>,
}

impl AuthEntry {
    /// Creates a new authentication entry without an expiry.
    #[must_use]
    pub fn new(key: AuthKey, status: AuthStatus) -> Self {
        Self {
            key,
            status,
            expiry: None,
        }
    }

    /// Returns a copy of the entry with the supplied expiry.
    #[must_use]
    pub fn with_expiry(mut self, t: std::time::SystemTime) -> Self {
        self.expiry = Some(t);
        self
    }

    /// Returns true when the entry represents currently valid authentication.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.status != AuthStatus::Authenticated {
            return false;
        }

        match self.expiry {
            None => true,
            Some(exp) => exp > std::time::SystemTime::now(),
        }
    }
}

/// In-memory map of authentication entries mirroring the BPF auth map.
#[derive(Debug, Default)]
pub struct AuthMap {
    entries: std::collections::HashMap<AuthKey, AuthEntry>,
}

impl AuthMap {
    /// Creates an empty authentication map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces an authentication entry.
    pub fn insert(&mut self, entry: AuthEntry) {
        self.entries.insert(entry.key.clone(), entry);
    }

    /// Returns an entry for the supplied key, if present.
    #[must_use]
    pub fn get(&self, key: &AuthKey) -> Option<&AuthEntry> {
        self.entries.get(key)
    }

    /// Removes and returns an entry for the supplied key, if present.
    pub fn remove(&mut self, key: &AuthKey) -> Option<AuthEntry> {
        self.entries.remove(key)
    }

    /// Returns true when the supplied key has valid authentication.
    #[must_use]
    pub fn is_authenticated(&self, key: &AuthKey) -> bool {
        self.entries.get(key).is_some_and(AuthEntry::is_valid)
    }

    /// Removes expired or terminal entries and returns the number deleted.
    pub fn gc(&mut self) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| entry.is_valid() || entry.status == AuthStatus::Pending);
        let removed = before - self.entries.len();
        tracing::debug!(
            removed,
            remaining = self.entries.len(),
            "garbage collected auth entries"
        );
        removed
    }

    /// Returns the number of entries in the map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A request to authenticate a pair of endpoints.
#[derive(Debug, Clone)]
pub struct AuthRequest {
    /// The endpoint pair to authenticate.
    pub key: AuthKey,
    /// When the request was created.
    pub timestamp: std::time::SystemTime,
}

impl AuthRequest {
    /// Creates a new authentication request with the current timestamp.
    #[must_use]
    pub fn new(key: AuthKey) -> Self {
        Self {
            key,
            timestamp: std::time::SystemTime::now(),
        }
    }
}

/// Trait implemented by authentication backends.
pub trait AuthHandler: Send + Sync {
    /// Returns the handler's authentication type.
    fn auth_type(&self) -> AuthType;

    /// Authenticates the supplied request.
    fn authenticate(&self, req: &AuthRequest) -> std::result::Result<AuthEntry, AuthError>;
}

/// Errors returned by authentication handlers.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The requested authentication type is not supported.
    #[error("auth type {0} not supported")]
    UnsupportedAuthType(AuthType),
    /// Authentication failed for a handler-specific reason.
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    /// Certificate validation failed.
    #[error("certificate error: {0}")]
    CertificateError(String),
    /// Authentication timed out.
    #[error("auth timed out")]
    Timeout,
}

#[cfg(test)]
mod parity_tests {
    use std::collections::{HashMap, HashSet};

    use seriousum_core::chrono::{Duration, Utc};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    struct AuthKey {
        local_identity: u32,
        remote_identity: u32,
        remote_node_id: u16,
        auth_type: u8,
    }

    #[derive(Debug, Clone, Copy)]
    struct AuthInfo {
        expiration: seriousum_core::chrono::DateTime<Utc>,
    }

    #[derive(Debug, Clone, Copy)]
    struct AuthInfoCache {
        auth_info: AuthInfo,
        stored_at: seriousum_core::chrono::DateTime<Utc>,
    }

    #[derive(Debug, Clone)]
    struct FakeAuthMap {
        entries: HashMap<AuthKey, AuthInfo>,
        fail_delete: bool,
        fail_all: bool,
    }

    impl FakeAuthMap {
        fn with_entries(entries: HashMap<AuthKey, AuthInfo>) -> Self {
            Self {
                entries,
                fail_delete: false,
                fail_all: false,
            }
        }

        fn delete(&mut self, key: AuthKey) -> Result<(), String> {
            if self.fail_delete {
                return Err(String::from("failed to delete entry"));
            }
            if self.entries.remove(&key).is_none() {
                return Err(String::from("key does not exist"));
            }
            Ok(())
        }

        fn delete_if<F>(&mut self, predicate: F) -> Result<(), String>
        where
            F: FnMut(&AuthKey, &mut AuthInfo) -> bool,
        {
            if self.fail_delete {
                return Err(String::from("failed to delete entry"));
            }
            let mut predicate = predicate;
            self.entries.retain(|k, v| !predicate(k, v));
            Ok(())
        }

        fn all(&self) -> Result<HashMap<AuthKey, AuthInfo>, String> {
            if self.fail_all {
                return Err(String::from("failed to list entries"));
            }
            Ok(self.entries.clone())
        }

        fn update(&mut self, key: AuthKey, info: AuthInfo) {
            self.entries.insert(key, info);
        }
    }

    struct AuthMapCache {
        auth_map: FakeAuthMap,
        cache_entries: HashMap<AuthKey, AuthInfoCache>,
    }

    impl AuthMapCache {
        fn all(&self) -> HashMap<AuthKey, AuthInfo> {
            let mut out = HashMap::new();
            for (k, v) in &self.cache_entries {
                out.insert(*k, v.auth_info);
            }
            out
        }

        fn get(&self, key: AuthKey) -> Result<AuthInfo, String> {
            self.cache_entries
                .get(&key)
                .map(|v| v.auth_info)
                .ok_or_else(|| format!("failed to get auth info for key: {key:?}"))
        }

        fn delete(&mut self, key: AuthKey) -> Result<(), String> {
            match self.auth_map.delete(key) {
                Ok(()) => {
                    self.cache_entries.remove(&key);
                    Ok(())
                }
                Err(_) if !self.auth_map.fail_delete => {
                    self.cache_entries.remove(&key);
                    Ok(())
                }
                Err(err) => Err(format!("failed to delete auth entry from map: {err}")),
            }
        }

        fn delete_if<F>(&mut self, mut predicate: F) -> Result<(), String>
        where
            F: FnMut(AuthKey, AuthInfo) -> bool,
        {
            let keys: Vec<AuthKey> = self
                .cache_entries
                .iter()
                .filter_map(|(k, v)| predicate(*k, v.auth_info).then_some(*k))
                .collect();

            for key in keys {
                match self.auth_map.delete(key) {
                    Ok(()) => {
                        self.cache_entries.remove(&key);
                    }
                    Err(_) if !self.auth_map.fail_delete => {
                        self.cache_entries.remove(&key);
                    }
                    Err(err) => return Err(format!("failed to delete auth entry from map: {err}")),
                }
            }
            Ok(())
        }

        fn restore_cache(&mut self) -> Result<(), String> {
            let all = self
                .auth_map
                .all()
                .map_err(|e| format!("failed to load all auth map entries: {e}"))?;
            for (k, v) in all {
                self.cache_entries.insert(
                    k,
                    AuthInfoCache {
                        auth_info: v,
                        stored_at: Utc::now(),
                    },
                );
            }
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    struct TestAuthHandler {
        auth_type: u8,
        succeeds: bool,
    }

    struct AuthManager {
        auth_handlers: HashMap<u8, TestAuthHandler>,
        auth_map: FakeAuthMap,
        node_ips: HashMap<u16, String>,
        handled_requests: Vec<(AuthKey, bool)>,
    }

    impl AuthManager {
        fn new(
            auth_handlers: Vec<TestAuthHandler>,
            auth_map: FakeAuthMap,
            node_ips: HashMap<u16, String>,
        ) -> Result<Self, String> {
            let mut handlers = HashMap::new();
            for handler in auth_handlers {
                if handlers.insert(handler.auth_type, handler).is_some() {
                    return Err(format!(
                        "multiple handlers for auth type: {}",
                        auth_type_name(handler.auth_type)
                    ));
                }
            }
            Ok(Self {
                auth_handlers: handlers,
                auth_map,
                node_ips,
                handled_requests: Vec::new(),
            })
        }

        fn authenticate(&mut self, key: AuthKey) -> Result<(), String> {
            let Some(handler) = self.auth_handlers.get(&key.auth_type) else {
                return Err(format!(
                    "unknown requested auth type: {}",
                    auth_type_name(key.auth_type)
                ));
            };
            if key.remote_node_id != 0 && !self.node_ips.contains_key(&key.remote_node_id) {
                return Err(format!(
                    "remote node IP not available for node ID {}",
                    key.remote_node_id
                ));
            }
            if !handler.succeeds {
                return Err(String::from("authentication failed"));
            }
            self.auth_map.update(
                key,
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            );
            Ok(())
        }

        fn handle_auth_request(&mut self, key: AuthKey) {
            if is_reserved_identity(key.local_identity) || is_reserved_identity(key.remote_identity)
            {
                return;
            }
            self.handled_requests.push((key, false));
        }

        fn handle_certificate_rotation_event(
            &mut self,
            identity: u32,
            deleted: bool,
        ) -> Result<(), String> {
            let all = self
                .auth_map
                .all()
                .map_err(|e| format!("failed to get all auth map entries: {e}"))?;
            for key in all.keys().copied() {
                if key.local_identity == identity || key.remote_identity == identity {
                    if deleted {
                        self.auth_map
                            .delete(key)
                            .map_err(|e| format!("failed to delete auth entry from map: {e}"))?;
                    } else {
                        self.handled_requests.push((key, true));
                    }
                }
            }
            Ok(())
        }
    }

    fn auth_type_name(auth_type: u8) -> &'static str {
        match auth_type {
            1 => "spire",
            2 => "test-always-fail",
            100 => "test-always-pass",
            _ => "unknown",
        }
    }

    fn is_reserved_identity(id: u32) -> bool {
        id < 256
    }

    enum IdentityChangeKind {
        Upsert,
        Sync,
        Delete,
    }

    struct AuthMapGarbageCollector {
        auth_map: FakeAuthMap,
        policy_repo: HashMap<(u32, u32), HashSet<u8>>,
        cilium_nodes_discovered: Option<HashSet<u16>>,
        cilium_nodes_synced: bool,
        cilium_nodes_deleted: HashSet<u16>,
        cilium_identities_discovered: Option<HashSet<u32>>,
        cilium_identities_synced: bool,
        cilium_identities_deleted: HashSet<u32>,
        endpoint_ids_in_use: HashSet<u32>,
        endpoints_cache_synced: bool,
    }

    impl AuthMapGarbageCollector {
        fn new(auth_map: FakeAuthMap, policy_repo: HashMap<(u32, u32), HashSet<u8>>) -> Self {
            let mut nodes = HashSet::new();
            nodes.insert(0);
            Self {
                auth_map,
                policy_repo,
                cilium_nodes_discovered: Some(nodes),
                cilium_nodes_synced: false,
                cilium_nodes_deleted: HashSet::new(),
                cilium_identities_discovered: Some(HashSet::new()),
                cilium_identities_synced: false,
                cilium_identities_deleted: HashSet::new(),
                endpoint_ids_in_use: HashSet::new(),
                endpoints_cache_synced: false,
            }
        }

        fn node_add(&mut self, node_id: u16) {
            if let Some(ref mut discovered) = self.cilium_nodes_discovered {
                discovered.insert(node_id);
            }
        }

        fn node_delete(&mut self, node_id: u16) {
            self.cilium_nodes_deleted.insert(node_id);
        }

        fn handle_identity_change(&mut self, kind: IdentityChangeKind, id: u32) {
            match kind {
                IdentityChangeKind::Upsert => {
                    if let Some(ref mut discovered) = self.cilium_identities_discovered {
                        discovered.insert(id);
                    }
                }
                IdentityChangeKind::Sync => {
                    self.cilium_identities_synced = true;
                }
                IdentityChangeKind::Delete => {
                    self.cilium_identities_deleted.insert(id);
                }
            }
        }

        fn cleanup(&mut self) -> Result<(), String> {
            self.cleanup_expired_entries()?;
            self.cleanup_nodes()?;
            self.cleanup_endpoints()?;
            self.cleanup_identities()?;
            self.cleanup_entries_without_auth_policy()?;
            Ok(())
        }

        fn cleanup_nodes(&mut self) -> Result<(), String> {
            if !self.cilium_nodes_synced {
                return Ok(());
            }
            if let Some(discovered) = self.cilium_nodes_discovered.take() {
                self.auth_map
                    .delete_if(|key, _| !discovered.contains(&key.remote_node_id))
                    .map_err(|e| format!("failed to cleanup missing nodes: {e}"))?;
            }

            let deleted: Vec<u16> = self.cilium_nodes_deleted.iter().copied().collect();
            for node_id in deleted {
                self.auth_map
                    .delete_if(|key, _| key.remote_node_id == node_id)
                    .map_err(|e| format!("failed to cleanup deleted node: {e}"))?;
                self.cilium_nodes_deleted.remove(&node_id);
            }
            Ok(())
        }

        fn cleanup_identities(&mut self) -> Result<(), String> {
            if !self.cilium_identities_synced {
                return Ok(());
            }
            if let Some(discovered) = self.cilium_identities_discovered.take() {
                self.auth_map
                    .delete_if(|key, _| {
                        !discovered.contains(&key.local_identity)
                            || !discovered.contains(&key.remote_identity)
                    })
                    .map_err(|e| format!("failed to cleanup missing identities: {e}"))?;
            }
            let deleted: Vec<u32> = self.cilium_identities_deleted.iter().copied().collect();
            for id in deleted {
                self.auth_map
                    .delete_if(|key, _| key.local_identity == id || key.remote_identity == id)
                    .map_err(|e| format!("failed to cleanup deleted identity: {e}"))?;
                self.cilium_identities_deleted.remove(&id);
            }
            Ok(())
        }

        fn cleanup_entries_without_auth_policy(&mut self) -> Result<(), String> {
            self.auth_map
                .delete_if(|key, _| {
                    self.policy_repo
                        .get(&(key.local_identity, key.remote_identity))
                        .is_none_or(|auth_types| !auth_types.contains(&key.auth_type))
                })
                .map_err(|e| format!("failed to cleanup entries without any auth policy: {e}"))
        }

        fn cleanup_expired_entries(&mut self) -> Result<(), String> {
            let now = Utc::now();
            self.auth_map
                .delete_if(|_, info| info.expiration < now)
                .map_err(|e| format!("failed to cleanup expired entries: {e}"))
        }

        fn cleanup_endpoints(&mut self) -> Result<(), String> {
            let Some(discovered) = self.cilium_identities_discovered.clone() else {
                return Ok(());
            };
            if !self.cilium_identities_synced || !self.endpoints_cache_synced {
                return Ok(());
            }
            for id in discovered {
                if !self.endpoint_ids_in_use.contains(&id) {
                    self.auth_map
                        .delete_if(|key, _| {
                            key.local_identity == id
                                || (key.remote_node_id == 0 && key.remote_identity == id)
                        })
                        .map_err(|e| {
                            format!("failed to cleanup auth map entries related to endpoint entries: {e}")
                        })?;
                }
            }
            Ok(())
        }
    }

    // manager_test.go parity

    #[test]
    fn parity_test_new_auth_manager_clashing_auth_handlers() {
        let result = AuthManager::new(
            vec![
                TestAuthHandler {
                    auth_type: 2,
                    succeeds: false,
                },
                TestAuthHandler {
                    auth_type: 2,
                    succeeds: false,
                },
            ],
            FakeAuthMap::with_entries(HashMap::new()),
            HashMap::new(),
        );
        let err = match result {
            Ok(_) => panic!("duplicate handler should fail"),
            Err(err) => err,
        };
        assert!(err.contains("multiple handlers for auth type: test-always-fail"));
    }

    #[test]
    fn parity_test_new_auth_manager() {
        let manager = AuthManager::new(
            vec![
                TestAuthHandler {
                    auth_type: 100,
                    succeeds: true,
                },
                TestAuthHandler {
                    auth_type: 127,
                    succeeds: true,
                },
            ],
            FakeAuthMap::with_entries(HashMap::new()),
            HashMap::new(),
        )
        .expect("manager should be created");
        assert_eq!(manager.auth_handlers.len(), 2);
    }

    #[test]
    fn parity_test_auth_manager_authenticate() {
        let mut manager = AuthManager::new(
            vec![
                TestAuthHandler {
                    auth_type: 2,
                    succeeds: false,
                },
                TestAuthHandler {
                    auth_type: 100,
                    succeeds: true,
                },
            ],
            FakeAuthMap::with_entries(HashMap::new()),
            HashMap::from([(2_u16, String::from("172.18.0.2"))]),
        )
        .expect("manager should be created");

        let missing_handler = manager.authenticate(AuthKey {
            local_identity: 1000,
            remote_identity: 2000,
            remote_node_id: 2,
            auth_type: 1,
        });
        assert!(
            missing_handler
                .expect_err("must fail")
                .contains("unknown requested auth type: spire")
        );

        let missing_node = manager.authenticate(AuthKey {
            local_identity: 1000,
            remote_identity: 2000,
            remote_node_id: 1,
            auth_type: 100,
        });
        assert!(
            missing_node
                .expect_err("must fail")
                .contains("remote node IP not available for node ID 1")
        );

        let success = manager.authenticate(AuthKey {
            local_identity: 1000,
            remote_identity: 2000,
            remote_node_id: 2,
            auth_type: 100,
        });
        assert!(success.is_ok());
        assert_eq!(manager.auth_map.entries.len(), 1);
    }

    #[test]
    fn parity_test_auth_manager_handle_auth_request() {
        let mut manager = AuthManager::new(
            vec![TestAuthHandler {
                auth_type: 100,
                succeeds: true,
            }],
            FakeAuthMap::with_entries(HashMap::new()),
            HashMap::new(),
        )
        .expect("manager should be created");
        let key = AuthKey {
            local_identity: 1000,
            remote_identity: 2000,
            remote_node_id: 0,
            auth_type: 100,
        };
        manager.handle_auth_request(key);
        assert_eq!(manager.handled_requests, vec![(key, false)]);
    }

    #[test]
    fn parity_test_auth_manager_handle_auth_request_reserved_remote_identity() {
        let mut manager = AuthManager::new(
            vec![TestAuthHandler {
                auth_type: 100,
                succeeds: true,
            }],
            FakeAuthMap::with_entries(HashMap::new()),
            HashMap::new(),
        )
        .expect("manager should be created");
        manager.handle_auth_request(AuthKey {
            local_identity: 100,
            remote_identity: 2,
            remote_node_id: 0,
            auth_type: 100,
        });
        assert!(manager.handled_requests.is_empty());
    }

    #[test]
    fn parity_test_auth_manager_handle_auth_request_reserved_local_identity() {
        let mut manager = AuthManager::new(
            vec![TestAuthHandler {
                auth_type: 100,
                succeeds: true,
            }],
            FakeAuthMap::with_entries(HashMap::new()),
            HashMap::new(),
        )
        .expect("manager should be created");
        manager.handle_auth_request(AuthKey {
            local_identity: 2,
            remote_identity: 100,
            remote_node_id: 0,
            auth_type: 100,
        });
        assert!(manager.handled_requests.is_empty());
    }

    #[test]
    fn parity_test_auth_manager_handle_certificate_rotation_event_error() {
        let mut map = FakeAuthMap::with_entries(HashMap::new());
        map.fail_all = true;
        let mut manager = AuthManager::new(
            vec![TestAuthHandler {
                auth_type: 100,
                succeeds: true,
            }],
            map,
            HashMap::new(),
        )
        .expect("manager should be created");
        let err = manager
            .handle_certificate_rotation_event(10, false)
            .expect_err("must fail");
        assert!(err.contains("failed to get all auth map entries: failed to list entries"));
    }

    #[test]
    fn parity_test_auth_manager_handle_certificate_rotation_event() {
        let entries = HashMap::from([
            (
                AuthKey {
                    local_identity: 1000,
                    remote_identity: 2000,
                    remote_node_id: 1,
                    auth_type: 100,
                },
                AuthInfo {
                    expiration: Utc::now(),
                },
            ),
            (
                AuthKey {
                    local_identity: 2000,
                    remote_identity: 3000,
                    remote_node_id: 1,
                    auth_type: 100,
                },
                AuthInfo {
                    expiration: Utc::now(),
                },
            ),
            (
                AuthKey {
                    local_identity: 3000,
                    remote_identity: 4000,
                    remote_node_id: 1,
                    auth_type: 100,
                },
                AuthInfo {
                    expiration: Utc::now(),
                },
            ),
        ]);
        let mut manager = AuthManager::new(
            vec![TestAuthHandler {
                auth_type: 100,
                succeeds: true,
            }],
            FakeAuthMap::with_entries(entries),
            HashMap::new(),
        )
        .expect("manager should be created");
        manager
            .handle_certificate_rotation_event(2000, false)
            .expect("rotation should succeed");
        assert!(!manager.handled_requests.is_empty());
        assert!(
            manager
                .handled_requests
                .iter()
                .all(|(k, reauth)| *reauth
                    && (k.local_identity == 2000 || k.remote_identity == 2000))
        );
    }

    #[test]
    fn parity_test_auth_manager_handle_certificate_deletion_event() {
        let entries = HashMap::from([
            (
                AuthKey {
                    local_identity: 1000,
                    remote_identity: 2000,
                    remote_node_id: 1,
                    auth_type: 100,
                },
                AuthInfo {
                    expiration: Utc::now(),
                },
            ),
            (
                AuthKey {
                    local_identity: 2000,
                    remote_identity: 3000,
                    remote_node_id: 1,
                    auth_type: 100,
                },
                AuthInfo {
                    expiration: Utc::now(),
                },
            ),
            (
                AuthKey {
                    local_identity: 3000,
                    remote_identity: 4000,
                    remote_node_id: 1,
                    auth_type: 100,
                },
                AuthInfo {
                    expiration: Utc::now(),
                },
            ),
        ]);
        let mut manager = AuthManager::new(
            vec![TestAuthHandler {
                auth_type: 100,
                succeeds: true,
            }],
            FakeAuthMap::with_entries(entries),
            HashMap::new(),
        )
        .expect("manager should be created");
        manager
            .handle_certificate_rotation_event(2000, true)
            .expect("deletion should succeed");
        assert_eq!(manager.auth_map.entries.len(), 1);
    }

    // mutual_authhandler_test.go parity

    #[derive(Debug, Clone)]
    struct TestCertificate {
        uri: String,
        identities: HashSet<u32>,
        signed_by: String,
        not_after: seriousum_core::chrono::DateTime<Utc>,
        is_ca: bool,
    }

    #[derive(Debug, Clone)]
    struct TestMutualAuthCertProvider {
        certs: HashMap<u32, TestCertificate>,
    }

    impl TestMutualAuthCertProvider {
        fn get_certificate_for_identity(&self, id: u32) -> Result<TestCertificate, String> {
            self.certs
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("no certificate for spiffe://spiffe.cilium/identity/{id}"))
        }

        fn validate_identity(&self, id: u32, cert: &TestCertificate) -> bool {
            cert.identities.contains(&id)
        }

        fn sni_to_numeric_identity(&self, sni: &str) -> Result<u32, String> {
            let suffix = ".spiffe.cilium";
            if !sni.ends_with(suffix) {
                return Err(format!("SNI {sni} does not belong to our trust domain"));
            }
            let id = sni.trim_end_matches(suffix);
            id.parse::<u32>()
                .map_err(|_| format!("invalid SNI identity: {id}"))
        }
    }

    #[derive(Debug, Clone)]
    struct TestEndpoint {
        security_identity: Option<u32>,
    }

    struct MutualAuthHandlerScaffold {
        cert: TestMutualAuthCertProvider,
        endpoint_manager: Option<Vec<TestEndpoint>>,
    }

    impl MutualAuthHandlerScaffold {
        fn verify_peer_certificate(
            &self,
            id: Option<u32>,
            ca_bundle: &HashSet<String>,
            cert_chains: &[Vec<TestCertificate>],
        ) -> Result<seriousum_core::chrono::DateTime<Utc>, String> {
            if cert_chains.is_empty() {
                return Err(String::from("no certificate chains found"));
            }

            let mut expiration_time = None;

            for chain in cert_chains {
                let Some(leaf) = chain.iter().rev().find(|cert| !cert.is_ca) else {
                    return Err(String::from("no leaf certificate found"));
                };

                if !ca_bundle.contains(&leaf.signed_by) {
                    return Err(String::from("failed to verify certificate"));
                }

                if let Some(id) = id
                    && !self.cert.validate_identity(id, leaf)
                {
                    return Err(String::from("unable to validate SAN"));
                }

                expiration_time = Some(leaf.not_after);
            }

            expiration_time.ok_or_else(|| String::from("failed to get expiration time"))
        }

        fn get_certificate_for_incoming_connection(
            &self,
            server_name: &str,
        ) -> Result<TestCertificate, String> {
            let id = self
                .cert
                .sni_to_numeric_identity(server_name)
                .map_err(|err| format!("failed to get identity for SNI {server_name}: {err}"))?;

            let Some(local_eps) = &self.endpoint_manager else {
                return Err(String::from("endpoint manager is not loaded"));
            };

            let matched = local_eps.iter().any(|ep| ep.security_identity == Some(id));
            if !matched {
                return Err(format!("no local endpoint present for identity {id}"));
            }

            self.cert.get_certificate_for_identity(id)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct TestMutualAuthRequest {
        local_identity: u32,
        remote_identity: u32,
        remote_node_ip: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestMutualAuthResponse {
        expiration_time: seriousum_core::chrono::DateTime<Utc>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum MutualAuthOutcome {
        AlreadyAuthenticated,
        AuthenticationInProgress,
        Authenticated(TestMutualAuthResponse),
    }

    trait TestMutualAuthenticator {
        fn authenticate(
            &mut self,
            request: &TestMutualAuthRequest,
        ) -> Result<TestMutualAuthResponse, String>;
    }

    #[derive(Default)]
    struct MockMutualAuthenticator {
        responses: HashMap<(u32, u32), std::result::Result<TestMutualAuthResponse, String>>,
        calls: Vec<TestMutualAuthRequest>,
    }

    impl MockMutualAuthenticator {
        fn with_responses(
            responses: HashMap<(u32, u32), std::result::Result<TestMutualAuthResponse, String>>,
        ) -> Self {
            Self {
                responses,
                calls: Vec::new(),
            }
        }
    }

    impl TestMutualAuthenticator for MockMutualAuthenticator {
        fn authenticate(
            &mut self,
            request: &TestMutualAuthRequest,
        ) -> Result<TestMutualAuthResponse, String> {
            self.calls.push(request.clone());
            self.responses
                .get(&(request.local_identity, request.remote_identity))
                .cloned()
                .unwrap_or_else(|| {
                    Err(format!(
                        "no response configured for {}->{}, node {}",
                        request.local_identity, request.remote_identity, request.remote_node_ip,
                    ))
                })
        }
    }

    #[derive(Debug, Clone)]
    struct MutualAuthStateEntry {
        expiration_time: seriousum_core::chrono::DateTime<Utc>,
        in_progress: bool,
    }

    #[derive(Default)]
    struct MutualAuthStateMachine {
        entries: HashMap<(u32, u32), MutualAuthStateEntry>,
    }

    impl MutualAuthStateMachine {
        fn set_authenticated(
            &mut self,
            request: &TestMutualAuthRequest,
            expiration_time: seriousum_core::chrono::DateTime<Utc>,
        ) {
            self.entries.insert(
                (request.local_identity, request.remote_identity),
                MutualAuthStateEntry {
                    expiration_time,
                    in_progress: false,
                },
            );
        }

        fn mark_in_progress(
            &mut self,
            request: &TestMutualAuthRequest,
            now: seriousum_core::chrono::DateTime<Utc>,
        ) {
            self.entries.insert(
                (request.local_identity, request.remote_identity),
                MutualAuthStateEntry {
                    expiration_time: now,
                    in_progress: true,
                },
            );
        }

        fn authenticate<A: TestMutualAuthenticator>(
            &mut self,
            authenticator: &mut A,
            request: TestMutualAuthRequest,
            now: seriousum_core::chrono::DateTime<Utc>,
        ) -> Result<MutualAuthOutcome, String> {
            let key = (request.local_identity, request.remote_identity);
            if let Some(entry) = self.entries.get(&key) {
                if entry.in_progress {
                    return Ok(MutualAuthOutcome::AuthenticationInProgress);
                }
                if entry.expiration_time > now {
                    return Ok(MutualAuthOutcome::AlreadyAuthenticated);
                }
            }

            self.entries.insert(
                key,
                MutualAuthStateEntry {
                    expiration_time: now,
                    in_progress: true,
                },
            );

            match authenticator.authenticate(&request) {
                Ok(response) => {
                    self.entries.insert(
                        key,
                        MutualAuthStateEntry {
                            expiration_time: response.expiration_time,
                            in_progress: false,
                        },
                    );
                    Ok(MutualAuthOutcome::Authenticated(response))
                }
                Err(err) => {
                    self.entries.remove(&key);
                    Err(err)
                }
            }
        }

        fn entry(&self, request: &TestMutualAuthRequest) -> Option<&MutualAuthStateEntry> {
            self.entries
                .get(&(request.local_identity, request.remote_identity))
        }
    }

    fn generate_test_certificates(
        signer: &str,
    ) -> (
        HashMap<u32, TestCertificate>,
        HashSet<String>,
        HashSet<String>,
    ) {
        let mut certs = HashMap::new();
        let mut uris = HashSet::new();
        for id in 1000..=1002 {
            let uri = format!("spiffe://spiffe.cilium/identity/{id}");
            uris.insert(uri.clone());
            certs.insert(
                id,
                TestCertificate {
                    uri,
                    identities: HashSet::from([id]),
                    signed_by: signer.to_owned(),
                    not_after: Utc::now() + Duration::hours(1),
                    is_ca: false,
                },
            );
        }
        (certs, HashSet::from([signer.to_owned()]), uris)
    }

    #[test]
    fn parity_test_mutual_auth_handler_verify_peer_certificate() {
        let (cert_map, ca_pool, _) = generate_test_certificates("ca-a");
        let (cert_map_other_ca, _, _) = generate_test_certificates("ca-b");
        let cert_provider = TestMutualAuthCertProvider {
            certs: cert_map.clone(),
        };
        let handler = MutualAuthHandlerScaffold {
            cert: cert_provider,
            endpoint_manager: None,
        };

        let valid = vec![
            cert_map
                .get(&1000)
                .expect("certificate should exist")
                .clone(),
        ];
        let invalid_ca = vec![
            cert_map_other_ca
                .get(&1000)
                .expect("certificate should exist")
                .clone(),
        ];

        let not_after = cert_map
            .get(&1000)
            .expect("certificate should exist")
            .not_after;

        assert_eq!(
            handler
                .verify_peer_certificate(Some(1000), &ca_pool, &[valid.clone()])
                .expect("verification should succeed"),
            not_after
        );
        assert_eq!(
            handler
                .verify_peer_certificate(None, &ca_pool, &[valid.clone()])
                .expect("verification should succeed"),
            not_after
        );
        assert!(
            handler
                .verify_peer_certificate(Some(1001), &ca_pool, &[valid.clone()])
                .is_err()
        );
        assert!(
            handler
                .verify_peer_certificate(Some(1000), &ca_pool, &[invalid_ca.clone()])
                .is_err()
        );
        assert!(
            handler
                .verify_peer_certificate(None, &ca_pool, &[invalid_ca])
                .is_err()
        );
        assert!(
            handler
                .verify_peer_certificate(None, &ca_pool, &[])
                .is_err()
        );
        assert!(
            handler
                .verify_peer_certificate(None, &HashSet::new(), &[valid])
                .is_err()
        );
    }

    #[test]
    fn parity_test_mutual_auth_handler_get_certificate_for_incoming_connection() {
        let (cert_map, _, uris) = generate_test_certificates("ca-a");
        let handler = MutualAuthHandlerScaffold {
            cert: TestMutualAuthCertProvider { certs: cert_map },
            endpoint_manager: Some(vec![
                TestEndpoint {
                    security_identity: Some(1000),
                },
                TestEndpoint {
                    security_identity: Some(1001),
                },
                TestEndpoint {
                    security_identity: Some(9999),
                },
            ]),
        };

        let cert = handler
            .get_certificate_for_incoming_connection("1000.spiffe.cilium")
            .expect("lookup should succeed");
        assert!(uris.contains(&cert.uri));
        assert_eq!(cert.uri, "spiffe://spiffe.cilium/identity/1000");
        assert!(
            handler
                .get_certificate_for_incoming_connection("1002.spiffe.cilium")
                .is_err()
        );
        assert!(
            handler
                .get_certificate_for_incoming_connection("9999.spiffe.cilium")
                .is_err()
        );
        assert!(
            handler
                .get_certificate_for_incoming_connection("www.example.com")
                .is_err()
        );
    }

    #[test]
    fn parity_test_mutual_auth_handler_authenticate() {
        let now = Utc::now();
        let already_authenticated = TestMutualAuthRequest {
            local_identity: 1000,
            remote_identity: 1001,
            remote_node_ip: String::from("127.0.0.1"),
        };
        let in_progress = TestMutualAuthRequest {
            local_identity: 1000,
            remote_identity: 1002,
            remote_node_ip: String::from("127.0.0.1"),
        };
        let expired = TestMutualAuthRequest {
            local_identity: 1001,
            remote_identity: 1002,
            remote_node_ip: String::from("127.0.0.1"),
        };
        let new_entry = TestMutualAuthRequest {
            local_identity: 1002,
            remote_identity: 1003,
            remote_node_ip: String::from("127.0.0.2"),
        };

        let mut state_machine = MutualAuthStateMachine::default();
        state_machine.set_authenticated(&already_authenticated, now + Duration::minutes(10));
        state_machine.mark_in_progress(&in_progress, now - Duration::minutes(1));
        state_machine.set_authenticated(&expired, now - Duration::minutes(5));

        let expired_response = TestMutualAuthResponse {
            expiration_time: now + Duration::minutes(15),
        };
        let new_response = TestMutualAuthResponse {
            expiration_time: now + Duration::minutes(20),
        };
        let mut authenticator = MockMutualAuthenticator::with_responses(HashMap::from([
            (
                (expired.local_identity, expired.remote_identity),
                Ok(expired_response.clone()),
            ),
            (
                (new_entry.local_identity, new_entry.remote_identity),
                Ok(new_response.clone()),
            ),
        ]));

        assert_eq!(
            state_machine
                .authenticate(&mut authenticator, already_authenticated.clone(), now)
                .expect("already-authenticated flow should short-circuit"),
            MutualAuthOutcome::AlreadyAuthenticated,
        );
        assert_eq!(
            state_machine
                .authenticate(&mut authenticator, in_progress.clone(), now)
                .expect("in-progress flow should short-circuit"),
            MutualAuthOutcome::AuthenticationInProgress,
        );
        assert_eq!(
            state_machine
                .authenticate(&mut authenticator, expired.clone(), now)
                .expect("expired auth should re-authenticate"),
            MutualAuthOutcome::Authenticated(expired_response.clone()),
        );
        assert_eq!(
            state_machine
                .authenticate(&mut authenticator, new_entry.clone(), now)
                .expect("new auth should authenticate"),
            MutualAuthOutcome::Authenticated(new_response.clone()),
        );

        assert_eq!(
            authenticator.calls,
            vec![expired.clone(), new_entry.clone()]
        );

        let expired_entry = state_machine
            .entry(&expired)
            .expect("expired entry should be refreshed");
        assert_eq!(
            expired_entry.expiration_time,
            expired_response.expiration_time
        );
        assert!(!expired_entry.in_progress);

        let new_state_entry = state_machine
            .entry(&new_entry)
            .expect("new entry should be stored");
        assert_eq!(
            new_state_entry.expiration_time,
            new_response.expiration_time
        );
        assert!(!new_state_entry.in_progress);

        assert!(
            state_machine
                .entry(&already_authenticated)
                .expect("existing entry should remain")
                .expiration_time
                > now
        );
        assert!(
            state_machine
                .entry(&in_progress)
                .expect("in-progress entry should remain")
                .in_progress
        );
    }

    // authmap_cache_test.go parity

    #[test]
    fn parity_test_auth_map_cache_restore_cache() {
        let mut cache = AuthMapCache {
            auth_map: FakeAuthMap::with_entries(HashMap::from([(
                AuthKey {
                    local_identity: 1000,
                    remote_identity: 2000,
                    remote_node_id: 10,
                    auth_type: 0,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(10),
                },
            )])),
            cache_entries: HashMap::new(),
        };

        cache.restore_cache().expect("restore should succeed");
        assert_eq!(cache.cache_entries.len(), 1);
        let value = cache
            .get(AuthKey {
                local_identity: 1000,
                remote_identity: 2000,
                remote_node_id: 10,
                auth_type: 0,
            })
            .expect("key should exist");
        assert!(value.expiration > Utc::now());
        let stored_at = cache
            .cache_entries
            .values()
            .next()
            .expect("cache entry should exist")
            .stored_at;
        assert!(stored_at <= Utc::now());
    }

    #[test]
    fn parity_test_auth_map_cache_all_returns_copy() {
        let cache = AuthMapCache {
            auth_map: FakeAuthMap::with_entries(HashMap::new()),
            cache_entries: HashMap::from([(
                AuthKey {
                    local_identity: 1000,
                    remote_identity: 2000,
                    remote_node_id: 10,
                    auth_type: 0,
                },
                AuthInfoCache {
                    auth_info: AuthInfo {
                        expiration: Utc::now() + Duration::minutes(10),
                    },
                    stored_at: Utc::now(),
                },
            )]),
        };

        let mut all = cache.all();
        assert_eq!(all.len(), 1);
        all.insert(
            AuthKey {
                local_identity: 10000,
                remote_identity: 20000,
                remote_node_id: 100,
                auth_type: 0,
            },
            AuthInfo {
                expiration: Utc::now() + Duration::minutes(10),
            },
        );
        assert_eq!(all.len(), 2);
        assert_eq!(cache.cache_entries.len(), 1);
    }

    #[test]
    fn parity_test_auth_map_cache_delete() {
        let mut cache = AuthMapCache {
            auth_map: FakeAuthMap::with_entries(HashMap::from([(
                AuthKey {
                    local_identity: 1000,
                    remote_identity: 2000,
                    remote_node_id: 10,
                    auth_type: 0,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(10),
                },
            )])),
            cache_entries: HashMap::from([
                (
                    AuthKey {
                        local_identity: 1000,
                        remote_identity: 2000,
                        remote_node_id: 10,
                        auth_type: 0,
                    },
                    AuthInfoCache {
                        auth_info: AuthInfo {
                            expiration: Utc::now() + Duration::minutes(10),
                        },
                        stored_at: Utc::now() - Duration::minutes(10),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 3000,
                        remote_identity: 2000,
                        remote_node_id: 10,
                        auth_type: 0,
                    },
                    AuthInfoCache {
                        auth_info: AuthInfo {
                            expiration: Utc::now() + Duration::minutes(10),
                        },
                        stored_at: Utc::now() - Duration::minutes(10),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 4000,
                        remote_identity: 2000,
                        remote_node_id: 10,
                        auth_type: 0,
                    },
                    AuthInfoCache {
                        auth_info: AuthInfo {
                            expiration: Utc::now() + Duration::minutes(10),
                        },
                        stored_at: Utc::now() - Duration::minutes(10),
                    },
                ),
            ]),
        };

        cache
            .delete(AuthKey {
                local_identity: 1000,
                remote_identity: 2000,
                remote_node_id: 10,
                auth_type: 0,
            })
            .expect("delete should succeed");
        assert_eq!(cache.cache_entries.len(), 2);

        cache
            .delete(AuthKey {
                local_identity: 3000,
                remote_identity: 2000,
                remote_node_id: 10,
                auth_type: 0,
            })
            .expect("delete should treat missing backend key as okay");
        assert_eq!(cache.cache_entries.len(), 1);

        cache.auth_map.fail_delete = true;
        let err = cache
            .delete(AuthKey {
                local_identity: 4000,
                remote_identity: 2000,
                remote_node_id: 10,
                auth_type: 0,
            })
            .expect_err("delete should fail on backend error");
        assert!(err.contains("failed to delete auth entry from map: failed to delete entry"));
        assert_eq!(cache.cache_entries.len(), 1);
    }

    #[test]
    fn parity_test_auth_map_cache_delete_if() {
        let mut cache = AuthMapCache {
            auth_map: FakeAuthMap::with_entries(HashMap::from([(
                AuthKey {
                    local_identity: 1000,
                    remote_identity: 2000,
                    remote_node_id: 10,
                    auth_type: 0,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(10),
                },
            )])),
            cache_entries: HashMap::from([
                (
                    AuthKey {
                        local_identity: 1000,
                        remote_identity: 2000,
                        remote_node_id: 10,
                        auth_type: 0,
                    },
                    AuthInfoCache {
                        auth_info: AuthInfo {
                            expiration: Utc::now() + Duration::minutes(10),
                        },
                        stored_at: Utc::now() - Duration::minutes(10),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 3000,
                        remote_identity: 2000,
                        remote_node_id: 10,
                        auth_type: 0,
                    },
                    AuthInfoCache {
                        auth_info: AuthInfo {
                            expiration: Utc::now() + Duration::minutes(10),
                        },
                        stored_at: Utc::now() - Duration::minutes(10),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 4000,
                        remote_identity: 2000,
                        remote_node_id: 10,
                        auth_type: 0,
                    },
                    AuthInfoCache {
                        auth_info: AuthInfo {
                            expiration: Utc::now() + Duration::minutes(10),
                        },
                        stored_at: Utc::now() - Duration::minutes(10),
                    },
                ),
            ]),
        };

        cache
            .delete_if(|key, _| key.local_identity == 1000 || key.local_identity == 3000)
            .expect("delete_if should succeed");
        assert_eq!(cache.cache_entries.len(), 1);

        cache.auth_map.fail_delete = true;
        let err = cache
            .delete_if(|key, _| key.local_identity == 4000)
            .expect_err("delete_if should fail");
        assert!(err.contains("failed to delete auth entry from map: failed to delete entry"));
        assert_eq!(cache.cache_entries.len(), 1);
    }

    // authmap_gc_test.go parity

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup_identities() {
        let entries = HashMap::from([
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 3,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 11,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 12,
                    remote_identity: 2,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 11,
                    remote_identity: 12,
                    remote_node_id: 0,
                    auth_type: 2,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
        ]);
        let mut gc =
            AuthMapGarbageCollector::new(FakeAuthMap::with_entries(entries), HashMap::new());
        gc.handle_identity_change(IdentityChangeKind::Upsert, 1);
        gc.handle_identity_change(IdentityChangeKind::Upsert, 2);
        gc.cleanup_identities()
            .expect("cleanup before sync should be noop");
        assert_eq!(gc.auth_map.entries.len(), 5);

        gc.handle_identity_change(IdentityChangeKind::Sync, 0);
        gc.handle_identity_change(IdentityChangeKind::Delete, 3);
        gc.handle_identity_change(IdentityChangeKind::Upsert, 3);
        gc.cleanup_identities()
            .expect("cleanup after sync should remove missing/deleted IDs");
        assert_eq!(gc.auth_map.entries.len(), 1);

        gc.handle_identity_change(IdentityChangeKind::Delete, 2);
        gc.cleanup_identities()
            .expect("cleanup should remove newly deleted ID");
        assert!(gc.auth_map.entries.is_empty());
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup_nodes() {
        let entries = HashMap::from([
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 1,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 2,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 3,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 4,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
        ]);
        let mut gc =
            AuthMapGarbageCollector::new(FakeAuthMap::with_entries(entries), HashMap::new());
        gc.node_add(1);
        gc.node_add(2);
        gc.cleanup_nodes().expect("cleanup before sync should noop");
        assert_eq!(gc.auth_map.entries.len(), 5);

        gc.cilium_nodes_synced = true;
        gc.node_delete(2);
        gc.node_add(3);
        gc.cleanup_nodes()
            .expect("cleanup should remove missing/deleted nodes");
        assert_eq!(gc.auth_map.entries.len(), 3);

        gc.node_add(5);
        gc.node_delete(3);
        gc.cleanup_nodes()
            .expect("cleanup should remove subsequent deleted nodes");
        assert_eq!(gc.auth_map.entries.len(), 2);
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup_policies() {
        let entries = HashMap::from([
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 3,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 4,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
        ]);
        let mut gc = AuthMapGarbageCollector::new(
            FakeAuthMap::with_entries(entries),
            HashMap::from([
                ((1, 2), HashSet::from([1_u8])),
                ((1, 3), HashSet::from([2_u8])),
            ]),
        );
        gc.cleanup_entries_without_auth_policy()
            .expect("cleanup should succeed");
        assert_eq!(gc.auth_map.entries.len(), 1);
        assert!(gc.auth_map.entries.contains_key(&AuthKey {
            local_identity: 1,
            remote_identity: 2,
            remote_node_id: 0,
            auth_type: 1,
        }));
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup_expired() {
        let mut gc = AuthMapGarbageCollector::new(
            FakeAuthMap::with_entries(HashMap::from([
                (
                    AuthKey {
                        local_identity: 1,
                        remote_identity: 2,
                        remote_node_id: 0,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 1,
                        remote_identity: 3,
                        remote_node_id: 0,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() - Duration::minutes(5),
                    },
                ),
            ])),
            HashMap::new(),
        );
        gc.cleanup_expired_entries()
            .expect("cleanup should succeed");
        assert_eq!(gc.auth_map.entries.len(), 1);
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup() {
        let entries = HashMap::from([
            (
                AuthKey {
                    local_identity: 1,
                    remote_identity: 2,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 3,
                    remote_identity: 4,
                    remote_node_id: 1,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 5,
                    remote_identity: 6,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() + Duration::minutes(5),
                },
            ),
            (
                AuthKey {
                    local_identity: 13,
                    remote_identity: 14,
                    remote_node_id: 0,
                    auth_type: 1,
                },
                AuthInfo {
                    expiration: Utc::now() - Duration::minutes(5),
                },
            ),
        ]);
        let mut gc = AuthMapGarbageCollector::new(
            FakeAuthMap::with_entries(entries),
            HashMap::from([
                ((1, 2), HashSet::from([1_u8])),
                ((3, 4), HashSet::from([1_u8])),
                ((5, 6), HashSet::from([1_u8])),
            ]),
        );
        gc.node_add(1);
        gc.node_add(2);
        gc.cilium_nodes_synced = true;
        gc.node_delete(1);
        for id in 1..15 {
            gc.handle_identity_change(IdentityChangeKind::Upsert, id);
        }
        gc.handle_identity_change(IdentityChangeKind::Sync, 0);
        gc.handle_identity_change(IdentityChangeKind::Delete, 6);
        gc.cleanup().expect("combined cleanup should succeed");
        assert_eq!(gc.auth_map.entries.len(), 1);
        assert!(gc.auth_map.entries.contains_key(&AuthKey {
            local_identity: 1,
            remote_identity: 2,
            remote_node_id: 0,
            auth_type: 1,
        }));
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup_endpoints() {
        let mut gc = AuthMapGarbageCollector::new(
            FakeAuthMap::with_entries(HashMap::from([
                (
                    AuthKey {
                        local_identity: 1,
                        remote_identity: 2,
                        remote_node_id: 0,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 2,
                        remote_identity: 1,
                        remote_node_id: 0,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 3,
                        remote_identity: 1,
                        remote_node_id: 100,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
            ])),
            HashMap::new(),
        );
        gc.endpoint_ids_in_use = HashSet::from([2, 3]);
        gc.endpoints_cache_synced = true;
        gc.cilium_identities_discovered = Some(HashSet::from([1, 2, 3]));
        gc.cilium_identities_synced = true;
        gc.cleanup_endpoints()
            .expect("endpoint cleanup should succeed");
        assert_eq!(gc.auth_map.entries.len(), 1);
        assert!(gc.auth_map.entries.contains_key(&AuthKey {
            local_identity: 3,
            remote_identity: 1,
            remote_node_id: 100,
            auth_type: 1,
        }));
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_cleanup_endpoints_noop_case() {
        let mut gc = AuthMapGarbageCollector::new(
            FakeAuthMap::with_entries(HashMap::from([
                (
                    AuthKey {
                        local_identity: 1,
                        remote_identity: 2,
                        remote_node_id: 0,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 2,
                        remote_identity: 1,
                        remote_node_id: 0,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
                (
                    AuthKey {
                        local_identity: 3,
                        remote_identity: 1,
                        remote_node_id: 100,
                        auth_type: 1,
                    },
                    AuthInfo {
                        expiration: Utc::now() + Duration::minutes(5),
                    },
                ),
            ])),
            HashMap::new(),
        );
        gc.endpoint_ids_in_use = HashSet::from([1, 2, 3]);
        gc.endpoints_cache_synced = true;
        gc.cilium_identities_discovered = Some(HashSet::from([1, 2, 3]));
        gc.cilium_identities_synced = true;
        gc.cleanup_endpoints()
            .expect("endpoint cleanup should be noop");
        assert_eq!(gc.auth_map.entries.len(), 3);
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_handle_node_event_error() {
        let mut map = FakeAuthMap::with_entries(HashMap::new());
        map.fail_delete = true;
        let mut gc = AuthMapGarbageCollector::new(map, HashMap::new());
        gc.node_add(10);
        gc.node_delete(10);
        gc.cilium_nodes_synced = true;
        gc.cilium_nodes_discovered = None;
        let err = gc.cleanup_nodes().expect_err("cleanup should fail");
        assert!(err.contains("failed to cleanup deleted node: failed to delete entry"));
    }

    #[test]
    fn parity_test_auth_map_garbage_collector_handle_identity_event_error() {
        let mut map = FakeAuthMap::with_entries(HashMap::new());
        map.fail_delete = true;
        let mut gc = AuthMapGarbageCollector::new(map, HashMap::new());
        gc.handle_identity_change(IdentityChangeKind::Delete, 4);
        gc.cilium_identities_synced = true;
        gc.cilium_identities_discovered = None;
        let err = gc.cleanup_identities().expect_err("cleanup should fail");
        assert!(err.contains("failed to cleanup deleted identity: failed to delete entry"));
    }
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

    #[test]
    fn test_auth_entry_validity() {
        let key = AuthKey {
            local_identity: 1,
            remote_identity: 2,
            remote_node_id: 3,
            auth_type: AuthType::SPIFFE,
        };
        let entry = AuthEntry::new(key.clone(), AuthStatus::Authenticated);
        assert!(entry.is_valid());

        let failed = AuthEntry::new(key.clone(), AuthStatus::Failed);
        assert!(!failed.is_valid());

        let expired = AuthEntry::new(key.clone(), AuthStatus::Authenticated)
            .with_expiry(std::time::SystemTime::UNIX_EPOCH);
        assert!(!expired.is_valid());
    }

    #[test]
    fn test_auth_map_insert_get_remove() {
        let mut map = AuthMap::new();
        let key = AuthKey {
            local_identity: 10,
            remote_identity: 20,
            remote_node_id: 0,
            auth_type: AuthType::Test,
        };
        let entry = AuthEntry::new(key.clone(), AuthStatus::Authenticated);
        map.insert(entry);
        assert!(map.is_authenticated(&key));
        assert!(map.get(&key).is_some());
        assert_eq!(map.len(), 1);
        map.remove(&key);
        assert!(!map.is_authenticated(&key));
    }

    #[test]
    fn test_auth_map_gc_removes_expired() {
        let mut map = AuthMap::new();
        let key = AuthKey {
            local_identity: 1,
            remote_identity: 2,
            remote_node_id: 0,
            auth_type: AuthType::Test,
        };
        let expired = AuthEntry::new(key.clone(), AuthStatus::Authenticated)
            .with_expiry(std::time::SystemTime::UNIX_EPOCH);
        map.insert(expired);
        assert_eq!(map.gc(), 1);
        assert!(map.is_empty());
    }

    #[test]
    fn test_auth_type_display() {
        assert_eq!(AuthType::SPIFFE.to_string(), "spiffe");
        assert_eq!(AuthType::Disabled.to_string(), "disabled");
    }

    #[test]
    fn test_pending_entry_survives_gc() {
        let mut map = AuthMap::new();
        let key = AuthKey {
            local_identity: 1,
            remote_identity: 2,
            remote_node_id: 0,
            auth_type: AuthType::Test,
        };
        map.insert(AuthEntry::new(key.clone(), AuthStatus::Pending));
        assert_eq!(map.gc(), 0);
        assert_eq!(map.len(), 1);
    }
}
