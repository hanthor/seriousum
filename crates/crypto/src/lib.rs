//! Crypto helpers for seriousum.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A SHA-256 fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Creates a fingerprint from raw bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw fingerprint bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Computes a SHA-256 fingerprint.
    pub fn sha256(data: &[u8]) -> Self {
        let d = digest(&SHA256, data);
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(d.as_ref());
        Self(bytes)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A raw 32-byte symmetric key used by features such as WireGuard and IPsec.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymmetricKey([u8; 32]);

impl SymmetricKey {
    /// Creates a new symmetric key from raw bytes.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Creates a symmetric key from a 32-byte slice.
    pub fn from_slice(s: &[u8]) -> Option<Self> {
        let arr: [u8; 32] = s.try_into().ok()?;
        Some(Self(arr))
    }

    /// Returns the raw key bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the key fingerprint.
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::sha256(&self.0)
    }

    /// Encodes the key as a lowercase hex string.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parses a key from a 64-character hexadecimal string.
    pub fn from_hex(s: &str) -> Result<Self, CryptoError> {
        if s.len() != 64 {
            return Err(CryptoError::InvalidKeyLength(s.len()));
        }

        let mut bytes = [0_u8; 32];
        for (index, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
            let pair =
                std::str::from_utf8(chunk).map_err(|_| CryptoError::InvalidHex(s.to_string()))?;
            bytes[index] =
                u8::from_str_radix(pair, 16).map_err(|_| CryptoError::InvalidHex(s.to_string()))?;
        }

        Ok(Self(bytes))
    }
}

impl fmt::Debug for SymmetricKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SymmetricKey([REDACTED])")
    }
}

impl fmt::Display for SymmetricKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Parsed TLS certificate metadata without the original certificate bytes.
#[derive(Debug, Clone)]
pub struct CertInfo {
    /// Subject distinguished name.
    pub subject: String,
    /// Issuer distinguished name.
    pub issuer: String,
    /// Earliest time the certificate is valid.
    pub not_before: SystemTime,
    /// Latest time the certificate is valid.
    pub not_after: SystemTime,
    /// Hexadecimal serial number.
    pub serial: String,
    /// Whether the certificate can act as a certificate authority.
    pub is_ca: bool,
    /// DNS subject alternative names.
    pub dns_names: Vec<String>,
    /// IP subject alternative names.
    pub ip_addresses: Vec<std::net::IpAddr>,
}

impl CertInfo {
    /// Returns true when the certificate validity window has elapsed.
    pub fn is_expired(&self) -> bool {
        SystemTime::now() > self.not_after
    }

    /// Returns true when the certificate covers the provided hostname.
    pub fn is_valid_for(&self, hostname: &str) -> bool {
        self.dns_names.iter().any(|name| {
            if name.starts_with("*.") {
                hostname.ends_with(&name[1..])
            } else {
                name == hostname
            }
        })
    }

    /// Returns the remaining certificate lifetime.
    pub fn expires_in(&self) -> Option<Duration> {
        self.not_after.duration_since(SystemTime::now()).ok()
    }
}

/// TLS configuration metadata for a Cilium component.
#[derive(Debug, Clone, Default)]
pub struct TLSConfig {
    /// Expected peer server name for certificate verification.
    pub server_name: Option<String>,
    /// Path to the trusted CA certificate bundle.
    pub ca_cert_path: Option<String>,
    /// Path to the client certificate used for mTLS.
    pub client_cert_path: Option<String>,
    /// Path to the client private key used for mTLS.
    pub client_key_path: Option<String>,
    /// Whether peer verification should be skipped.
    pub skip_verify: bool,
    /// Minimum TLS protocol version.
    pub min_version: TLSVersion,
}

/// Supported TLS protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TLSVersion {
    /// TLS 1.2.
    TLS12,
    /// TLS 1.3.
    #[default]
    TLS13,
}

impl TLSConfig {
    /// Creates a default TLS configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the expected server name.
    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// Sets whether certificate verification should be skipped.
    pub fn with_skip_verify(mut self, skip: bool) -> Self {
        self.skip_verify = skip;
        self
    }

    /// Returns true when both client certificate and key paths are configured.
    pub fn is_mtls(&self) -> bool {
        self.client_cert_path.is_some() && self.client_key_path.is_some()
    }
}

/// IPsec key material paired with a Cilium key slot identifier.
#[derive(Debug, Clone)]
pub struct IPsecKey {
    /// Key slot identifier in the range 1-255.
    pub key_id: u8,
    /// Authentication key material.
    pub auth_key: SymmetricKey,
    /// Encryption key material.
    pub enc_key: SymmetricKey,
    /// Cipher suite used with the key material.
    pub cipher_suite: IPsecCipher,
}

/// Supported IPsec cipher suites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IPsecCipher {
    /// AES-GCM with a 128-bit key.
    AES128GCM,
    /// AES-GCM with a 256-bit key.
    AES256GCM,
    /// ChaCha20-Poly1305.
    ChaCha20Poly1305,
}

impl IPsecKey {
    /// Creates an IPsec key if the slot identifier is valid.
    pub fn new(
        key_id: u8,
        auth_key: SymmetricKey,
        enc_key: SymmetricKey,
        cipher_suite: IPsecCipher,
    ) -> Result<Self, CryptoError> {
        if key_id == 0 {
            return Err(CryptoError::InvalidKeyId);
        }

        Ok(Self {
            key_id,
            auth_key,
            enc_key,
            cipher_suite,
        })
    }
}

/// Tracks active and previous keys during key rotation.
#[derive(Debug, Clone, Default)]
pub struct KeyRotationState {
    /// Currently active key identifier.
    pub current_key_id: u8,
    /// Previously active key identifier retained during transition.
    pub previous_key_id: Option<u8>,
    /// Monotonic rotation epoch.
    pub rotation_epoch: u64,
}

impl KeyRotationState {
    /// Creates a new key rotation state with an initial key identifier.
    pub fn new(initial_key_id: u8) -> Self {
        Self {
            current_key_id: initial_key_id,
            previous_key_id: None,
            rotation_epoch: 0,
        }
    }

    /// Rotates the active key to a new identifier and increments the epoch.
    pub fn rotate(&mut self, new_key_id: u8) {
        self.previous_key_id = Some(self.current_key_id);
        self.current_key_id = new_key_id;
        self.rotation_epoch += 1;
    }

    /// Returns true when a key identifier is active during the current transition.
    pub fn is_key_active(&self, key_id: u8) -> bool {
        key_id == self.current_key_id || Some(key_id) == self.previous_key_id
    }
}

/// Errors returned by pure crypto helper types.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// Returned when a hex-encoded key has the wrong length.
    #[error("invalid key length: got {0} bytes, expected 64 hex chars")]
    InvalidKeyLength(usize),
    /// Returned when a key contains invalid hexadecimal characters.
    #[error("invalid hex in key: {0}")]
    InvalidHex(String),
    /// Returned when an IPsec key ID is outside the allowed range.
    #[error("key id must be 1-255")]
    InvalidKeyId,
    /// Returned when a certificate is already expired.
    #[error("certificate expired")]
    CertExpired,
    /// Returned when a certificate does not match the requested hostname.
    #[error("certificate not valid for host: {0}")]
    InvalidHost(String),
    /// Returned when filesystem-backed crypto material cannot be read.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// A lightweight keypair placeholder used by higher-level components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPair {
    /// Public key bytes.
    pub public: Vec<u8>,
    /// Private key bytes.
    pub private: Vec<u8>,
}

impl KeyPair {
    /// Creates a new keypair from raw bytes.
    pub fn new(public: impl Into<Vec<u8>>, private: impl Into<Vec<u8>>) -> Self {
        Self {
            public: public.into(),
            private: private.into(),
        }
    }

    /// Returns a fingerprint of the public key.
    pub fn public_fingerprint(&self) -> Fingerprint {
        Fingerprint::sha256(&self.public)
    }
}

/// Parsed certificate and private key data loaded from PEM files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsKeypair {
    /// DER-encoded certificate chain.
    pub certificate_chain: Vec<Vec<u8>>,
    /// Original PEM-encoded private key bytes.
    pub private_key_pem: Vec<u8>,
}

/// Parsed custom CA certificates loaded from PEM files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateAuthorityPool {
    /// DER-encoded CA certificates.
    pub certificates: Vec<Vec<u8>>,
}

/// Errors returned by certloader parity helpers.
#[derive(Debug, Error)]
pub enum CertLoaderError {
    /// Returned when only one of cert/private-key path is provided.
    #[error("certificate and private key are both required, but only one was provided")]
    InvalidKeypair,

    /// Returned when a PEM file cannot be read.
    #[error("failed to load cert {path:?}: {source}")]
    ReadFile {
        /// Path to file that failed to load.
        path: PathBuf,
        /// Read error.
        source: std::io::Error,
    },

    /// Returned when a PEM file is malformed or does not contain certificates.
    #[error("failed to load cert {path:?}: must be PEM encoded")]
    InvalidPem {
        /// Path to malformed PEM file.
        path: PathBuf,
    },

    /// Returned when private key PEM cannot be parsed.
    #[error("failed to load keypair: {0}")]
    InvalidKeypairPem(String),

    /// Returned when a watched server config is missing cert path.
    #[error("certificate file path is required")]
    MissingCertFile,

    /// Returned when a watched server config is missing private key path.
    #[error("private key file path is required")]
    MissingPrivkeyFile,
}

