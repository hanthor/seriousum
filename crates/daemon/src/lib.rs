//! Pure daemon configuration and lifecycle models.
//!
//! This crate ports the pure data model pieces from Cilium's daemon package
//! and includes a minimal runtime loop for long-running agent startup.

pub mod health;
pub mod runtime;

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use seriousum_config::RuntimeConfig;

pub use health::{HealthStatus, ReadinessState, SharedHealth, new_health, set_ready, set_stopping};
pub use runtime::{DaemonRuntime, ShutdownSignal};

/// Errors returned by pure daemon configuration and lifecycle helpers.
#[derive(Debug, Error)]
pub enum DaemonError {
    /// A configuration value was invalid.
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    /// The daemon was queried before it reached a ready phase.
    #[error("daemon not ready: current phase is {0:?}")]
    NotReady(DaemonPhase),
    /// JSON or other serde-based conversion failed.
    #[error("config serialization failed: {0}")]
    SerializationError(String),
}

/// Result type used by daemon helpers.
pub type Result<T> = std::result::Result<T, DaemonError>;

fn default_true() -> bool {
    true
}

/// Encapsulation mode for pod-to-pod traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TunnelMode {
    /// Use VXLAN tunneling.
    #[default]
    Vxlan,
    /// Use Geneve tunneling.
    Geneve,
    /// Disable tunneling.
    Disabled,
}

/// Policy enforcement behavior for the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PolicyEnforcementMode {
    /// Follow endpoint and cluster defaults.
    #[default]
    Default,
    /// Always enforce policy.
    Always,
    /// Never enforce policy.
    Never,
}

impl FromStr for PolicyEnforcementMode {
    type Err = DaemonError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "default" | "Default" => Ok(Self::Default),
            "always" | "Always" => Ok(Self::Always),
            "never" | "Never" => Ok(Self::Never),
            other => Err(DaemonError::InvalidConfig(format!(
                "unknown policy enforcement mode: {other}"
            ))),
        }
    }
}

/// Backend used to allocate security identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[allow(clippy::upper_case_acronyms)]
pub enum IdentityAllocationMode {
    /// Allocate identities via kvstore.
    #[default]
    Kvstore,
    /// Allocate identities via Kubernetes CRDs.
    CRD,
}

/// NodePort forwarding behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NodePortMode {
    /// Always perform SNAT.
    Snat,
    /// Use hybrid SNAT/DSR behavior.
    #[default]
    Hybrid,
    /// Use direct server return.
    Dsr,
}

/// Datapath attachment mode for endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DatapathMode {
    /// Use veth pairs.
    #[default]
    Veth,
    /// Use ipvlan devices.
    Ipvlan,
    /// Use netkit attachment.
    Netkit,
}

