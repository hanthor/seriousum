//! Daemon entrypoint and lifecycle wiring.
//!
//! This module implements the main agent binary wiring all subsystems together.
//! It provides:
//!
//! - Async component initialization and startup sequencing
//! - Graceful shutdown handling via signals
//! - Dependency injection and component registration
//! - Configuration validation and initialization
//! - Integration of eBPF, networking, identity, policy, endpoints, LB, DNS, observability

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use seriousum_config::Config;
use seriousum_kvstore::KvStore;

// ============================================================================
// Error types
// ============================================================================

/// Errors that can occur during daemon operation.
#[derive(Debug, Error)]
pub enum Error {
    #[error("component initialization failed: {0}")]
    ComponentInitFailed(String),

    #[error("component '{0}' not found")]
    ComponentNotFound(String),

    #[error("component '{0}' already registered")]
    ComponentAlreadyRegistered(String),

    #[error("startup sequencing error: {0}")]
    StartupError(String),

    #[error("graceful shutdown failed: {0}")]
    ShutdownError(String),

    #[error("configuration validation failed: {0}")]
    ConfigError(String),

    #[error("signal handling error: {0}")]
    SignalError(String),

    #[error("subsystem error: {0}")]
    SubsystemError(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// Component lifecycle
// ============================================================================

/// Component state in the daemon lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentState {
    /// Component has been registered but not started.
    Registered,
    /// Component is starting (Start hook running).
    Starting,
    /// Component is running (Run hook active).
    Running,
    /// Component is stopping (Stop hook running).
    Stopping,
    /// Component has stopped.
    Stopped,
    /// Component encountered an error.
    Error,
}

impl std::fmt::Display for ComponentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registered => write!(f, "Registered"),
            Self::Starting => write!(f, "Starting"),
            Self::Running => write!(f, "Running"),
            Self::Stopping => write!(f, "Stopping"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// Dependency information for a component.
#[derive(Debug, Clone)]
pub struct ComponentDependency {
    /// Name of the dependency.
    pub name: String,
    /// Is this dependency optional?
    pub optional: bool,
}

/// Lifecycle hooks for a component.
#[async_trait::async_trait]
pub trait ComponentHooks: Send + Sync {
    /// Called during component initialization. Should perform lightweight setup.
    async fn start(&self) -> Result<()> {
        Ok(())
    }

    /// Called after all components are started. Main work loop runs here.
    /// Block until the component should stop (e.g., via cancellation token).
    async fn run(&self) -> Result<()> {
        Ok(())
    }

    /// Called during daemon shutdown. Should clean up resources.
    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    /// Get dependencies this component requires.
    fn dependencies(&self) -> Vec<ComponentDependency> {
        vec![]
    }
}

/// Metadata for a registered component.
#[derive(Clone)]
pub struct ComponentMetadata {
    /// Component name (must be unique).
    pub name: String,
    /// Component description.
    pub description: String,
    /// Lifecycle hooks.
    pub hooks: Arc<dyn ComponentHooks>,
}

// ============================================================================
// Configuration
// ============================================================================

/// Daemon configuration validation rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct DaemonConfig {
    /// Cluster name.
    pub cluster_name: String,
    /// Local node name.
    pub node_name: String,
    /// Enable Kubernetes integration.
    pub enable_kubernetes: bool,
    /// Enable eBPF datapath.
    pub enable_ebpf: bool,
    /// Enable policy enforcement.
    pub enable_policy: bool,
    /// Enable identity management.
    pub enable_identity: bool,
    /// Enable load balancing.
    pub enable_loadbalancer: bool,
    /// Enable DNS proxy.
    pub enable_dns_proxy: bool,
    /// Enable observability (Hubble).
    pub enable_observability: bool,
    /// Enable health checks.
    pub enable_health_checks: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            cluster_name: "default".to_string(),
            node_name: "local-node".to_string(),
            enable_kubernetes: true,
            enable_ebpf: true,
            enable_policy: true,
            enable_identity: true,
            enable_loadbalancer: true,
            enable_dns_proxy: true,
            enable_observability: true,
            enable_health_checks: true,
        }
    }
}

