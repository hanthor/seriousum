//! Daemon entrypoint and lifecycle wiring.

use std::path::{Path, PathBuf};

use clap::Parser;
use tracing::{info, warn};

use seriousum_config::Config;
use seriousum_kvstore::KvStore;

/// Command-line arguments for the daemon.
#[derive(Debug, Clone, Parser)]
#[command(name = "seriousum-daemon", version, about = "Run the seriousum daemon")]
pub struct Cli {
    /// Optional configuration file.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

/// The daemon runtime.
#[derive(Debug, Clone)]
pub struct Daemon {
    config: Config,
    store: KvStore,
}

impl Daemon {
    /// Create a new daemon.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            store: KvStore::new(),
        }
    }

    /// Run the daemon.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!(cluster = %self.config.agent.cluster_name, node = %self.config.agent.node_name, "starting seriousum daemon");
        self.store.set("daemon/state", b"running".to_vec()).await;
        self.store
            .set(
                "daemon/cluster",
                self.config.agent.cluster_name.as_bytes().to_vec(),
            )
            .await;
        self.store
            .set(
                "daemon/node",
                self.config.agent.node_name.as_bytes().to_vec(),
            )
            .await;
        warn!("daemon runtime is a scaffold; control-plane wiring will be added incrementally");
        Ok(())
    }
}

/// Returns the default configuration file path.
pub fn default_config_path() -> PathBuf {
    PathBuf::from("seriousum.json")
}

fn load_config_from_path(path: &Path) -> anyhow::Result<Config> {
    seriousum_config::Config::load(path)
}

/// Initializes tracing for the daemon.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
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

/// Execute the daemon.
pub async fn execute(cli: Cli) -> anyhow::Result<()> {
    init_tracing();
    let config = load_config(cli.config)?;
    let daemon = Daemon::new(config);
    daemon.run().await
}

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

    #[test]
    fn cli_parses() {
        let cli = Cli::parse_from(["seriousum-daemon"]);
        assert!(cli.config.is_none());
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
}