/// Pure daemon configuration ported from `option.DaemonConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct DaemonConfig {
    /// Whether IPv4 support is enabled.
    pub ipv4_enabled: bool,
    /// Whether IPv6 support is enabled.
    pub ipv6_enabled: bool,
    /// Tunnel encapsulation mode.
    pub tunnel_mode: TunnelMode,
    /// Native routing CIDR when tunneling is disabled.
    pub native_routing_cidr: Option<String>,
    /// Cluster name advertised by the agent.
    pub cluster_name: String,
    /// Numeric cluster identifier.
    pub cluster_id: u8,
    /// Policy enforcement mode.
    pub policy_enforcement_mode: PolicyEnforcementMode,
    /// Whether policy audit mode is enabled.
    pub enable_policy_audit_mode: bool,
    /// Identity allocation backend.
    pub identity_allocation_mode: IdentityAllocationMode,
    /// Grace period for identity changes in milliseconds.
    pub identity_change_grace_period_ms: u64,
    /// Whether topology-aware service routing is enabled.
    pub enable_service_topology: bool,
    /// Whether ExternalIPs are enabled.
    pub enable_external_ips: bool,
    /// NodePort forwarding mode.
    pub node_port_mode: NodePortMode,
    /// Whether Hubble is enabled.
    pub enable_hubble: bool,
    /// Whether Kubernetes watchers should be started by the runtime.
    #[serde(default = "default_true")]
    pub enable_k8s_integration: bool,
    /// Address used by the Hubble server.
    pub hubble_listen_address: String,
    /// Size of the Hubble flow buffer.
    pub hubble_flow_buffer_size: u32,
    /// Endpoint datapath mode.
    pub datapath_mode: DatapathMode,
    /// Dynamic BPF map sizing ratio.
    pub bpf_map_dynamic_size_ratio: f64,
    /// Maximum TCP conntrack entries.
    pub bpf_ct_tcp_max: u32,
    /// Maximum non-TCP conntrack entries.
    pub bpf_ct_any_max: u32,
    /// Additional labels applied to the agent.
    pub agent_labels: Vec<String>,
    /// Configuration directory path.
    pub config_dir: String,
    /// Runtime state directory path.
    pub state_dir: String,
    /// Default daemon log level.
    pub log_level: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            ipv4_enabled: true,
            ipv6_enabled: false,
            tunnel_mode: TunnelMode::Vxlan,
            native_routing_cidr: None,
            cluster_name: "default".into(),
            cluster_id: 0,
            policy_enforcement_mode: PolicyEnforcementMode::Default,
            enable_policy_audit_mode: false,
            identity_allocation_mode: IdentityAllocationMode::Kvstore,
            identity_change_grace_period_ms: 5_000,
            enable_service_topology: false,
            enable_external_ips: true,
            node_port_mode: NodePortMode::Hybrid,
            enable_hubble: false,
            enable_k8s_integration: true,
            hubble_listen_address: "localhost:4244".into(),
            hubble_flow_buffer_size: 4_096,
            datapath_mode: DatapathMode::Veth,
            bpf_map_dynamic_size_ratio: 0.0025,
            bpf_ct_tcp_max: 524_288,
            bpf_ct_any_max: 262_144,
            agent_labels: vec![],
            config_dir: "/var/lib/cilium".into(),
            state_dir: "/var/run/cilium".into(),
            log_level: "info".into(),
        }
    }
}

/// High-level lifecycle phase of the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonPhase {
    /// Agent is initializing.
    Starting,
    /// Waiting for Kubernetes connectivity.
    WaitingForK8s,
    /// Waiting for initial identity allocation.
    WaitingForIdentity,
    /// Agent is fully operational.
    Running,
    /// Agent is shutting down.
    Stopping,
    /// Agent has stopped.
    Stopped,
}

/// Current daemon runtime status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonStatus {
    /// Current daemon lifecycle phase.
    pub phase: DaemonPhase,
    /// Node name associated with the daemon.
    pub node_name: String,
    /// Reported Cilium/seriousum version.
    pub cilium_version: String,
    /// Kernel version string if known.
    pub kernel_version: String,
    /// Primary IPv4 address.
    pub ipv4_address: Option<Ipv4Addr>,
    /// Primary IPv6 address.
    pub ipv6_address: Option<Ipv6Addr>,
    /// Number of managed endpoints.
    pub endpoint_count: u32,
    /// Number of managed identities.
    pub identity_count: u32,
    /// Number of installed policies.
    pub policy_count: u32,
    /// Daemon uptime in seconds.
    pub uptime_secs: u64,
}

impl DaemonStatus {
    /// Creates a new daemon status for the provided node.
    pub fn new(node_name: impl Into<String>) -> Self {
        Self {
            phase: DaemonPhase::Starting,
            node_name: node_name.into(),
            cilium_version: env!("CARGO_PKG_VERSION").to_string(),
            kernel_version: String::from("unknown"),
            ipv4_address: None,
            ipv6_address: None,
            endpoint_count: 0,
            identity_count: 0,
            policy_count: 0,
            uptime_secs: 0,
        }
    }

    /// Returns `true` when the daemon is fully operational.
    pub fn is_ready(&self) -> bool {
        self.phase == DaemonPhase::Running
    }
}

/// In-memory daemon state container.
#[derive(Debug, Clone)]
pub struct Daemon {
    config: Arc<DaemonConfig>,
    status: Arc<RwLock<DaemonStatus>>,
}

impl Daemon {
    /// Creates a new in-memory daemon model.
    pub fn new(config: DaemonConfig, node_name: impl Into<String>) -> Self {
        Self {
            config: Arc::new(config),
            status: Arc::new(RwLock::new(DaemonStatus::new(node_name))),
        }
    }

    /// Returns the daemon configuration.
    pub fn config(&self) -> &DaemonConfig {
        self.config.as_ref()
    }