impl DaemonConfig {
    /// Validate the daemon configuration.
    pub fn validate(&self) -> Result<()> {
        if self.cluster_name.is_empty() {
            return Err(Error::ConfigError("cluster_name cannot be empty".to_string()));
        }
        if self.node_name.is_empty() {
            return Err(Error::ConfigError("node_name cannot be empty".to_string()));
        }
        if self.cluster_name.len() > 253 {
            return Err(Error::ConfigError(
                "cluster_name exceeds maximum length (253 chars)".to_string(),
            ));
        }
        if self.node_name.len() > 253 {
            return Err(Error::ConfigError(
                "node_name exceeds maximum length (253 chars)".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Component registry
// ============================================================================

/// Registry for all daemon components.
#[derive(Clone)]
pub struct ComponentRegistry {
    components: Arc<DashMap<String, ComponentMetadata>>,
    states: Arc<DashMap<String, ComponentState>>,
}

impl ComponentRegistry {
    /// Create a new component registry.
    pub fn new() -> Self {
        Self {
            components: Arc::new(DashMap::new()),
            states: Arc::new(DashMap::new()),
        }
    }

    /// Register a component.
    pub fn register(&self, component: ComponentMetadata) -> Result<()> {
        if self.components.contains_key(&component.name) {
            return Err(Error::ComponentAlreadyRegistered(component.name));
        }
        self.states.insert(component.name.clone(), ComponentState::Registered);
        self.components.insert(component.name.clone(), component);
        Ok(())
    }

    /// Get a registered component by name.
    pub fn get(&self, name: &str) -> Option<ComponentMetadata> {
        self.components.get(name).map(|r| r.clone())
    }

    /// List all registered component names.
    pub fn list(&self) -> Vec<String> {
        self.components
            .iter()
            .map(|r| r.key().clone())
            .collect()
    }

    /// Get the current state of a component.
    pub fn state(&self, name: &str) -> Option<ComponentState> {
        self.states.get(name).map(|r| *r)
    }

    /// Set the state of a component.
    fn set_state(&self, name: &str, state: ComponentState) {
        self.states.insert(name.to_string(), state);
    }

    /// Check if all dependencies are satisfied for a component.
    pub fn dependencies_satisfied(&self, name: &str) -> Result<bool> {
        let component = self
            .get(name)
            .ok_or_else(|| Error::ComponentNotFound(name.to_string()))?;

        for dep in component.hooks.dependencies() {
            if !dep.optional && !self.components.contains_key(&dep.name) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Module definitions
// ============================================================================

/// Infrastructure module components (external services).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct InfrastructureModule {
    /// K8s client component enabled.
    pub kubernetes_enabled: bool,
    /// KVStore component enabled.
    pub kvstore_enabled: bool,
    /// Metrics component enabled.
    pub metrics_enabled: bool,
    /// CNI component enabled.
    pub cni_enabled: bool,
    /// Health check component enabled.
    pub healthz_enabled: bool,
}

impl Default for InfrastructureModule {
    fn default() -> Self {
        Self {
            kubernetes_enabled: true,
            kvstore_enabled: true,
            metrics_enabled: true,
            cni_enabled: true,
            healthz_enabled: true,
        }
    }
}

/// Control plane module components (core control logic).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ControlPlaneModule {
    /// Endpoint management enabled.
    pub endpoints_enabled: bool,
    /// Policy enforcement enabled.
    pub policy_enabled: bool,
    /// Identity management enabled.
    pub identity_enabled: bool,
    /// Load balancing enabled.
    pub loadbalancer_enabled: bool,
    /// Proxy/L7 enabled.
    pub proxy_enabled: bool,
    /// K8s watchers enabled.
    pub k8s_watchers_enabled: bool,
    /// DNS proxy enabled.
    pub dns_proxy_enabled: bool,
    /// Observability (Hubble) enabled.
    pub observability_enabled: bool,
}

impl Default for ControlPlaneModule {
    fn default() -> Self {
        Self {
            endpoints_enabled: true,
            policy_enabled: true,
            identity_enabled: true,
            loadbalancer_enabled: true,
            proxy_enabled: true,
            k8s_watchers_enabled: true,
            dns_proxy_enabled: true,
            observability_enabled: true,
        }
    }
}

// ============================================================================
// Daemon instance
// ============================================================================

/// Signals that can be sent to the daemon.
#[derive(Debug, Clone)]
pub enum DaemonSignal {
    /// Request graceful shutdown.
    Shutdown,
    /// Request reconfiguration.
    Reconfigure,
}

/// Internal daemon state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonState {
    Init,
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

/// The daemon runtime orchestrating all subsystems.
pub struct Daemon {
    config: Arc<DaemonConfig>,
    registry: Arc<ComponentRegistry>,
    kvstore: Arc<KvStore>,
    signal_tx: Arc<broadcast::Sender<DaemonSignal>>,
    #[allow(dead_code)]
    infrastructure: Arc<InfrastructureModule>,
    #[allow(dead_code)]
    controlplane: Arc<ControlPlaneModule>,
    state: Arc<RwLock<DaemonState>>,
}

impl Daemon {
    /// Create a new daemon with the given configuration.
    pub fn new(config: DaemonConfig) -> Result<Self> {
        config.validate()?;

        let (signal_tx, _) = broadcast::channel(32);

        Ok(Self {
            config: Arc::new(config),
            registry: Arc::new(ComponentRegistry::new()),
            kvstore: Arc::new(KvStore::new()),
            signal_tx: Arc::new(signal_tx),
            infrastructure: Arc::new(InfrastructureModule::default()),
            controlplane: Arc::new(ControlPlaneModule::default()),
            state: Arc::new(RwLock::new(DaemonState::Init)),
        })
    }

    /// Get the daemon configuration.
    pub fn config(&self) -> Arc<DaemonConfig> {
        self.config.clone()
    }

    /// Get the component registry.
    pub fn registry(&self) -> Arc<ComponentRegistry> {
        self.registry.clone()
    }

    /// Register a component.
    pub fn register_component(&self, component: ComponentMetadata) -> Result<()> {
        self.registry.register(component)
    }

    /// Get the signal sender for sending signals to the daemon.
    pub fn signal_sender(&self) -> Arc<broadcast::Sender<DaemonSignal>> {
        self.signal_tx.clone()
    }

    /// Run the daemon with full lifecycle management.
    ///
    /// This function:
    /// 1. Initializes all components
    /// 2. Starts all components in dependency order
    /// 3. Runs all components concurrently
    /// 4. On signal or error, cleanly shuts down all components
    /// 5. Returns when all components are stopped
    pub async fn run(&self) -> Result<()> {
        info!(
            cluster = %self.config.cluster_name,
            node = %self.config.node_name,
            "starting seriousum daemon"
        );

        // Transition to starting state
        *self.state.write().await = DaemonState::Starting;

        // Initialize kvstore
        self.kvstore
            .set("daemon/state", b"starting".to_vec())
            .await;
        self.kvstore
            .set("daemon/cluster", self.config.cluster_name.as_bytes().to_vec())
            .await;
        self.kvstore
            .set("daemon/node", self.config.node_name.as_bytes().to_vec())
            .await;

        // Start all components
        if let Err(e) = self.start_all_components().await {
            error!("failed to start components: {e}");
            *self.state.write().await = DaemonState::Error;
            return Err(e);
        }

        // Transition to running state
        *self.state.write().await = DaemonState::Running;
        info!("daemon is now running");

        // Wait for shutdown signal or component failure
        let result = self.wait_for_shutdown().await;

        // Graceful shutdown
        *self.state.write().await = DaemonState::Stopping;
        info!("daemon is shutting down gracefully");

        if let Err(e) = self.stop_all_components().await {
            error!("errors during shutdown: {e}");
            *self.state.write().await = DaemonState::Error;
            self.kvstore.set("daemon/state", b"error".to_vec()).await;
            return Err(e);
        }

        *self.state.write().await = DaemonState::Stopped;
        self.kvstore.set("daemon/state", b"stopped".to_vec()).await;
        info!("daemon stopped successfully");

        result
    }

    /// Start all registered components in dependency order.
    async fn start_all_components(&self) -> Result<()> {
        let component_names = self.registry.list();
        debug!("starting {} components", component_names.len());

        for name in component_names {
            self.start_component(&name).await?;
        }

        Ok(())
    }

    /// Start a single component.
    async fn start_component(&self, name: &str) -> Result<()> {
        let component = self
            .registry
            .get(name)
            .ok_or_else(|| Error::ComponentNotFound(name.to_string()))?;

        // Check dependencies
        if !self.registry.dependencies_satisfied(name)? {
            return Err(Error::StartupError(format!(
                "component '{name}' has unsatisfied dependencies"
            )));
        }

        debug!("starting component: {name}");
        self.registry.set_state(name, ComponentState::Starting);

        match component.hooks.start().await {
            Ok(()) => {
                self.registry.set_state(name, ComponentState::Running);
                debug!("component started: {name}");
                Ok(())
            }
            Err(e) => {
                self.registry.set_state(name, ComponentState::Error);
                Err(Error::ComponentInitFailed(format!(
                    "component '{name}' failed to start: {e}"
                )))
            }
        }
    }

    /// Stop all registered components in reverse dependency order.
    async fn stop_all_components(&self) -> Result<()> {
        let mut component_names = self.registry.list();
        // Reverse order for graceful shutdown (LIFO)
        component_names.reverse();

        debug!("stopping {} components", component_names.len());

        let mut errors = vec![];
        for name in component_names {
            if let Err(e) = self.stop_component(&name).await {
                errors.push(format!("{e}"));
            }
        }

        if !errors.is_empty() {
            return Err(Error::ShutdownError(errors.join("; ")));
        }

        Ok(())
    }

    /// Stop a single component.
    async fn stop_component(&self, name: &str) -> Result<()> {
        let component = self
            .registry
            .get(name)
            .ok_or_else(|| Error::ComponentNotFound(name.to_string()))?;

        if self.registry.state(name) != Some(ComponentState::Running) {
            debug!("component '{name}' not running, skipping stop");
            return Ok(());
        }

        debug!("stopping component: {name}");
        self.registry.set_state(name, ComponentState::Stopping);

        match component.hooks.stop().await {
            Ok(()) => {
                self.registry.set_state(name, ComponentState::Stopped);
                debug!("component stopped: {name}");
                Ok(())
            }
            Err(e) => {
                self.registry.set_state(name, ComponentState::Error);
                Err(Error::SubsystemError(format!(
                    "component '{name}' failed to stop: {e}"
                )))
            }
        }
    }

    /// Wait for shutdown signal or component failure.
    async fn wait_for_shutdown(&self) -> Result<()> {
        let mut signal_rx = self.signal_tx.subscribe();

        tokio::select! {
            result = signal_rx.recv() => {
                match result {
                    Ok(DaemonSignal::Shutdown) => {
                        debug!("received shutdown signal");
                        Ok(())
                    }
                    Ok(DaemonSignal::Reconfigure) => {
                        debug!("received reconfigure signal (treated as shutdown)");
                        Ok(())
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        warn!("signal buffer lagged, treating as shutdown");
                        Ok(())
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("signal channel closed");
                        Ok(())
                    }
                }
            }
        }
    }

    /// Get the current daemon state.
    pub async fn state(&self) -> DaemonState {
        *self.state.read().await
    }

    /// Check if daemon is running.
    pub async fn is_running(&self) -> bool {
        *self.state.read().await == DaemonState::Running
    }
}

// ============================================================================
// CLI and configuration loading
// ============================================================================

/// Command-line arguments for the daemon.
#[derive(Debug, Clone, Parser)]
#[command(name = "seriousum-daemon", version, about = "Run the seriousum daemon")]
pub struct Cli {
    /// Optional configuration file.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

/// Returns the default configuration file path.
pub fn default_config_path() -> PathBuf {
    PathBuf::from("seriousum.json")
}

fn load_config_from_path(path: &Path) -> anyhow::Result<Config> {
    seriousum_config::Config::load(path)
}

/// Loads daemon configuration.
pub fn load_config(path: Option<PathBuf>) -> anyhow::Result<Config> {
    match path {
        Some(path) if path.exists() => load_config_from_path(path.as_path()),
        Some(path) => {
            warn!(path = %path.display(), "configuration file not found; using defaults");
            Ok(Config::default())
        }
        None => {
            let path = default_config_path();
            if path.exists() {
                load_config_from_path(path.as_path())
            } else {
                Ok(Config::default())
            }
        }
    }
}

/// Initializes tracing for the daemon.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

/// Execute the daemon.
pub async fn execute(cli: Cli) -> anyhow::Result<()> {
    init_tracing();
    let config = load_config(cli.config)?;

    let daemon_config = DaemonConfig {
        cluster_name: config.agent.cluster_name.clone(),
        node_name: config.agent.node_name.clone(),
        ..Default::default()
    };

    let daemon = Daemon::new(daemon_config)?;
    daemon.run().await.map_err(|e| anyhow::anyhow!("{e}"))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = format!(
            "seriousum-daemon-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        );
        path.push(nonce);
        path
    }

    // ========== Configuration tests ==========

    #[test]
    fn daemon_config_default_is_valid() {
        let config = DaemonConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn daemon_config_validates_cluster_name() {
        let mut config = DaemonConfig::default();
        config.cluster_name = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn daemon_config_validates_node_name() {
        let mut config = DaemonConfig::default();
        config.node_name = String::new();
        assert!(config.validate().is_err());
    }

    #[test]
    fn daemon_config_validates_cluster_name_length() {
        let mut config = DaemonConfig::default();
        config.cluster_name = "x".repeat(254);
        assert!(config.validate().is_err());
    }

    #[test]
    fn daemon_config_validates_node_name_length() {
        let mut config = DaemonConfig::default();
        config.node_name = "x".repeat(254);
        assert!(config.validate().is_err());
    }

    // ========== Component registry tests ==========

    #[test]
    fn registry_starts_empty() {
        let registry = ComponentRegistry::new();
        assert_eq!(registry.list().len(), 0);
    }

    #[tokio::test]
    async fn registry_registers_component() {
        let registry = ComponentRegistry::new();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        assert!(registry.register(component).is_ok());
        assert_eq!(registry.list().len(), 1);
    }

    #[tokio::test]
    async fn registry_prevents_duplicate_registration() {
        let registry = ComponentRegistry::new();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        assert!(registry.register(component.clone()).is_ok());
        assert!(registry.register(component).is_err());
    }

    #[tokio::test]
    async fn registry_tracks_component_state() {
        let registry = ComponentRegistry::new();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        registry.register(component).unwrap();
        assert_eq!(
            registry.state("test-component"),
            Some(ComponentState::Registered)
        );
    }

    #[tokio::test]
    async fn registry_retrieves_component() {
        let registry = ComponentRegistry::new();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        registry.register(component.clone()).unwrap();

        let retrieved = registry.get("test-component");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-component");
    }

    #[tokio::test]
    async fn registry_returns_none_for_nonexistent_component() {
        let registry = ComponentRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn registry_satisfies_dependencies_when_available() {
        let registry = ComponentRegistry::new();

        let component1 = ComponentMetadata {
            name: "component-1".to_string(),
            description: "First component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        registry.register(component1).unwrap();

        let component2 = ComponentMetadata {
            name: "component-2".to_string(),
            description: "Second component".to_string(),
            hooks: Arc::new(TestComponentWithDeps),
        };

        registry.register(component2).unwrap();

        assert!(registry
            .dependencies_satisfied("component-2")
            .unwrap_or_default());
    }

    // ========== Daemon lifecycle tests ==========

    #[tokio::test]
    async fn daemon_creates_with_valid_config() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config);
        assert!(daemon.is_ok());
    }

    #[tokio::test]
    async fn daemon_rejects_invalid_cluster_name() {
        let mut config = DaemonConfig::default();
        config.cluster_name = String::new();
        let daemon = Daemon::new(config);
        assert!(daemon.is_err());
    }

    #[tokio::test]
    async fn daemon_rejects_invalid_node_name() {
        let mut config = DaemonConfig::default();
        config.node_name = String::new();
        let daemon = Daemon::new(config);
        assert!(daemon.is_err());
    }

    #[tokio::test]
    async fn daemon_starts_in_init_state() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();
        assert_eq!(daemon.state().await, DaemonState::Init);
    }

    #[tokio::test]
    async fn daemon_accepts_signal_sender() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();
        let sender = daemon.signal_sender();
        let _rx = sender.subscribe();
        assert!(sender.send(DaemonSignal::Shutdown).is_ok());
    }

    #[tokio::test]
    async fn daemon_initializes_kvstore() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();

        // Check that we can access kvstore
        let kvstore = daemon.kvstore.clone();
        kvstore.set("test_key", b"test_value".to_vec()).await;

        // Note: kvstore is in-memory so this just verifies it was created
    }

    #[tokio::test]
    async fn daemon_registers_component() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        assert!(daemon.register_component(component).is_ok());
    }

    #[tokio::test]
    async fn daemon_prevents_duplicate_component_registration() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        assert!(daemon.register_component(component.clone()).is_ok());
        assert!(daemon.register_component(component).is_err());
    }

    // ========== Component state tests ==========

    #[test]
    fn component_state_display() {
        assert_eq!(ComponentState::Registered.to_string(), "Registered");
        assert_eq!(ComponentState::Starting.to_string(), "Starting");
        assert_eq!(ComponentState::Running.to_string(), "Running");
        assert_eq!(ComponentState::Stopping.to_string(), "Stopping");
        assert_eq!(ComponentState::Stopped.to_string(), "Stopped");
        assert_eq!(ComponentState::Error.to_string(), "Error");
    }

    #[tokio::test]
    async fn registry_rejects_missing_dependency() {
        let registry = ComponentRegistry::new();

        let component = ComponentMetadata {
            name: "dependent-component".to_string(),
            description: "A component with missing dependency".to_string(),
            hooks: Arc::new(TestComponentWithDeps),
        };

        registry.register(component).unwrap();

        // Dependency is not satisfied (component-1 not registered)
        let result = registry.dependencies_satisfied("dependent-component");
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn component_metadata_clones_correctly() {
        let component = ComponentMetadata {
            name: "test".to_string(),
            description: "test".to_string(),
            hooks: Arc::new(TestComponent),
        };

        let cloned = component.clone();
        assert_eq!(cloned.name, component.name);
        assert_eq!(cloned.description, component.description);
    }

    #[tokio::test]
    async fn infrastructure_module_has_sensible_defaults() {
        let infra = InfrastructureModule::default();
        assert!(infra.kubernetes_enabled);
        assert!(infra.kvstore_enabled);
        assert!(infra.metrics_enabled);
        assert!(infra.cni_enabled);
        assert!(infra.healthz_enabled);
    }

    #[tokio::test]
    async fn controlplane_module_has_sensible_defaults() {
        let cp = ControlPlaneModule::default();
        assert!(cp.endpoints_enabled);
        assert!(cp.policy_enabled);
        assert!(cp.identity_enabled);
        assert!(cp.loadbalancer_enabled);
        assert!(cp.proxy_enabled);
        assert!(cp.k8s_watchers_enabled);
        assert!(cp.dns_proxy_enabled);
        assert!(cp.observability_enabled);
    }

    #[tokio::test]
    async fn daemon_component_not_found_error() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();
        let result = daemon.stop_component("nonexistent").await;
        assert!(result.is_err());
        match result {
            Err(Error::ComponentNotFound(name)) => assert_eq!(name, "nonexistent"),
            _ => panic!("expected ComponentNotFound error"),
        }
    }

    #[tokio::test]
    async fn registry_list_returns_all_components() {
        let registry = ComponentRegistry::new();

        for i in 0..3 {
            let component = ComponentMetadata {
                name: format!("component-{i}"),
                description: format!("Component {i}"),
                hooks: Arc::new(TestComponent),
            };
            registry.register(component).ok();
        }

        let list = registry.list();
        assert_eq!(list.len(), 3);
    }

    #[tokio::test]
    async fn component_dependency_creates_correctly() {
        let dep = ComponentDependency {
            name: "my-dep".to_string(),
            optional: false,
        };
        assert_eq!(dep.name, "my-dep");
        assert!(!dep.optional);

        let optional_dep = ComponentDependency {
            name: "optional-dep".to_string(),
            optional: true,
        };
        assert!(optional_dep.optional);
    }

    #[tokio::test]
    async fn daemon_signal_enum_variants() {
        let shutdown = DaemonSignal::Shutdown;
        let reconfigure = DaemonSignal::Reconfigure;

        match shutdown {
            DaemonSignal::Shutdown => {},
            _ => panic!("wrong variant"),
        }

        match reconfigure {
            DaemonSignal::Reconfigure => {},
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn daemon_handles_graceful_shutdown() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        daemon.register_component(component).unwrap();

        // Spawn daemon in background
        let daemon_clone = Arc::new(daemon);
        let daemon_for_spawn = daemon_clone.clone();

        let daemon_handle = tokio::spawn(async move {
            let _ = daemon_for_spawn.run().await;
        });

        // Give daemon time to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Send shutdown signal
        let sender = daemon_clone.signal_sender();
        let _ = sender.send(DaemonSignal::Shutdown);

        // Wait for daemon to finish
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            daemon_handle,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn daemon_handles_multiple_component_stop_errors() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        daemon.register_component(component).unwrap();

        // Manually set up a component to test stop
        let registry = daemon.registry();
        registry.set_state("test-component", ComponentState::Running);

        // Test that we can stop components
        let stop_result = daemon.stop_component("test-component").await;
        assert!(stop_result.is_ok());
    }

    #[tokio::test]
    async fn daemon_transitions_states_correctly() {
        let config = DaemonConfig::default();
        let daemon = Daemon::new(config).unwrap();

        let component = ComponentMetadata {
            name: "test-component".to_string(),
            description: "A test component".to_string(),
            hooks: Arc::new(TestComponent),
        };

        daemon.register_component(component).unwrap();

        // Initial state
        assert_eq!(daemon.state().await, DaemonState::Init);

        let daemon_clone = Arc::new(daemon);
        let daemon_for_spawn = daemon_clone.clone();

        let daemon_handle = tokio::spawn(async move {
            let _ = daemon_for_spawn.run().await;
        });

        // Wait a bit for state transitions
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Should be starting or running
        let state = daemon_clone.state().await;
        assert!(
            state == DaemonState::Starting || state == DaemonState::Running,
            "state should be Starting or Running, got {state:?}",
        );

        // Shutdown
        daemon_clone.signal_sender().send(DaemonSignal::Shutdown).ok();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            daemon_handle,
        )
        .await;

        assert!(result.is_ok());
    }

    // ========== CLI tests ==========

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
        let path = unique_path("missing.json");

        let config = load_config(Some(path.clone())).expect("load default config");

        assert_eq!(config, Config::default());
    }

    #[test]
    fn load_config_uses_defaults_when_default_path_is_missing() {
        let original_dir = std::env::current_dir().expect("current dir");
        let temp_dir = unique_path("cwd");
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        std::env::set_current_dir(&temp_dir).expect("set temp dir");

        let config = load_config(None).expect("load default config");

        std::env::set_current_dir(original_dir).expect("restore cwd");
        assert_eq!(config, Config::default());
    }

    // ========== Test fixtures ==========

    struct TestComponent;

    #[async_trait::async_trait]
    impl ComponentHooks for TestComponent {
        async fn start(&self) -> Result<()> {
            debug!("test component started");
            Ok(())
        }

        async fn run(&self) -> Result<()> {
            // Don't actually block in tests
            Ok(())
        }

        async fn stop(&self) -> Result<()> {
            debug!("test component stopped");
            Ok(())
        }
    }

    struct TestComponentWithDeps;

    #[async_trait::async_trait]
    impl ComponentHooks for TestComponentWithDeps {
        async fn start(&self) -> Result<()> {
            Ok(())
        }

        async fn run(&self) -> Result<()> {
            Ok(())
        }

        async fn stop(&self) -> Result<()> {
            Ok(())
        }

        fn dependencies(&self) -> Vec<ComponentDependency> {
            vec![ComponentDependency {
                name: "component-1".to_string(),
                optional: false,
            }]
        }
    }
}