#[derive(Debug, Clone, Default)]
struct ReloaderState {
    ca_cert_pool: Option<CertificateAuthorityPool>,
    ca_cert_pool_generation: u64,
    keypair: Option<TlsKeypair>,
    keypair_generation: u64,
}

/// File-based TLS reloader ported from cilium/pkg/crypto/certloader/reloader.go.
#[derive(Debug)]
pub struct FileReloader {
    ca_files: Vec<PathBuf>,
    cert_file: Option<PathBuf>,
    privkey_file: Option<PathBuf>,
    state: Mutex<ReloaderState>,
}

impl FileReloader {
    /// Construct a file reloader that is immediately loaded and ready.
    pub fn new_ready(
        ca_files: Vec<PathBuf>,
        cert_file: Option<PathBuf>,
        privkey_file: Option<PathBuf>,
    ) -> Result<Self, CertLoaderError> {
        let mut reloader = Self::new(ca_files, cert_file, privkey_file)?;
        let _ = reloader.reload()?;
        Ok(reloader)
    }

    /// Construct a file reloader without loading files yet.
    pub fn new(
        ca_files: Vec<PathBuf>,
        cert_file: Option<PathBuf>,
        privkey_file: Option<PathBuf>,
    ) -> Result<Self, CertLoaderError> {
        let cert_is_set = cert_file.is_some();
        let key_is_set = privkey_file.is_some();
        if cert_is_set != key_is_set {
            return Err(CertLoaderError::InvalidKeypair);
        }

        Ok(Self {
            ca_files,
            cert_file,
            privkey_file,
            state: Mutex::new(ReloaderState::default()),
        })
    }

    /// Returns true when both certificate and private key are configured.
    pub fn has_keypair(&self) -> bool {
        self.cert_file.is_some() && self.privkey_file.is_some()
    }

    /// Returns true when custom CA files are configured.
    pub fn has_custom_ca(&self) -> bool {
        !self.ca_files.is_empty()
    }

    /// Returns true when configured file contents are loaded.
    pub fn ready(&self) -> bool {
        let (keypair, ca_cert_pool) = self.keypair_and_ca_cert_pool();
        if self.has_keypair() && keypair.is_none() {
            return false;
        }
        if self.has_custom_ca() && ca_cert_pool.is_none() {
            return false;
        }
        true
    }

    /// Return loaded keypair and CA pool snapshot.
    pub fn keypair_and_ca_cert_pool(
        &self,
    ) -> (Option<TlsKeypair>, Option<CertificateAuthorityPool>) {
        let state = lock(&self.state);
        (state.keypair.clone(), state.ca_cert_pool.clone())
    }

    /// Reload keypair and custom CA files atomically.
    pub fn reload(
        &mut self,
    ) -> Result<(Option<TlsKeypair>, Option<CertificateAuthorityPool>), CertLoaderError> {
        let keypair = if self.has_keypair() {
            Some(read_keypair(
                self.cert_file.as_deref(),
                self.privkey_file.as_deref(),
            )?)
        } else {
            None
        };

        let ca_cert_pool = if self.has_custom_ca() {
            Some(read_certificate_authority(&self.ca_files)?)
        } else {
            None
        };

        let mut state = lock(&self.state);
        if let Some(ref keypair_loaded) = keypair {
            state.keypair = Some(keypair_loaded.clone());
            state.keypair_generation += 1;
        }
        if let Some(ref ca_loaded) = ca_cert_pool {
            state.ca_cert_pool = Some(ca_loaded.clone());
            state.ca_cert_pool_generation += 1;
        }

        Ok((keypair, ca_cert_pool))
    }

    /// Reload only keypair files.
    pub fn reload_keypair(&mut self) -> Result<Option<TlsKeypair>, CertLoaderError> {
        if !self.has_keypair() {
            return Ok(None);
        }

        let keypair = read_keypair(self.cert_file.as_deref(), self.privkey_file.as_deref())?;
        let mut state = lock(&self.state);
        state.keypair = Some(keypair.clone());
        state.keypair_generation += 1;
        Ok(Some(keypair))
    }

    /// Reload only custom CA files.
    pub fn reload_ca(&mut self) -> Result<Option<CertificateAuthorityPool>, CertLoaderError> {
        if !self.has_custom_ca() {
            return Ok(None);
        }

        let ca_cert_pool = read_certificate_authority(&self.ca_files)?;
        let mut state = lock(&self.state);
        state.ca_cert_pool = Some(ca_cert_pool.clone());
        state.ca_cert_pool_generation += 1;
        Ok(Some(ca_cert_pool))
    }

    /// Return keypair and CA generation counters.
    pub fn generations(&self) -> (u64, u64) {
        let state = lock(&self.state);
        (state.keypair_generation, state.ca_cert_pool_generation)
    }
}

/// Lightweight client TLS config used by parity tests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClientConfig {
    /// Optional minimum TLS protocol version.
    pub min_tls_version: Option<u16>,
    /// Optional root CAs.
    pub root_cas: Option<CertificateAuthorityPool>,
    /// Optional mTLS keypair.
    pub client_keypair: Option<TlsKeypair>,
}

const WATCHER_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// File-backed cert watcher using deterministic polling in place of fsnotify.
#[derive(Debug)]
pub struct Watcher {
    reloader: Arc<Mutex<FileReloader>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Watcher {
    /// Construct a watcher that is immediately loaded and ready.
    pub fn new(
        ca_files: Vec<PathBuf>,
        cert_file: Option<PathBuf>,
        privkey_file: Option<PathBuf>,
    ) -> Result<Self, CertLoaderError> {
        let reloader = FileReloader::new_ready(ca_files, cert_file, privkey_file)?;
        Ok(Self::from_reloader(reloader))
    }

    fn from_reloader(reloader: FileReloader) -> Self {
        let tracked = tracked_files(&reloader);
        let keypair_paths = tracked.0;
        let ca_paths = tracked.1;
        let mut keypair_hashes = file_hashes(&keypair_paths);
        let mut ca_hashes = file_hashes(&ca_paths);
        let reloader = Arc::new(Mutex::new(reloader));
        let stop = Arc::new(AtomicBool::new(false));
        let reloader_for_worker = Arc::clone(&reloader);
        let stop_for_worker = Arc::clone(&stop);

        let worker = thread::spawn(move || {
            {
                let mut reload = lock(&reloader_for_worker);
                if !reload.ready() {
                    let _ = reload.reload_keypair();
                    let _ = reload.reload_ca();
                }
            }

            while !stop_for_worker.load(Ordering::Relaxed) {
                let keypair_changed = update_hashes(&keypair_paths, &mut keypair_hashes);
                let ca_changed = update_hashes(&ca_paths, &mut ca_hashes);

                if keypair_changed || ca_changed {
                    let mut reload = lock(&reloader_for_worker);
                    if keypair_changed {
                        let _ = reload.reload_keypair();
                    }
                    if ca_changed {
                        let _ = reload.reload_ca();
                    }
                }

                thread::sleep(WATCHER_POLL_INTERVAL);
            }
        });

        Self {
            reloader,
            stop,
            worker: Mutex::new(Some(worker)),
        }
    }

    /// Returns true when keypair files are configured.
    pub fn has_keypair(&self) -> bool {
        lock(&self.reloader).has_keypair()
    }

    /// Returns true when custom CA files are configured.
    pub fn has_custom_ca(&self) -> bool {
        lock(&self.reloader).has_custom_ca()
    }

    /// Returns true when watched TLS material is loaded.
    pub fn ready(&self) -> bool {
        lock(&self.reloader).ready()
    }

    /// Return loaded keypair and CA pool snapshot.
    pub fn keypair_and_ca_cert_pool(
        &self,
    ) -> (Option<TlsKeypair>, Option<CertificateAuthorityPool>) {
        lock(&self.reloader).keypair_and_ca_cert_pool()
    }

    /// Return generation counters from underlying reloader.
    pub fn generations(&self) -> (u64, u64) {
        lock(&self.reloader).generations()
    }

    /// Stop polling thread.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = lock(&self.worker).take() {
            let _ = handle.join();
        }
    }
}

/// Returns a channel that yields one watcher once configured files become ready.
pub fn future_watcher(
    ca_files: Vec<PathBuf>,
    cert_file: Option<PathBuf>,
    privkey_file: Option<PathBuf>,
) -> Result<mpsc::Receiver<Watcher>, CertLoaderError> {
    let reloader = FileReloader::new(ca_files, cert_file, privkey_file)?;
    let watcher = Watcher::from_reloader(reloader);
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let mut watcher = Some(watcher);
        loop {
            let Some(current) = watcher.as_ref() else {
                return;
            };
            if current.ready() {
                let _ = tx.send(watcher.take().expect("watcher present"));
                return;
            }
            thread::sleep(WATCHER_POLL_INTERVAL);
        }
    });

    Ok(rx)
}

/// File-backed client config builder ported from client.go behavior.
#[derive(Debug)]
pub struct WatchedClientConfig {
    watcher: Watcher,
}

impl WatchedClientConfig {
    /// Construct a watched client config from file paths.
    pub fn new(
        ca_files: Vec<PathBuf>,
        cert_file: Option<PathBuf>,
        privkey_file: Option<PathBuf>,
    ) -> Result<Self, CertLoaderError> {
        let watcher = Watcher::new(ca_files, cert_file, privkey_file)?;
        Ok(Self { watcher })
    }