    /// Returns a snapshot of the current daemon status.
    pub async fn status(&self) -> DaemonStatus {
        self.status.read().await.clone()
    }

    /// Updates the current daemon phase.
    pub async fn set_phase(&self, phase: DaemonPhase) {
        debug!(?phase, "updating daemon phase");
        self.status.write().await.phase = phase;
    }

    /// Returns whether the daemon is ready.
    pub async fn is_ready(&self) -> bool {
        self.status.read().await.is_ready()
    }

    /// Increments the endpoint count.
    pub async fn increment_endpoint_count(&self) {
        self.status.write().await.endpoint_count += 1;
    }

    /// Decrements the endpoint count without underflowing.
    pub async fn decrement_endpoint_count(&self) {
        let mut status = self.status.write().await;
        status.endpoint_count = status.endpoint_count.saturating_sub(1);
    }
}

/// Command-line arguments for the daemon.
#[derive(Debug, Clone, Parser)]
#[command(
    name = "seriousum-daemon",
    version,
    about = "Run the seriousum daemon",
    disable_help_flag = true,
    ignore_errors = true
)]
pub struct Cli {
    /// Optional configuration file.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Additional daemon flags passed by the Cilium Helm chart and scripts.
    /// These are accepted for compatibility and ignored until implemented.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    #[allow(clippy::pub_underscore_fields)]
    pub _extra: Vec<String>,
}

/// Returns the default configuration file path.
pub fn default_config_path() -> PathBuf {
    PathBuf::from("seriousum.json")
}

fn load_config_from_path(path: &Path) -> anyhow::Result<RuntimeConfig> {
    RuntimeConfig::load(path)
}

/// Loads daemon configuration from disk or falls back to defaults.
pub fn load_config(path: Option<PathBuf>) -> anyhow::Result<RuntimeConfig> {
    match path {
        Some(path) if path.exists() => load_config_from_path(path.as_path()),
        Some(path) => {
            warn!(path = %path.display(), "configuration file not found; using defaults");
            Ok(RuntimeConfig::default())
        }
        None => {
            let path = default_config_path();
            if path.exists() {
                load_config_from_path(path.as_path())
            } else {
                Ok(RuntimeConfig::default())
            }
        }
    }
}

/// Initializes tracing for daemon binaries.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .try_init();
}

/// Maps runtime configuration into the daemon runtime configuration model.
pub fn daemon_config_from_runtime_config(config: &RuntimeConfig) -> anyhow::Result<DaemonConfig> {
    let cluster_id = u8::try_from(config.agent.cluster_id)
        .map_err(|_| anyhow::anyhow!("cluster id {} exceeds u8 range", config.agent.cluster_id))?;

    Ok(DaemonConfig {
        ipv4_enabled: config.agent.enable_ipv4,
        ipv6_enabled: config.agent.enable_ipv6,
        cluster_name: config.agent.cluster_name.clone(),
        cluster_id,
        ..DaemonConfig::default()
    })
}

/// Executes the daemon binary entrypoint as a long-running runtime loop.
pub async fn execute(cli: Cli) -> anyhow::Result<()> {
    init_tracing();

    let runtime_config = load_config(cli.config)?;
    let config = daemon_config_from_runtime_config(&runtime_config)?;
    let runtime = DaemonRuntime::new(config);
    runtime
        .run()
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    Ok(())
}

/// Mutable runtime options toggled during daemon setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MutableDaemonOptions {
    /// Emit drop notifications.
    pub drop_notify: bool,
    /// Emit trace notifications.
    pub trace_notify: bool,
    /// Emit policy verdict notifications.
    pub policy_verdict_notify: bool,
}

/// Runtime configuration options initialized during daemon setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfigOptions {
    /// Identity allocation mode.
    pub identity_allocation_mode: IdentityAllocationMode,
    /// Enable dry mode for control-plane only tests.
    pub dry_mode: bool,
    /// Mutable daemon options.
    pub mutable_options: MutableDaemonOptions,
}

impl Default for RuntimeConfigOptions {
    fn default() -> Self {
        Self {
            identity_allocation_mode: IdentityAllocationMode::CRD,
            dry_mode: false,
            mutable_options: MutableDaemonOptions::default(),
        }
    }
}

