use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueHint};
use seriousum_api::{CONTRACT_VERSION, CORE_VERSION};
use seriousum_config::Config;
use seriousum_operator::Operator;

/// Minimal CLI scaffold for the seriousum control plane.
#[derive(Debug, Parser)]
#[command(name = "seriousum-cli", about = "seriousum control-plane scaffold")]
pub struct Cli {
    /// Selected command.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level CLI commands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Print version metadata.
    Version,

    /// Inspect and validate configuration files.
    Config {
        /// Config-related commands.
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Inspect operator health/reporting scaffolds.
    Operator {
        /// Operator-related commands.
        #[command(subcommand)]
        command: OperatorCommand,
    },
}

/// Config subcommands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum ConfigCommand {
    /// Load a config file and report success if it parses.
    Check {
        /// Path to the config file.
        #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
        path: PathBuf,
    },
}

/// Operator subcommands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum OperatorCommand {
    /// Print a synthesized operator report.
    Report,
}

/// CLI result alias.
pub type Result<T> = std::result::Result<T, seriousum_core::Error>;

/// Runs the CLI from the process arguments.
pub fn run() -> Result<String> {
    run_from(std::env::args_os())
}

/// Runs the CLI from an arbitrary iterator of arguments.
pub fn run_from<I, T>(args: I) -> Result<String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)
        .map_err(|error| seriousum_core::Error::Config(error.to_string()))?;
    execute(cli.command)
}

/// Executes a parsed command.
pub fn execute(command: Command) -> Result<String> {
    match command {
        Command::Version => Ok(version_output()),
        Command::Config { command } => execute_config(command),
        Command::Operator { command } => execute_operator(command),
    }
}

fn execute_config(command: ConfigCommand) -> Result<String> {
    match command {
        ConfigCommand::Check { path } => {
            let config = Config::load(path.as_path())?;
            Ok(format!(
                "config ok: {} ({})",
                path.display(),
                config_summary(&config)
            ))
        }
    }
}

fn execute_operator(command: OperatorCommand) -> Result<String> {
    match command {
        OperatorCommand::Report => {
            let report = Operator::scaffold().report();
            Ok(serde_json::to_string_pretty(&report)
                .map_err(|error| seriousum_core::Error::Config(error.to_string()))?)
        }
    }
}

fn version_output() -> String {
    format!(
        "seriousum-cli {}\ncontract {}\ncore {}",
        env!("CARGO_PKG_VERSION"),
        CONTRACT_VERSION,
        CORE_VERSION,
    )
}

fn config_summary(config: &Config) -> String {
    format!(
        "agent={}, node={}, cluster={}, mtu={}, ipv4={}, ipv6={}",
        config.agent.name,
        config.agent.node_name,
        config.agent.cluster_name,
        config.network.mtu,
        config.network.enable_ipv4,
        config.network.enable_ipv6,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = format!(
            "seriousum-cli-{name}-{}-{}",
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
    fn parses_version_command() {
        let cli = Cli::try_parse_from(["seriousum-cli", "version"]).expect("parse version command");
        assert_eq!(cli.command, Command::Version);
    }

    #[test]
    fn parses_config_check_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "config",
            "check",
            "--path",
            "/tmp/config.json",
        ])
        .expect("parse config check command");

        assert_eq!(
            cli.command,
            Command::Config {
                command: ConfigCommand::Check {
                    path: PathBuf::from("/tmp/config.json")
                },
            }
        );
    }

    #[test]
    fn version_execution_includes_contract_and_core_versions() {
        let output = execute(Command::Version).expect("execute version command");

        assert!(output.contains(&format!("seriousum-cli {}", env!("CARGO_PKG_VERSION"))));
        assert!(output.contains(&format!("contract {}", CONTRACT_VERSION)));
        assert!(output.contains(&format!("core {}", CORE_VERSION)));
    }

    #[test]
    fn config_check_reads_valid_config() {
        let path = temp_config_path("config.json");
        std::fs::write(
            &path,
            r#"{
                "agent": { "name": "seriousum-agent" },
                "network": { "mtu": 9000 }
            }"#,
        )
        .expect("write temp config");

        let output = execute(Command::Config {
            command: ConfigCommand::Check { path: path.clone() },
        })
        .expect("execute config check");

        assert!(output.contains("config ok:"));
        assert!(output.contains(&path.display().to_string()));
        assert!(output.contains("seriousum-agent"));
        assert!(output.contains("mtu=9000"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn operator_report_is_synthesized_from_shared_types() {
        let output = execute(Command::Operator {
            command: OperatorCommand::Report,
        })
        .expect("execute operator report");

        assert!(output.contains("operator scaffold ready"));
        assert!(output.contains(r#""status": "healthy""#));
        assert!(output.contains(r#""contract": "0.1.0""#));
    }
}