    /// Return true if mTLS keypair is configured.
    pub fn is_mutual_tls(&self) -> bool {
        self.watcher.has_keypair()
    }

    /// Build a client config from a base config.
    pub fn client_config(&self, base: &ClientConfig) -> ClientConfig {
        let (keypair, ca_cert_pool) = self.watcher.keypair_and_ca_cert_pool();
        let mut cfg = base.clone();
        cfg.root_cas = ca_cert_pool;
        cfg.client_keypair = if self.is_mutual_tls() { keypair } else { None };
        cfg
    }

    /// Stop watching files.
    pub fn stop(&self) {
        self.watcher.stop();
    }

    /// Return current generation counters.
    pub fn generations(&self) -> (u64, u64) {
        self.watcher.generations()
    }
}

/// Returns a channel that yields one watched client config when files are ready.
pub fn future_watched_client_config(
    ca_files: Vec<PathBuf>,
    cert_file: Option<PathBuf>,
    privkey_file: Option<PathBuf>,
) -> Result<mpsc::Receiver<WatchedClientConfig>, CertLoaderError> {
    let fw = future_watcher(ca_files, cert_file, privkey_file)?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(watcher) = fw.recv() {
            let _ = tx.send(WatchedClientConfig { watcher });
        }
    });
    Ok(rx)
}

/// Minimal server TLS config used by parity tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Optional minimum TLS protocol version.
    pub min_tls_version: Option<u16>,
    /// Loaded server certificates.
    pub certificates: Vec<TlsKeypair>,
    /// Optional client CAs for mTLS.
    pub client_cas: Option<CertificateAuthorityPool>,
    /// Client-auth mode.
    pub client_auth: ClientAuth,
    /// ALPN protocols.
    pub next_protocols: Vec<String>,
}

/// Minimal client-auth mode for parity tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientAuth {
    /// Disable client certificate auth.
    #[default]
    NoClientCert,
    /// Require and verify client certificates.
    RequireAndVerifyClientCert,
}

/// File-backed server config builder ported from server.go behavior.
#[derive(Debug)]
pub struct WatchedServerConfig {
    watcher: Watcher,
}

impl WatchedServerConfig {
    /// Construct watched server config. Certificate and key paths are required.
    pub fn new(
        ca_files: Vec<PathBuf>,
        cert_file: Option<PathBuf>,
        privkey_file: Option<PathBuf>,
    ) -> Result<Self, CertLoaderError> {
        if cert_file.is_none() {
            return Err(CertLoaderError::MissingCertFile);
        }
        if privkey_file.is_none() {
            return Err(CertLoaderError::MissingPrivkeyFile);
        }
        let watcher = Watcher::new(ca_files, cert_file, privkey_file)?;
        Ok(Self { watcher })
    }

    /// Return true if this server is configured for mTLS.
    pub fn is_mutual_tls(&self) -> bool {
        self.watcher.has_custom_ca()
    }

    /// Build server config from a base config.
    pub fn server_config(&self, base: &ServerConfig) -> ServerConfig {
        let (keypair, ca_cert_pool) = self.watcher.keypair_and_ca_cert_pool();
        let mut cfg = base.clone();
        cfg.certificates = keypair.into_iter().collect();
        if !cfg.next_protocols.iter().any(|proto| proto == "h2") {
            cfg.next_protocols.push("h2".to_string());
        }
        if self.is_mutual_tls() {
            cfg.client_cas = ca_cert_pool;
            if cfg.client_auth == ClientAuth::NoClientCert {
                cfg.client_auth = ClientAuth::RequireAndVerifyClientCert;
            }
        }
        cfg
    }

    /// Return generation counters.
    pub fn generations(&self) -> (u64, u64) {
        self.watcher.generations()
    }

    /// Stop watching files.
    pub fn stop(&self) {
        self.watcher.stop();
    }
}

/// Returns a channel that yields one watched server config when files are ready.
pub fn future_watched_server_config(
    ca_files: Vec<PathBuf>,
    cert_file: Option<PathBuf>,
    privkey_file: Option<PathBuf>,
) -> Result<mpsc::Receiver<WatchedServerConfig>, CertLoaderError> {
    if cert_file.is_none() {
        return Err(CertLoaderError::MissingCertFile);
    }
    if privkey_file.is_none() {
        return Err(CertLoaderError::MissingPrivkeyFile);
    }
    let fw = future_watcher(ca_files, cert_file, privkey_file)?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok(watcher) = fw.recv() {
            let _ = tx.send(WatchedServerConfig { watcher });
        }
    });
    Ok(rx)
}

fn tracked_files(reloader: &FileReloader) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut keypair = Vec::new();
    if let Some(cert_file) = &reloader.cert_file {
        keypair.push(cert_file.clone());
    }
    if let Some(privkey_file) = &reloader.privkey_file {
        keypair.push(privkey_file.clone());
    }
    (keypair, reloader.ca_files.clone())
}

fn file_hashes(paths: &[PathBuf]) -> Vec<Option<Fingerprint>> {
    paths.iter().map(|path| file_hash(path)).collect()
}

fn file_hash(path: &Path) -> Option<Fingerprint> {
    fs::read(path).ok().map(|bytes| Fingerprint::sha256(&bytes))
}

fn update_hashes(paths: &[PathBuf], hashes: &mut [Option<Fingerprint>]) -> bool {
    let mut changed = false;
    for (path, hash) in paths.iter().zip(hashes.iter_mut()) {
        let current = file_hash(path);
        if current != *hash {
            *hash = current;
            changed = true;
        }
    }
    changed
}

fn read_keypair(
    cert_file: Option<&Path>,
    privkey_file: Option<&Path>,
) -> Result<TlsKeypair, CertLoaderError> {
    let cert_path = cert_file.ok_or(CertLoaderError::InvalidKeypair)?;
    let key_path = privkey_file.ok_or(CertLoaderError::InvalidKeypair)?;

    let cert_pem = fs::read(cert_path).map_err(|source| CertLoaderError::ReadFile {
        path: cert_path.to_path_buf(),
        source,
    })?;

    let mut cert_reader = cert_pem.as_slice();
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CertLoaderError::InvalidPem {
            path: cert_path.to_path_buf(),
        })?;
    if certs.is_empty() {
        return Err(CertLoaderError::InvalidPem {
            path: cert_path.to_path_buf(),
        });
    }

    let key_pem = fs::read(key_path).map_err(|source| CertLoaderError::ReadFile {
        path: key_path.to_path_buf(),
        source,
    })?;
    let mut key_reader = key_pem.as_slice();
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|err| CertLoaderError::InvalidKeypairPem(err.to_string()))?;
    if key.is_none() {
        return Err(CertLoaderError::InvalidKeypairPem(
            "no private key found".to_string(),
        ));
    }

    Ok(TlsKeypair {
        certificate_chain: certs.into_iter().map(|cert| cert.to_vec()).collect(),
        private_key_pem: key_pem,
    })
}