/// Applies daemon test-style configuration bootstrapping.
///
/// Ported from `setupConfigOptions` in `daemon/cmd/daemon_test.go`.
pub fn setup_config_options(options: &mut RuntimeConfigOptions) {
    options.identity_allocation_mode = IdentityAllocationMode::Kvstore;
    options.dry_mode = true;
    options.mutable_options.drop_notify = true;
    options.mutable_options.trace_notify = true;
    options.mutable_options.policy_verdict_notify = true;
}

/// Minimal policy rule entry for daemon policy update parity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEntry {
    /// Rule identifier.
    pub name: String,
}

/// Policy update request mirrored from `policyTypes.PolicyUpdate`.
#[derive(Debug)]
pub struct PolicyUpdate {
    /// Policy rules being updated.
    pub rules: Vec<PolicyEntry>,
    /// Update source or resource identifier.
    pub resource: String,
    /// Completion signal channel carrying policy revision.
    pub done_chan: Option<std::sync::mpsc::SyncSender<u64>>,
}

impl PolicyUpdate {
    /// Creates a policy update.
    pub fn new(resource: impl Into<String>, rules: Vec<PolicyEntry>) -> Self {
        Self {
            rules,
            resource: resource.into(),
            done_chan: None,
        }
    }
}

/// Interface for applying policy updates.
pub trait PolicyImporter: Send + Sync {
    /// Apply a policy update and eventually signal completion on `done_chan`.
    fn update_policy(&self, update: PolicyUpdate) -> Result<()>;
}

/// Convenience wrapper that adds a single policy source update.
///
/// Ported from `policyImport` in `daemon/cmd/daemon_test.go`.
pub fn policy_import(importer: &dyn PolicyImporter, rules: Vec<PolicyEntry>) -> Result<u64> {
    update_policy(importer, PolicyUpdate::new("policy", rules))
}

/// Convenience wrapper that synchronously performs a policy update.
///
/// Ported from `updatePolicy` in `daemon/cmd/daemon_test.go`.
pub fn update_policy(importer: &dyn PolicyImporter, mut update: PolicyUpdate) -> Result<u64> {
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    update.done_chan = Some(done_tx);

    importer.update_policy(update)?;
    done_rx
        .recv()
        .map_err(|_| DaemonError::InvalidConfig("policy update did not signal completion".into()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;

    fn test_artifact_path(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-artifacts");
        std::fs::create_dir_all(&dir).expect("create test artifacts directory");
        dir.join(format!(
            "{name}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn test_default_config() {
        let cfg = DaemonConfig::default();
        assert!(cfg.ipv4_enabled);
        assert!(!cfg.ipv6_enabled);
        assert!(cfg.enable_k8s_integration);
        assert_eq!(cfg.tunnel_mode, TunnelMode::Vxlan);
        assert_eq!(cfg.cluster_name, "default");
    }

    #[test]
    fn test_policy_enforcement_mode_parse() {
        assert_eq!(
            "always".parse::<PolicyEnforcementMode>().unwrap(),
            PolicyEnforcementMode::Always
        );
        assert_eq!(
            "Never".parse::<PolicyEnforcementMode>().unwrap(),
            PolicyEnforcementMode::Never
        );
        assert!("bogus".parse::<PolicyEnforcementMode>().is_err());
    }

    #[tokio::test]
    async fn test_daemon_phase_transitions() {
        let daemon = Daemon::new(DaemonConfig::default(), "node1");
        assert!(!daemon.is_ready().await);
        daemon.set_phase(DaemonPhase::Running).await;
        assert!(daemon.is_ready().await);
        daemon.set_phase(DaemonPhase::Stopping).await;
        assert!(!daemon.is_ready().await);
    }

    #[tokio::test]
    async fn test_endpoint_count() {
        let daemon = Daemon::new(DaemonConfig::default(), "node1");
        daemon.increment_endpoint_count().await;
        daemon.increment_endpoint_count().await;
        assert_eq!(daemon.status().await.endpoint_count, 2);
        daemon.decrement_endpoint_count().await;
        assert_eq!(daemon.status().await.endpoint_count, 1);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let cfg = DaemonConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: DaemonConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cfg.cluster_name, back.cluster_name);
        assert_eq!(cfg.ipv4_enabled, back.ipv4_enabled);
        assert_eq!(cfg.enable_k8s_integration, back.enable_k8s_integration);
    }

    #[test]
    fn test_config_serde_defaults_enable_k8s_integration() {
        let cfg: DaemonConfig = serde_json::from_str("{}")
            .expect("deserialize config with missing k8s integration field");
        assert!(cfg.enable_k8s_integration);
    }

    #[test]
    fn test_daemon_status_not_ready_by_default() {
        let status = DaemonStatus::new("my-node");
        assert!(!status.is_ready());
        assert_eq!(status.phase, DaemonPhase::Starting);
    }

    #[test]
    fn cli_parses() {
        let cli = Cli::parse_from(["seriousum-daemon"]);
        assert!(cli.config.is_none());
    }

    #[test]
    fn cli_parses_with_config() {
        let cli = Cli::parse_from(["seriousum-daemon", "--config", "test.json"]);
        assert_eq!(cli.config, Some(PathBuf::from("test.json")));
    }

    #[test]
    fn load_config_uses_defaults_when_explicit_path_is_missing() {
        let path = test_artifact_path("missing");
        let config = load_config(Some(path)).expect("load default config");
        assert_eq!(config, RuntimeConfig::default());
    }

    #[test]
    fn load_config_uses_defaults_when_default_path_is_missing() {
        let original_dir = std::env::current_dir().expect("current dir");
        let temp_dir = test_artifact_path("cwd");
        std::fs::create_dir_all(&temp_dir).expect("create test directory");
        std::env::set_current_dir(&temp_dir).expect("set test directory");

        let config = load_config(None).expect("load default config");

        std::env::set_current_dir(original_dir).expect("restore cwd");
        let _ = std::fs::remove_dir_all(&temp_dir);
        assert_eq!(config, RuntimeConfig::default());
    }

    #[test]
    fn parity_setup_config_options() {
        let mut options = RuntimeConfigOptions::default();
        setup_config_options(&mut options);

        assert_eq!(
            options.identity_allocation_mode,
            IdentityAllocationMode::Kvstore
        );
        assert!(options.dry_mode);
        assert!(options.mutable_options.drop_notify);
        assert!(options.mutable_options.trace_notify);
        assert!(options.mutable_options.policy_verdict_notify);
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedPolicyUpdate {
        rules: Vec<PolicyEntry>,
        resource: String,
    }

    #[derive(Default)]
    struct MockPolicyImporter {
        revision: u64,
        updates: Mutex<Vec<CapturedPolicyUpdate>>,
    }

    impl MockPolicyImporter {
        fn with_revision(revision: u64) -> Self {
            Self {
                revision,
                updates: Mutex::new(vec![]),
            }
        }
    }

    impl PolicyImporter for MockPolicyImporter {
        fn update_policy(&self, mut update: PolicyUpdate) -> Result<()> {
            if let Some(done_chan) = update.done_chan.take() {
                done_chan.send(self.revision).map_err(|_| {
                    DaemonError::InvalidConfig("failed to deliver policy revision".into())
                })?;
            } else {
                return Err(DaemonError::InvalidConfig(
                    "policy update missing completion channel".into(),
                ));
            }

            let mut updates = self
                .updates
                .lock()
                .map_err(|_| DaemonError::InvalidConfig("mock importer lock poisoned".into()))?;
            updates.push(CapturedPolicyUpdate {
                rules: update.rules,
                resource: update.resource,
            });

            Ok(())
        }
    }

    #[test]
    fn parity_policy_import() {
        let importer = MockPolicyImporter::with_revision(42);
        let rules = vec![PolicyEntry {
            name: "allow-all".to_string(),
        }];

        let revision = policy_import(&importer, rules.clone()).unwrap();
        assert_eq!(revision, 42);

        let updates = importer.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].resource, "policy");
        assert_eq!(updates[0].rules, rules);
    }

    #[test]
    fn parity_update_policy() {
        let importer = MockPolicyImporter::with_revision(99);
        let update = PolicyUpdate::new(
            "custom-resource",
            vec![PolicyEntry {
                name: "rule-a".to_string(),
            }],
        );

        let revision = update_policy(&importer, update).unwrap();
        assert_eq!(revision, 99);

        let updates = importer.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].resource, "custom-resource");
        assert_eq!(
            updates[0].rules,
            vec![PolicyEntry {
                name: "rule-a".to_string(),
            }]
        );
    }
}