fn read_certificate_authority(
    ca_files: &[PathBuf],
) -> Result<CertificateAuthorityPool, CertLoaderError> {
    let mut certificates = Vec::new();

    for path in ca_files {
        let pem = fs::read(path).map_err(|source| CertLoaderError::ReadFile {
            path: path.clone(),
            source,
        })?;

        let mut reader = pem.as_slice();
        let mut certs = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CertLoaderError::InvalidPem { path: path.clone() })?;
        if certs.is_empty() {
            return Err(CertLoaderError::InvalidPem { path: path.clone() });
        }

        certificates.extend(certs.drain(..).map(|cert| cert.to_vec()));
    }

    Ok(CertificateAuthorityPool { certificates })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn fingerprint_is_stable() {
        let fp = Fingerprint::sha256(b"hello");
        assert_eq!(fp.as_bytes().len(), 32);
    }

    #[test]
    fn keypair_works() {
        let kp = KeyPair::new([1, 2, 3], [4, 5, 6]);
        assert_eq!(kp.public_fingerprint().as_bytes().len(), 32);
        assert_eq!(kp.private, vec![4, 5, 6]);
    }

    #[test]
    fn test_symmetric_key_hex_roundtrip() {
        let key = SymmetricKey::new([0xab; 32]);
        let hex = key.to_hex();
        assert_eq!(hex.len(), 64);
        let key2 = SymmetricKey::from_hex(&hex).unwrap();
        assert_eq!(key, key2);
    }

    #[test]
    fn test_symmetric_key_invalid_hex() {
        assert!(SymmetricKey::from_hex("not-valid").is_err());
        assert!(SymmetricKey::from_hex(&"ab".repeat(16)).is_err());
    }

    #[test]
    fn test_symmetric_key_debug_redacted() {
        let key = SymmetricKey::new([0; 32]);
        let dbg = format!("{:?}", key);
        assert!(dbg.contains("REDACTED"));
        assert!(!dbg.contains("00"));
    }

    #[test]
    fn test_tls_config_mtls() {
        let cfg = TLSConfig::new().with_server_name("cilium.example.com");
        assert!(!cfg.is_mtls());
        let mtls = TLSConfig {
            client_cert_path: Some("/cert.pem".into()),
            client_key_path: Some("/key.pem".into()),
            ..Default::default()
        };
        assert!(mtls.is_mtls());
    }

    #[test]
    fn test_cert_info_wildcard_match() {
        let cert = CertInfo {
            subject: "CN=*.example.com".into(),
            issuer: "CN=CA".into(),
            not_before: SystemTime::now() - Duration::from_secs(3600),
            not_after: SystemTime::now() + Duration::from_secs(86400),
            serial: "01".into(),
            is_ca: false,
            dns_names: vec!["*.example.com".into()],
            ip_addresses: vec![],
        };
        assert!(cert.is_valid_for("api.example.com"));
        assert!(!cert.is_valid_for("api.other.com"));
        assert!(!cert.is_expired());
    }

    #[test]
    fn test_key_rotation() {
        let mut state = KeyRotationState::new(1);
        assert!(state.is_key_active(1));
        assert!(!state.is_key_active(2));
        state.rotate(2);
        assert!(state.is_key_active(1));
        assert!(state.is_key_active(2));
        assert_eq!(state.rotation_epoch, 1);
        state.rotate(3);
        assert!(!state.is_key_active(1));
        assert!(state.is_key_active(2));
        assert!(state.is_key_active(3));
    }

    #[test]
    fn test_ipsec_key_invalid_id() {
        let k = SymmetricKey::new([0; 32]);
        assert!(IPsecKey::new(0, k.clone(), k.clone(), IPsecCipher::AES256GCM).is_err());
        assert!(IPsecKey::new(1, k.clone(), k.clone(), IPsecCipher::AES256GCM).is_ok());
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-artifacts");
            fs::create_dir_all(&base).expect("create .test-artifacts");

            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("crypto-{nanos}-{id}"));
            fs::create_dir_all(&path).expect("create test dir");

            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Clone)]
    struct TlsConfigFiles {
        ca_files: Vec<PathBuf>,
        cert_file: PathBuf,
        privkey_file: PathBuf,
    }

    fn directories() -> (TestDir, TlsConfigFiles, TlsConfigFiles) {
        let dir = TestDir::new();

        let hubble_dir = dir.path.join("hubble");
        let relay_dir = dir.path.join("relay");
        fs::create_dir_all(&hubble_dir).expect("create hubble dir");
        fs::create_dir_all(&relay_dir).expect("create relay dir");

        let hubble = TlsConfigFiles {
            ca_files: vec![hubble_dir.join("ca.crt")],
            cert_file: hubble_dir.join("server.crt"),
            privkey_file: hubble_dir.join("server.key"),
        };
        let relay = TlsConfigFiles {
            ca_files: vec![relay_dir.join("ca.crt")],
            cert_file: relay_dir.join("server.crt"),
            privkey_file: relay_dir.join("server.key"),
        };

        (dir, hubble, relay)
    }

    fn setup(hubble: &TlsConfigFiles, relay: &TlsConfigFiles) {
        write_file(&hubble.ca_files[0], INITIAL_CA_CERT);
        write_file(&hubble.cert_file, INITIAL_CERT);
        write_file(&hubble.privkey_file, INITIAL_KEY);

        write_file(&relay.ca_files[0], INITIAL_CA_CERT);
        write_file(&relay.cert_file, INITIAL_CERT);
        write_file(&relay.privkey_file, INITIAL_KEY);
    }

    fn write_file(path: &Path, content: &[u8]) {
        fs::write(path, content).expect("write test pem");
    }

    fn rotate(hubble: &TlsConfigFiles, relay: &TlsConfigFiles) {
        let mut rotated_ca = INITIAL_CA_CERT.to_vec();
        rotated_ca.push(b'\n');
        let mut rotated_cert = INITIAL_CERT.to_vec();
        rotated_cert.push(b'\n');
        let mut rotated_key = INITIAL_KEY.to_vec();
        rotated_key.push(b'\n');

        write_file(&hubble.ca_files[0], &rotated_ca);
        write_file(&hubble.cert_file, &rotated_cert);
        write_file(&hubble.privkey_file, &rotated_key);

        write_file(&relay.ca_files[0], &rotated_ca);
        write_file(&relay.cert_file, &rotated_cert);
        write_file(&relay.privkey_file, &rotated_key);
    }

    fn rotated_material() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut rotated_ca = INITIAL_CA_CERT.to_vec();
        rotated_ca.push(b'\n');
        let mut rotated_cert = INITIAL_CERT.to_vec();
        rotated_cert.push(b'\n');
        let mut rotated_key = INITIAL_KEY.to_vec();
        rotated_key.push(b'\n');
        (rotated_ca, rotated_cert, rotated_key)
    }

    #[cfg(unix)]
    fn k8s_data_dir_name() -> String {
        format!("..{:020}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    #[cfg(unix)]
    fn k8s_directories() -> (TestDir, TlsConfigFiles) {
        let dir = TestDir::new();
        let root = dir.path.clone();

        let hubble = TlsConfigFiles {
            ca_files: vec![root.join("client-ca.crt")],
            cert_file: root.join("hubble").join("server.crt"),
            privkey_file: root.join("hubble").join("server.key"),
        };

        let empty_data_dir = k8s_data_dir_name();
        fs::create_dir_all(root.join(&empty_data_dir)).expect("create initial k8s data dir");
        std::os::unix::fs::symlink(&empty_data_dir, root.join("..data"))
            .expect("create ..data symlink");

        (dir, hubble)
    }

    #[cfg(unix)]
    fn k8s_update(dir: &Path, cert: &[u8], key: &[u8], ca: &[u8]) {
        let new_data_dir = k8s_data_dir_name();
        let old_data_dir = fs::read_link(dir.join("..data")).expect("read old ..data symlink");
        let new_data_path = dir.join(&new_data_dir);
        fs::create_dir_all(new_data_path.join("hubble")).expect("create new hubble dir");
        write_file(&new_data_path.join("hubble").join("server.crt"), cert);
        write_file(&new_data_path.join("hubble").join("server.key"), key);
        write_file(&new_data_path.join("client-ca.crt"), ca);

        std::os::unix::fs::symlink(&new_data_dir, dir.join("..data_tmp"))
            .expect("create ..data_tmp symlink");
        fs::rename(dir.join("..data_tmp"), dir.join("..data")).expect("swap ..data symlink");
        fs::remove_dir_all(dir.join(old_data_dir)).expect("remove old k8s data dir");
    }

    #[cfg(unix)]
    fn k8_setup(dir: &Path) {
        k8s_update(dir, INITIAL_CERT, INITIAL_KEY, INITIAL_CA_CERT);
        std::os::unix::fs::symlink(Path::new("..data").join("hubble"), dir.join("hubble"))
            .expect("create hubble symlink");
        std::os::unix::fs::symlink("..data/client-ca.crt", dir.join("client-ca.crt"))
            .expect("create ca symlink");
    }

    #[cfg(unix)]
    fn k8s_rotate(dir: &Path) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (rotated_ca, rotated_cert, rotated_key) = rotated_material();
        k8s_update(dir, &rotated_cert, &rotated_key, &rotated_ca);
        (rotated_ca, rotated_cert, rotated_key)
    }

    fn wait_until(mut predicate: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        predicate()
    }

    fn ca_pool_from_bytes(bytes: &[u8]) -> CertificateAuthorityPool {
        let mut reader = bytes;
        let certificates = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("parse ca cert")
            .into_iter()
            .map(|cert| cert.to_vec())
            .collect();
        CertificateAuthorityPool { certificates }
    }

    fn keypair_from_bytes(cert_pem: &[u8], key_pem: &[u8]) -> TlsKeypair {
        let mut cert_reader = cert_pem;
        let certificate_chain = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("parse cert chain")
            .into_iter()
            .map(|cert| cert.to_vec())
            .collect();

        let mut key_reader = key_pem;
        let _ = rustls_pemfile::private_key(&mut key_reader)
            .expect("parse private key")
            .expect("private key present");

        TlsKeypair {
            certificate_chain,
            private_key_pem: key_pem.to_vec(),
        }
    }

    mod parity_tests {
        use super::*;
        use seriousum_core::{HookError, HookFn, Lifecycle, Promise};
        use std::path::PathBuf;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;
        use std::thread::JoinHandle;

        // ---- certloader/cell_test.go ----

        #[derive(Clone)]
        struct CellTestConfig {
            tls: bool,
            tls_cert_file: Option<PathBuf>,
            tls_key_file: Option<PathBuf>,
            tls_client_ca_files: Vec<PathBuf>,
        }

        struct CellHarness {
            promise: Option<Promise<Arc<WatchedServerConfig>>>,
            error: Arc<Mutex<Option<CertLoaderError>>>,
            cleaned: Arc<AtomicBool>,
        }

        fn install_watched_server_config_cell(
            lifecycle: &mut Lifecycle,
            config: CellTestConfig,
        ) -> CellHarness {
            let resolved = Arc::new(Mutex::new(None));
            let error = Arc::new(Mutex::new(None));
            let cleaned = Arc::new(AtomicBool::new(false));
            if !config.tls {
                return CellHarness {
                    promise: None,
                    error,
                    cleaned,
                };
            }

            let promise = Promise::new();
            let promise_for_start = promise.clone();
            let resolved_for_start = Arc::clone(&resolved);
            let resolved_for_stop = Arc::clone(&resolved);
            let error_for_start = Arc::clone(&error);
            let cleaned_for_stop = Arc::clone(&cleaned);
            let stop = Arc::new(AtomicBool::new(false));
            let stop_for_start = Arc::clone(&stop);
            let stop_for_stop = Arc::clone(&stop);
            let worker = Arc::new(Mutex::new(None::<JoinHandle<()>>));
            let worker_for_start = Arc::clone(&worker);
            let worker_for_stop = Arc::clone(&worker);

            lifecycle.append(HookFn::new(
                move || -> Result<(), HookError> {
                    stop_for_start.store(false, Ordering::SeqCst);
                    let promise = promise_for_start.clone();
                    let resolved = Arc::clone(&resolved_for_start);
                    let error = Arc::clone(&error_for_start);
                    let stop = Arc::clone(&stop_for_start);
                    let config = config.clone();
                    let handle = thread::spawn(move || {
                        let Some(cert_file) = config.tls_cert_file.clone() else {
                            *lock(&error) = Some(CertLoaderError::MissingCertFile);
                            return;
                        };
                        let Some(key_file) = config.tls_key_file.clone() else {
                            *lock(&error) = Some(CertLoaderError::MissingPrivkeyFile);
                            return;
                        };

                        match FileReloader::new(
                            config.tls_client_ca_files,
                            Some(cert_file),
                            Some(key_file),
                        ) {
                            Ok(reloader) => {
                                let watcher = Watcher::from_reloader(reloader);
                                while !stop.load(Ordering::SeqCst) {
                                    if watcher.ready() {
                                        let config = Arc::new(WatchedServerConfig { watcher });
                                        *lock(&resolved) = Some(Arc::clone(&config));
                                        promise.resolve(config);
                                        return;
                                    }
                                    thread::sleep(WATCHER_POLL_INTERVAL);
                                }
                                watcher.stop();
                            }
                            Err(err) => {
                                *lock(&error) = Some(err);
                            }
                        }
                    });
                    *lock(&worker_for_start) = Some(handle);
                    Ok(())
                },
                move || -> Result<(), HookError> {
                    stop_for_stop.store(true, Ordering::SeqCst);
                    if let Some(handle) = lock(&worker_for_stop).take() {
                        let _ = handle.join();
                    }
                    if let Some(config) = lock(&resolved_for_stop).take() {
                        config.stop();
                    }
                    cleaned_for_stop.store(true, Ordering::SeqCst);
                    Ok(())
                },
            ));

            CellHarness {
                promise: Some(promise),
                error,
                cleaned,
            }
        }

        /// Port of Go: TestCell in pkg/crypto/certloader/cell_test.go
        #[test]
        fn parity_test_cell() {
            let (_dir, hubble, relay) = directories();
            let mut lifecycle = Lifecycle::new();
            let harness = install_watched_server_config_cell(
                &mut lifecycle,
                CellTestConfig {
                    tls: true,
                    tls_cert_file: Some(hubble.cert_file.clone()),
                    tls_key_file: Some(hubble.privkey_file.clone()),
                    tls_client_ca_files: hubble.ca_files.clone(),
                },
            );
            let promise = harness
                .promise
                .expect("tls-enabled cell should provide a promise");
            let receiver = promise.receiver();

            lifecycle.start_all().expect("lifecycle should start");
            let setup_handle = thread::spawn({
                let hubble = hubble.clone();
                let relay = relay.clone();
                move || {
                    thread::sleep(Duration::from_millis(100));
                    setup(&hubble, &relay);
                }
            });

            assert!(wait_until(|| receiver.borrow().as_ref().is_some()));
            setup_handle.join().expect("setup thread should finish");

            let watched = receiver.borrow().clone().expect("promise should resolve");
            let server_config = watched.server_config(&ServerConfig {
                min_tls_version: None,
                certificates: Vec::new(),
                client_cas: None,
                client_auth: ClientAuth::NoClientCert,
                next_protocols: Vec::new(),
            });
            assert_eq!(server_config.certificates.len(), 1);
            assert_eq!(
                server_config.client_auth,
                ClientAuth::RequireAndVerifyClientCert
            );
            assert!(lock(&harness.error).is_none());

            lifecycle.stop_all();
            assert!(harness.cleaned.load(Ordering::SeqCst));
        }

        /// Port of Go: TestCellConfigError in pkg/crypto/certloader/cell_test.go
        #[test]
        fn parity_test_cell_config_error() {
            let mut lifecycle = Lifecycle::new();
            let harness = install_watched_server_config_cell(
                &mut lifecycle,
                CellTestConfig {
                    tls: true,
                    tls_cert_file: None,
                    tls_key_file: None,
                    tls_client_ca_files: Vec::new(),
                },
            );
            let promise = harness
                .promise
                .expect("tls-enabled cell should provide a promise");
            let receiver = promise.receiver();

            lifecycle.start_all().expect("lifecycle should start");
            assert!(wait_until(|| lock(&harness.error).is_some()));
            assert!(matches!(
                &*lock(&harness.error),
                Some(CertLoaderError::MissingCertFile)
            ));
            assert!(receiver.borrow().is_none());

            lifecycle.stop_all();
            assert!(harness.cleaned.load(Ordering::SeqCst));
        }

        /// Port of Go: TestCellShutdown in pkg/crypto/certloader/cell_test.go
        #[test]
        fn parity_test_cell_shutdown() {
            let (_dir, hubble, _relay) = directories();
            let mut lifecycle = Lifecycle::new();
            let harness = install_watched_server_config_cell(
                &mut lifecycle,
                CellTestConfig {
                    tls: true,
                    tls_cert_file: Some(hubble.cert_file.clone()),
                    tls_key_file: Some(hubble.privkey_file.clone()),
                    tls_client_ca_files: hubble.ca_files.clone(),
                },
            );
            let promise = harness
                .promise
                .expect("tls-enabled cell should provide a promise");
            let receiver = promise.receiver();

            lifecycle.start_all().expect("lifecycle should start");
            thread::sleep(Duration::from_millis(100));
            lifecycle.stop_all();

            assert!(receiver.borrow().is_none());
            assert!(harness.cleaned.load(Ordering::SeqCst));
        }

        /// Port of Go: TestCellDisabled in pkg/crypto/certloader/cell_test.go
        #[test]
        fn parity_test_cell_disabled() {
            let mut lifecycle = Lifecycle::new();
            let harness = install_watched_server_config_cell(
                &mut lifecycle,
                CellTestConfig {
                    tls: false,
                    tls_cert_file: None,
                    tls_key_file: None,
                    tls_client_ca_files: Vec::new(),
                },
            );

            lifecycle
                .start_all()
                .expect("disabled lifecycle should start");
            assert!(harness.promise.is_none());
            lifecycle.stop_all();
        }

        // ---- certloader/server_test.go ----

        /// Port of Go: TestNewWatchedServerConfigErrors in pkg/crypto/certloader/server_test.go
        #[test]
        fn parity_test_new_watched_server_config_errors() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            assert!(matches!(
                WatchedServerConfig::new(
                    relay.ca_files.clone(),
                    None,
                    Some(hubble.privkey_file.clone())
                )
                .expect_err("missing cert file should fail"),
                CertLoaderError::MissingCertFile
            ));
            assert!(matches!(
                WatchedServerConfig::new(
                    relay.ca_files.clone(),
                    Some(hubble.cert_file.clone()),
                    None
                )
                .expect_err("missing key file should fail"),
                CertLoaderError::MissingPrivkeyFile
            ));
        }

        /// Port of Go: TestWatchedServerConfigIsMutualTLS in pkg/crypto/certloader/server_test.go
        #[test]
        fn parity_test_watched_server_config_is_mutual_tls() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let keypair_only = WatchedServerConfig::new(
                vec![],
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("keypair-only watched server config");
            assert!(!keypair_only.is_mutual_tls());
            keypair_only.stop();

            let ca_and_keypair = WatchedServerConfig::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("ca+keypair watched server config");
            assert!(ca_and_keypair.is_mutual_tls());
            ca_and_keypair.stop();
        }

        /// Port of Go: TestFutureWatchedServerConfig in pkg/crypto/certloader/server_test.go
        #[test]
        fn parity_test_future_watched_server_config() {
            let (_dir, hubble, relay) = directories();
            let ch = future_watched_server_config(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("future watched server config");
            assert!(ch.recv_timeout(Duration::from_millis(150)).is_err());

            setup(&hubble, &relay);
            let s = ch
                .recv_timeout(Duration::from_secs(2))
                .expect("future server config should become ready");
            s.stop();
        }

        /// Port of Go: TestNewWatchedServerConfig in pkg/crypto/certloader/server_test.go
        #[test]
        fn parity_test_new_watched_server_config() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_ca = ca_pool_from_bytes(INITIAL_CA_CERT);
            let expected_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);

            let s = WatchedServerConfig::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("watched server config");

            let cfg = s.server_config(&ServerConfig {
                min_tls_version: Some(0x0304),
                certificates: vec![],
                client_cas: None,
                client_auth: ClientAuth::NoClientCert,
                next_protocols: vec![],
            });
            assert_eq!(cfg.min_tls_version, Some(0x0304));
            assert_eq!(cfg.certificates, vec![expected_keypair]);
            assert_eq!(cfg.client_cas, Some(expected_ca));
            assert_eq!(cfg.client_auth, ClientAuth::RequireAndVerifyClientCert);
            assert!(cfg.next_protocols.iter().any(|proto| proto == "h2"));
            s.stop();
        }

        /// Port of Go: TestWatchedServerConfigRotation in pkg/crypto/certloader/server_test.go
        #[test]
        fn parity_test_watched_server_config_rotation() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let s = WatchedServerConfig::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("watched server config");
            let (prev_keypair, prev_ca) = s.generations();

            rotate(&hubble, &relay);
            assert!(wait_until(|| {
                let (keypair, ca) = s.generations();
                keypair > prev_keypair && ca > prev_ca
            }));
            s.stop();
        }

        // ---- certloader/watcher_test.go ----

        /// Port of Go: TestNewWatcherError in pkg/crypto/certloader/watcher_test.go
        #[test]
        fn parity_test_new_watcher_error() {
            let (_dir, hubble, relay) = directories();
            let err = Watcher::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect_err("missing files should fail");
            assert!(matches!(err, CertLoaderError::ReadFile { .. }));
        }

        /// Port of Go: TestNewWatcher in pkg/crypto/certloader/watcher_test.go
        #[test]
        fn parity_test_new_watcher() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let w = Watcher::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("new watcher");
            let (keypair, ca) = w.keypair_and_ca_cert_pool();
            assert!(keypair.is_some());
            assert!(ca.is_some());
            w.stop();
        }

        /// Port of Go: TestRotation in pkg/crypto/certloader/watcher_test.go
        #[test]
        fn parity_test_rotation() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let w = Watcher::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("watcher");
            let (prev_keypair, prev_ca) = w.generations();

            rotate(&hubble, &relay);
            assert!(wait_until(|| {
                let (keypair, ca) = w.generations();
                keypair > prev_keypair || ca > prev_ca
            }));
            w.stop();
        }

        /// Port of Go: TestFutureWatcherImmediately in pkg/crypto/certloader/watcher_test.go
        #[test]
        fn parity_test_future_watcher_immediately() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let ch = future_watcher(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("future watcher");
            let w = ch
                .recv_timeout(Duration::from_secs(2))
                .expect("future watcher should be immediately ready");
            w.stop();
        }

        /// Port of Go: TestFutureWatcher in pkg/crypto/certloader/watcher_test.go
        #[test]
        fn parity_test_future_watcher() {
            let (_dir, hubble, relay) = directories();
            let ch = future_watcher(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("future watcher");
            assert!(ch.recv_timeout(Duration::from_millis(150)).is_err());

            setup(&hubble, &relay);
            let w = ch
                .recv_timeout(Duration::from_secs(2))
                .expect("future watcher should become ready");
            w.stop();
        }

        /// Port of Go: TestFutureWatcherShutdownBeforeReady in pkg/crypto/certloader/watcher_test.go
        #[test]
        fn parity_test_future_watcher_shutdown_before_ready() {
            let (_dir, hubble, relay) = directories();
            let ch = future_watcher(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("future watcher");
            assert!(ch.recv_timeout(Duration::from_millis(150)).is_err());
        }

        /// Port of Go: TestKubernetesMount in pkg/crypto/certloader/watcher_test.go
        #[cfg(unix)]
        #[test]
        fn parity_test_kubernetes_mount() {
            let (dir, hubble) = k8s_directories();
            let ch = future_watcher(
                hubble.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("future watcher");
            assert!(ch.recv_timeout(Duration::from_millis(150)).is_err());

            k8_setup(&dir.path);

            let w = ch
                .recv_timeout(Duration::from_secs(2))
                .expect("future watcher should become ready");
            let expected_initial_ca = ca_pool_from_bytes(INITIAL_CA_CERT);
            let expected_initial_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);
            let (keypair, ca) = w.keypair_and_ca_cert_pool();
            assert_eq!(keypair, Some(expected_initial_keypair));
            assert_eq!(ca, Some(expected_initial_ca));

            let (prev_keypair, prev_ca) = w.generations();
            let (rotated_ca_pem, rotated_cert_pem, rotated_key_pem) = k8s_rotate(&dir.path);
            assert!(wait_until(|| {
                let (keypair_gen, ca_gen) = w.generations();
                keypair_gen > prev_keypair && ca_gen > prev_ca
            }));

            let expected_rotated_ca = ca_pool_from_bytes(&rotated_ca_pem);
            let expected_rotated_keypair = keypair_from_bytes(&rotated_cert_pem, &rotated_key_pem);
            let (keypair, ca) = w.keypair_and_ca_cert_pool();
            assert_eq!(keypair, Some(expected_rotated_keypair));
            assert_eq!(ca, Some(expected_rotated_ca));
            w.stop();
        }

        // ---- certloader/client_test.go ----

        /// Port of Go: TestWatchedClientConfigIsMutualTLS in pkg/crypto/certloader/client_test.go
        #[test]
        fn parity_test_watched_client_config_is_mutual_tls() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let empty = WatchedClientConfig::new(vec![], None, None).expect("empty client config");
            assert!(!empty.is_mutual_tls());

            let keypair_only = WatchedClientConfig::new(
                vec![],
                Some(relay.cert_file.clone()),
                Some(relay.privkey_file.clone()),
            )
            .expect("keypair-only client config");
            assert!(keypair_only.is_mutual_tls());

            let ca_only = WatchedClientConfig::new(vec![hubble.ca_files[0].clone()], None, None)
                .expect("ca-only client config");
            assert!(!ca_only.is_mutual_tls());

            let ca_and_keypair = WatchedClientConfig::new(
                vec![hubble.ca_files[0].clone()],
                Some(relay.cert_file.clone()),
                Some(relay.privkey_file.clone()),
            )
            .expect("ca+keypair client config");
            assert!(ca_and_keypair.is_mutual_tls());
        }

        /// Port of Go: TestFutureWatchedClientConfig in pkg/crypto/certloader/client_test.go
        #[test]
        fn parity_test_future_watched_client_config() {
            let (_dir, hubble, relay) = directories();
            let ch = future_watched_client_config(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("future watched client config");
            assert!(ch.recv_timeout(Duration::from_millis(150)).is_err());

            setup(&hubble, &relay);
            let cfg = ch
                .recv_timeout(Duration::from_secs(2))
                .expect("future client config should become ready");
            cfg.stop();
        }

        /// Port of Go: TestNewWatchedClientConfig in pkg/crypto/certloader/client_test.go
        #[test]
        fn parity_test_new_watched_client_config() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_ca = ca_pool_from_bytes(INITIAL_CA_CERT);
            let expected_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);

            let config = WatchedClientConfig::new(
                vec![hubble.ca_files[0].clone()],
                Some(relay.cert_file.clone()),
                Some(relay.privkey_file.clone()),
            )
            .expect("watched client config");

            let tls_config = config.client_config(&ClientConfig {
                min_tls_version: Some(0x0304),
                ..ClientConfig::default()
            });

            assert_eq!(tls_config.min_tls_version, Some(0x0304));
            assert_eq!(tls_config.root_cas, Some(expected_ca));
            assert_eq!(tls_config.client_keypair, Some(expected_keypair));
        }

        /// Port of Go: TestNewWatchedClientConfigWithoutClientCert in pkg/crypto/certloader/client_test.go
        #[test]
        fn parity_test_new_watched_client_config_without_client_cert() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let config = WatchedClientConfig::new(vec![hubble.ca_files[0].clone()], None, None)
                .expect("watched client config without cert");
            let tls_config = config.client_config(&ClientConfig {
                min_tls_version: Some(0x0304),
                ..ClientConfig::default()
            });

            assert_eq!(tls_config.min_tls_version, Some(0x0304));
            assert!(tls_config.root_cas.is_some());
            assert_eq!(tls_config.client_keypair, None);
        }

        /// Port of Go: TestWatchedClientConfigRotation in pkg/crypto/certloader/client_test.go
        #[test]
        fn parity_test_watched_client_config_rotation() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let config = WatchedClientConfig::new(
                vec![hubble.ca_files[0].clone()],
                Some(relay.cert_file.clone()),
                Some(relay.privkey_file.clone()),
            )
            .expect("watched client config");
            let (prev_keypair, prev_ca) = config.generations();

            rotate(&hubble, &relay);
            assert!(wait_until(|| {
                let (keypair, ca) = config.generations();
                keypair > prev_keypair || ca > prev_ca
            }));

            let tls_config = config.client_config(&ClientConfig::default());
            assert!(tls_config.client_keypair.is_some());
            assert!(tls_config.root_cas.is_some());
            config.stop();
        }

        // ---- certloader/reloader_test.go ----

        /// Port of Go: TestNewFileReloaderErrors in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_new_file_reloader_errors() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            assert!(matches!(
                FileReloader::new_ready(
                    relay.ca_files.clone(),
                    Some(hubble.cert_file.clone()),
                    None,
                )
                .expect_err("missing private key should fail"),
                CertLoaderError::InvalidKeypair
            ));

            assert!(matches!(
                FileReloader::new_ready(
                    relay.ca_files.clone(),
                    None,
                    Some(hubble.privkey_file.clone()),
                )
                .expect_err("missing cert should fail"),
                CertLoaderError::InvalidKeypair
            ));

            assert!(matches!(
                FileReloader::new(relay.ca_files.clone(), Some(hubble.cert_file.clone()), None)
                    .expect_err("missing private key should fail"),
                CertLoaderError::InvalidKeypair
            ));

            assert!(matches!(
                FileReloader::new(
                    relay.ca_files.clone(),
                    None,
                    Some(hubble.privkey_file.clone())
                )
                .expect_err("missing cert should fail"),
                CertLoaderError::InvalidKeypair
            ));
        }

        /// Port of Go: TestHasKeypair in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_has_keypair() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let tests = vec![
                (
                    FileReloader::new_ready(vec![], None, None).expect("empty ready"),
                    false,
                ),
                (FileReloader::new(vec![], None, None).expect("empty"), false),
                (
                    FileReloader::new_ready(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair ready"),
                    true,
                ),
                (
                    FileReloader::new(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair"),
                    true,
                ),
                (
                    FileReloader::new_ready(relay.ca_files.clone(), None, None).expect("ca ready"),
                    false,
                ),
                (
                    FileReloader::new(relay.ca_files.clone(), None, None).expect("ca only"),
                    false,
                ),
                (
                    FileReloader::new_ready(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("ca and keypair ready"),
                    true,
                ),
                (
                    FileReloader::new(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("ca and keypair"),
                    true,
                ),
            ];

            for (reloader, expected) in tests {
                assert_eq!(reloader.has_keypair(), expected);
            }
        }

        /// Port of Go: TestHasCustomCA in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_has_custom_ca() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let tests = vec![
                (
                    FileReloader::new_ready(vec![], None, None).expect("empty ready"),
                    false,
                ),
                (FileReloader::new(vec![], None, None).expect("empty"), false),
                (
                    FileReloader::new_ready(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair ready"),
                    false,
                ),
                (
                    FileReloader::new(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair"),
                    false,
                ),
                (
                    FileReloader::new_ready(relay.ca_files.clone(), None, None).expect("ca ready"),
                    true,
                ),
                (
                    FileReloader::new(relay.ca_files.clone(), None, None).expect("ca only"),
                    true,
                ),
                (
                    FileReloader::new_ready(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("ca and keypair ready"),
                    true,
                ),
                (
                    FileReloader::new(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("ca and keypair"),
                    true,
                ),
            ];

            for (reloader, expected) in tests {
                assert_eq!(reloader.has_custom_ca(), expected);
            }
        }

        /// Port of Go: TestReady in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_ready() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let tests = vec![
                (
                    FileReloader::new_ready(vec![], None, None).expect("empty ready"),
                    true,
                ),
                (FileReloader::new(vec![], None, None).expect("empty"), true),
                (
                    FileReloader::new_ready(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair ready"),
                    true,
                ),
                (
                    FileReloader::new(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair"),
                    false,
                ),
                (
                    FileReloader::new_ready(relay.ca_files.clone(), None, None).expect("ca ready"),
                    true,
                ),
                (
                    FileReloader::new(relay.ca_files.clone(), None, None).expect("ca only"),
                    false,
                ),
                (
                    FileReloader::new_ready(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("ca and keypair ready"),
                    true,
                ),
                (
                    FileReloader::new(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("ca and keypair"),
                    false,
                ),
            ];

            for (reloader, expected) in tests {
                assert_eq!(reloader.ready(), expected);
            }
        }

        /// Port of Go: TestKeypairAndCACertPool in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_keypair_and_ca_cert_pool() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);
            let expected_ca_pool = ca_pool_from_bytes(INITIAL_CA_CERT);

            let mut empty_ready = FileReloader::new_ready(vec![], None, None).expect("empty ready");
            let empty = FileReloader::new(vec![], None, None).expect("empty");
            let keypair_ready = FileReloader::new_ready(
                vec![],
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("keypair ready");
            let keypair = FileReloader::new(
                vec![],
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("keypair");
            let ca_ready =
                FileReloader::new_ready(relay.ca_files.clone(), None, None).expect("ca ready");
            let ca = FileReloader::new(relay.ca_files.clone(), None, None).expect("ca");
            let both_ready = FileReloader::new_ready(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("both ready");
            let both = FileReloader::new(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("both");

            let _ = empty_ready.reload();

            assert_eq!(empty.keypair_and_ca_cert_pool(), (None, None));
            assert_eq!(
                keypair_ready.keypair_and_ca_cert_pool(),
                (Some(expected_keypair.clone()), None)
            );
            assert_eq!(keypair.keypair_and_ca_cert_pool(), (None, None));
            assert_eq!(
                ca_ready.keypair_and_ca_cert_pool(),
                (None, Some(expected_ca_pool.clone()))
            );
            assert_eq!(ca.keypair_and_ca_cert_pool(), (None, None));
            assert_eq!(
                both_ready.keypair_and_ca_cert_pool(),
                (Some(expected_keypair), Some(expected_ca_pool))
            );
            assert_eq!(both.keypair_and_ca_cert_pool(), (None, None));
        }

        /// Port of Go: TestPrivilegedReload in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_privileged_reload() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);
            let expected_ca_pool = ca_pool_from_bytes(INITIAL_CA_CERT);

            let mut tests = vec![
                (
                    FileReloader::new(vec![], None, None).expect("empty"),
                    None,
                    None,
                ),
                (
                    FileReloader::new(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair"),
                    Some(expected_keypair.clone()),
                    None,
                ),
                (
                    FileReloader::new(relay.ca_files.clone(), None, None).expect("ca"),
                    None,
                    Some(expected_ca_pool.clone()),
                ),
                (
                    FileReloader::new(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("both"),
                    Some(expected_keypair),
                    Some(expected_ca_pool),
                ),
            ];

            for (reloader, expected_keypair, expected_ca) in &mut tests {
                let (prev_keypair_gen, prev_ca_gen) = reloader.generations();
                let (keypair, ca_pool) = reloader.reload().expect("reload");
                assert_eq!(&keypair, expected_keypair);
                assert_eq!(&ca_pool, expected_ca);

                let (keypair_gen, ca_gen) = reloader.generations();
                let expected_keypair_gen = if expected_keypair.is_some() {
                    prev_keypair_gen + 1
                } else {
                    prev_keypair_gen
                };
                let expected_ca_gen = if expected_ca.is_some() {
                    prev_ca_gen + 1
                } else {
                    prev_ca_gen
                };
                assert_eq!(keypair_gen, expected_keypair_gen);
                assert_eq!(ca_gen, expected_ca_gen);
                assert_eq!(reloader.keypair_and_ca_cert_pool(), (keypair, ca_pool));
            }
        }

        /// Port of Go: TestReloadKeypair in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_reload_keypair() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);

            let mut tests = vec![
                (FileReloader::new(vec![], None, None).expect("empty"), None),
                (
                    FileReloader::new(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair"),
                    Some(expected_keypair.clone()),
                ),
                (
                    FileReloader::new(relay.ca_files.clone(), None, None).expect("ca"),
                    None,
                ),
                (
                    FileReloader::new(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("both"),
                    Some(expected_keypair),
                ),
            ];

            for (reloader, expected_keypair) in &mut tests {
                let (prev_keypair_gen, _) = reloader.generations();
                let keypair = reloader.reload_keypair().expect("reload keypair");
                assert_eq!(&keypair, expected_keypair);

                let (keypair_gen, _) = reloader.generations();
                let expected_gen = if expected_keypair.is_some() {
                    prev_keypair_gen + 1
                } else {
                    prev_keypair_gen
                };
                assert_eq!(keypair_gen, expected_gen);
                let (stored_keypair, stored_ca) = reloader.keypair_and_ca_cert_pool();
                assert_eq!(stored_keypair, keypair);
                assert_eq!(stored_ca, None);
            }
        }

        /// Port of Go: TestReloadCA in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_reload_ca() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_ca_pool = ca_pool_from_bytes(INITIAL_CA_CERT);

            let mut tests = vec![
                (FileReloader::new(vec![], None, None).expect("empty"), None),
                (
                    FileReloader::new(
                        vec![],
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("keypair"),
                    None,
                ),
                (
                    FileReloader::new(relay.ca_files.clone(), None, None).expect("ca"),
                    Some(expected_ca_pool.clone()),
                ),
                (
                    FileReloader::new(
                        relay.ca_files.clone(),
                        Some(hubble.cert_file.clone()),
                        Some(hubble.privkey_file.clone()),
                    )
                    .expect("both"),
                    Some(expected_ca_pool),
                ),
            ];

            for (reloader, expected_ca) in &mut tests {
                let (_, prev_ca_gen) = reloader.generations();
                let ca_pool = reloader.reload_ca().expect("reload ca");
                assert_eq!(&ca_pool, expected_ca);

                let (_, ca_gen) = reloader.generations();
                let expected_gen = if expected_ca.is_some() {
                    prev_ca_gen + 1
                } else {
                    prev_ca_gen
                };
                assert_eq!(ca_gen, expected_gen);
                let (stored_keypair, stored_ca) = reloader.keypair_and_ca_cert_pool();
                assert_eq!(stored_keypair, None);
                assert_eq!(stored_ca, ca_pool);
            }
        }

        /// Port of Go: TestReloadError in pkg/crypto/certloader/reloader_test.go
        #[test]
        fn parity_test_reload_error() {
            let (_dir, hubble, relay) = directories();
            setup(&hubble, &relay);

            let expected_keypair = keypair_from_bytes(INITIAL_CERT, INITIAL_KEY);
            let expected_ca_pool = ca_pool_from_bytes(INITIAL_CA_CERT);

            let mut reloader = FileReloader::new_ready(
                relay.ca_files.clone(),
                Some(hubble.cert_file.clone()),
                Some(hubble.privkey_file.clone()),
            )
            .expect("ready reloader");
            assert!(reloader.ready());

            assert_eq!(
                reloader.keypair_and_ca_cert_pool(),
                (
                    Some(expected_keypair.clone()),
                    Some(expected_ca_pool.clone())
                )
            );

            fs::remove_file(&hubble.privkey_file).expect("remove private key");
            let (prev_keypair_gen, prev_ca_gen) = reloader.generations();
            let reload_result = reloader.reload();
            assert!(reload_result.is_err());

            assert_eq!(
                reloader.keypair_and_ca_cert_pool(),
                (Some(expected_keypair), Some(expected_ca_pool))
            );
            assert_eq!(reloader.generations(), (prev_keypair_gen, prev_ca_gen));
        }
    }

    const INITIAL_CA_CERT: &[u8] = br#"-----BEGIN CERTIFICATE-----
MIIDJzCCAg+gAwIBAgIQMUvUDie0mikTSp2IsrB4YjANBgkqhkiG9w0BAQsFADAe
MRwwGgYDVQQDExNodWJibGUtY2EuY2lsaXVtLmlvMB4XDTIwMTAwMTEzMjUzMVoX
DTIzMTAwMTEzMjUzMVowHjEcMBoGA1UEAxMTaHViYmxlLWNhLmNpbGl1bS5pbzCC
ASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAMTvo+CAC97XWht2cHg5BAmZ
mXwlhPcBJEUFsUs+S4TrOnsgm9rSv0OQISVZ+GyJ0bjIqmDu1FXIH/YrUPTjbslj
9/0xfxDRMLjGisF/yB+ydLjHJZ3JUpVr8hWPsiC4ykK3nZS8tW0/sezIBd9Cx517
RpKGF9qzxSim37qnu41sVF9X8KcKKB5jLGjmYMsDfWmUPPLxdJ2y3N5PAmD7Ejtc
1Acw+DS1GoxZdLv+ULdWLqtg97rxx9KCd/M5p4q3z8Zp1vgndOFWcpu1XkLH5ncl
JI1XxgU2LorGQZkkUUVsjqnuMqvld0q8PkFWIppR2D08R9/4zJCm2ysswCBC3s8C
AwEAAaNhMF8wDgYDVR0PAQH/BAQDAgKkMB0GA1UdJQQWMBQGCCsGAQUFBwMBBggr
BgEFBQcDAjAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBRcvx1oKKu4r3uIZaD5
gfqRoH2MdDANBgkqhkiG9w0BAQsFAAOCAQEATef0QXzMWq2xzdkJkZksxnE0KNE8
laEXtpLfwOIi7JjMoXWthKEmr2aB5VILmiIzoimmfEclfbCfZCfIqXhWIo8Tf54c
Csind5H8L/cyZFWIiKt8KL2UnJtpbJUndFEvHpLAIksur6FGMjlUWDay7Aoky30y
jesErGj1HyfHJ/uFwExPPjISeOaLho8HlSs2GWVGVwdj0quwDZpO1RNsjzwY/9dZ
5aHOmj879VLHjgIXZ5wmB8cEi+j/QMsJUQcck4AnbwJOHg3QNo7N/ijeXCilBmfU
/SIbm68WynGdIBXcA9lE8spxRk0u8aZ2XxWqjXNgrgOCEFb4LwFRauhpgQ==
-----END CERTIFICATE-----
"#;

    const INITIAL_CERT: &[u8] = br#"-----BEGIN CERTIFICATE-----
MIIDUzCCAjugAwIBAgIRALW5Aia05bOKS5c6pCiF6vEwDQYJKoZIhvcNAQELBQAw
HjEcMBoGA1UEAxMTaHViYmxlLWNhLmNpbGl1bS5pbzAeFw0yMDEwMDExMzI1MzJa
Fw0yMzEwMDExMzI1MzJaMCMxITAfBgNVBAMMGCouaHViYmxlLXJlbGF5LmNpbGl1
bS5pbzCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAKmhlb+yXgWK3dyq
0AyA2ZLHmH1D8Q+weeE2pNpSS9Nf3Q9/ILtQ149E4FnTh2FKrVspPo0n8AVJykev
HK3KQWQkzO/wirCaJiOEDXyB8mZuug4avJ0s/Kmde4urxp39iFUaJsAoujAwkuXZ
3dpNAGRjYRu92xUyBwjHYGSGzwKjonYGnGZJ9MfRT10W77taF7MC8ol2UpZi+VqG
uUsaD8kNDtNGpUYmFQIGdNJQz6ZHc8shQRYVtZgZ52oIy3HjfVpjM4reVjzvSaQs
+LvJKNinTP15tYiHzDFmPLQkM0I+xoXWi4kz7LZ62kn547A+yMsxd7Yqa9dufppC
PbKN4WECAwEAAaOBhjCBgzAOBgNVHQ8BAf8EBAMCBaAwHQYDVR0lBBYwFAYIKwYB
BQUHAwEGCCsGAQUFBwMCMAwGA1UdEwEB/wQCMAAwHwYDVR0jBBgwFoAUXL8daCir
uK97iGWg+YH6kaB9jHQwIwYDVR0RBBwwGoIYKi5odWJibGUtcmVsYXkuY2lsaXVt
LmlvMA0GCSqGSIb3DQEBCwUAA4IBAQB9j/tj3OO6sbkkPWPMijC0KqSFl0RtGVfl
fFEqalxs+CAchrjgFz87rpe0omYdgUKGKp2RVxzt2ibVBo1z1JliZNVz6fNLiqT/
D+/4yGdQBtqJ+Z3PvZz1HNAls/+01d8hKpw4i5krRztlVWO8ubijjkgcHwtxSRL9
Y5AAszQL1crOr5upHAHV2JdhdYV16V+eAqBVXScI0f4LZA5jJfz+032rQh7YgV7m
fWreTeQPP1XlzwAgYXQ/hoWIsl3/qt0oP0N5s9IAGxZEe8cnSKPS2jc+Egz5f6zF
jjx7jgWrWRTL+F4ZJ9G6Qku0hFwVRTbR0i+2Gm4nxfx1yx/Cxd44
-----END CERTIFICATE-----
"#;

    const INITIAL_KEY: &[u8] = br#"-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAqaGVv7JeBYrd3KrQDIDZkseYfUPxD7B54Tak2lJL01/dD38g
u1DXj0TgWdOHYUqtWyk+jSfwBUnKR68crcpBZCTM7/CKsJomI4QNfIHyZm66Dhq8
nSz8qZ17i6vGnf2IVRomwCi6MDCS5dnd2k0AZGNhG73bFTIHCMdgZIbPAqOidgac
Zkn0x9FPXRbvu1oXswLyiXZSlmL5Woa5SxoPyQ0O00alRiYVAgZ00lDPpkdzyyFB
FhW1mBnnagjLceN9WmMzit5WPO9JpCz4u8ko2KdM/Xm1iIfMMWY8tCQzQj7GhdaL
iTPstnraSfnjsD7IyzF3tipr125+mkI9so3hYQIDAQABAoIBABlu0qb1NUebdHw7
WAon33c0WdaeMyxpBz0PFlRtdlTw0JIcO2oaStd+Oiz9nBSoP6mlW22KiWAhmiR5
StF7u6YqJlfrNsAXvJQinmsGiLN28ope09y0/ATqSbW9QYA6nRA1ZY32DURgZAX2
Tl8GoIJsrAiexJQ+9fMJAZjQ5YS9iYTQUmom2JqudrpzgFWJnhTt4Et2JhbonRyB
RFrsGTBX1qD2ueW/U8pWdrLPml/vzNJNvwsPdw0rZe5tJAuYJXfStqyVT6Fd6Hb4
7Yu2pPdREEtNT1khY45ajRs4hg8LXOh6WVDaY5utSLt6Q3HdCHpwj9u8NwUQ/b0F
mlBvdwkCgYEA0/fPsSEVP22U2JLp6CPIPSO6pwDe+LC+VOsPlG+Ee18p+NRT8Whk
VxhiJ7nmWGNU2riqPlzcb8u2fP5RRcA59sPIu9htCln4t9E1mYYLpT3I8OJcPt6c
/YCoinR5rI5YpSw2hqAVGlfAI89JBOpFTj1ium17ON1Pgx2bLVfoND8CgYEAzN5a
/zcAAkYzDE/gJuGP/OI8v2JIASHBgkbCn8W9YuaahzPnvN5MTjRoggClu3sqim1t
pgE4zP+mSr6XNC4WgVOJknIyCIwPsH3xrcUiApcDDZJYbmteuhAX6C9dyraSSroY
BrP6sygL1QvIMKEsviZmn13xJYAI65gT7paGAl8CgYAnnFykljEZTEoPeszZQ66M
tluQD9qbELRQvCiKLZjNUUhPpqYVK9PsbrMRB21jQRS/VtkBlGrhPWlZzFC1vylV
0tp1OAmQcKXI/ACPMvyEIZqmYTapzQH7YYqdbQy70VIBc9SwrcOjy5gtWPQlRf4z
k8caXZE0XC8aqnKwM4hCEwKBgAdGMebz9f0ervtV7riSs8Ef61ZEUBgyMaPFjW2M
4N+dHomEb0sGfaEdPUS4byoMAoOtxQHq8zBcN3RZ9hZ1OHlZFP5tLZeeGYSDxEwO
PtnmsMYPlzI8f72NirvEysjC2MjseKPsSg+IcXscEvyfDG6oAGbSOBjDxg1Pdg23
rIRzAoGAYHc7ILNRbeD0bIH55JPQ7iu2DXNTW1KVhIkx2INPcbK7HgO2hHH52cEg
ck9YU3p58lvJC3iA/FwczkEgxt9h8EwJMdsNK1abzMNHUHu52udA6YZbrKs22OiT
whwz8ZXadaGGom3X1ZiHyCHnMvK26QUmUS0sa9t2RfSheawFpLo=
-----END RSA PRIVATE KEY-----
"#;
}
