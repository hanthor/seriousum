use std::fmt::Write as FmtWrite;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use serde::Serialize;
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

    /// Report Cilium feature status.
    Features {
        /// Feature-related commands.
        #[command(subcommand)]
        command: FeaturesCommand,
    },

    /// Create a deterministic sysdump artifact summary.
    Sysdump {
        /// Optional output filename for the sysdump artifact.
        #[arg(long = "output-filename", value_name = "FILE", value_hint = ValueHint::FilePath)]
        output_filename: Option<PathBuf>,
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
#[derive(Debug, Subcommand, PartialEq, Eq, Copy, Clone)]
pub enum OperatorCommand {
    /// Print a synthesized operator report.
    Report,
}

/// Feature subcommands.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum FeaturesCommand {
    /// Print a synthesized feature status report.
    Status {
        /// Render the report in a specific format.
        #[arg(short = 'o', long = "output", default_value = "markdown")]
        output: OutputFormat,

        /// Optional file to write the rendered output to.
        #[arg(long = "output-file", value_name = "FILE", value_hint = ValueHint::FilePath)]
        output_file: Option<PathBuf>,
    },
}

/// Supported feature status output formats.
#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// JSON output.
    Json,

    /// Markdown output.
    Markdown,
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
        Command::Features { command } => execute_features(command),
        Command::Sysdump { output_filename } => execute_sysdump(output_filename),
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

fn execute_features(command: FeaturesCommand) -> Result<String> {
    match command {
        FeaturesCommand::Status {
            output,
            output_file,
        } => {
            let report = features_status_report();
            let rendered = match output {
                OutputFormat::Json => serde_json::to_string_pretty(&report)
                    .map_err(|error| seriousum_core::Error::Config(error.to_string()))?,
                OutputFormat::Markdown => features_status_markdown(&report),
            };

            if let Some(path) = output_file {
                write_text_file(&path, &rendered)?;
            }

            Ok(rendered)
        }
    }
}

fn execute_sysdump(output_filename: Option<PathBuf>) -> Result<String> {
    let report = sysdump_report(output_filename.clone());
    let rendered = serde_json::to_string_pretty(&report)
        .map_err(|error| seriousum_core::Error::Config(error.to_string()))?;

    if let Some(path) = output_filename {
        write_text_file(&path, &rendered)?;
    }

    Ok(rendered)
}

fn write_text_file(path: &PathBuf, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(path, contents)?;
    Ok(())
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FeatureState {
    name: &'static str,
    enabled: bool,
    description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct FeaturesStatusReport {
    command: &'static str,
    status: &'static str,
    features: Vec<FeatureState>,
}

fn features_status_report() -> FeaturesStatusReport {
    FeaturesStatusReport {
        command: "features status",
        status: "ok",
        features: vec![
            FeatureState {
                name: "bpf",
                enabled: true,
                description: "eBPF datapath scaffold is available",
            },
            FeatureState {
                name: "hubble",
                enabled: true,
                description: "observability scaffold is available",
            },
            FeatureState {
                name: "cluster-mesh",
                enabled: false,
                description: "cluster mesh is not enabled in the scaffold",
            },
        ],
    }
}

fn features_status_markdown(report: &FeaturesStatusReport) -> String {
    let mut rendered = String::from("# features status\n\n");
    let _ = writeln!(rendered, "- command: {}", report.command);
    let _ = writeln!(rendered, "- status: {}\n", report.status);
    rendered.push_str("| feature | enabled | description |\n");
    rendered.push_str("| --- | --- | --- |\n");

    for feature in &report.features {
        let _ = writeln!(
            rendered,
            "| {} | {} | {} |",
            feature.name,
            if feature.enabled { "yes" } else { "no" },
            feature.description,
        );
    }

    rendered
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SysdumpReport {
    command: &'static str,
    status: &'static str,
    output_filename: Option<PathBuf>,
    artifacts: Vec<&'static str>,
}

fn sysdump_report(output_filename: Option<PathBuf>) -> SysdumpReport {
    SysdumpReport {
        command: "sysdump",
        status: "prepared",
        output_filename,
        artifacts: vec![
            "cluster-info.txt",
            "resources.txt",
            "logs.txt",
            "version.txt",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
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
    fn parses_features_status_command() {
        let cli = Cli::try_parse_from([
            "cilium-cli",
            "features",
            "status",
            "-o",
            "json",
            "--output-file",
            "/tmp/features.json",
        ])
        .expect("parse features status command");

        assert_eq!(
            cli.command,
            Command::Features {
                command: FeaturesCommand::Status {
                    output: OutputFormat::Json,
                    output_file: Some(PathBuf::from("/tmp/features.json")),
                },
            }
        );
    }

    #[test]
    fn parses_sysdump_command() {
        let cli = Cli::try_parse_from([
            "cilium-cli",
            "sysdump",
            "--output-filename",
            "/tmp/sysdump.json",
        ])
        .expect("parse sysdump command");

        assert_eq!(
            cli.command,
            Command::Sysdump {
                output_filename: Some(PathBuf::from("/tmp/sysdump.json")),
            }
        );
    }

    #[test]
    fn version_execution_includes_contract_and_core_versions() {
        let output = execute(Command::Version).expect("execute version command");

        assert!(output.contains(&format!("seriousum-cli {}", env!("CARGO_PKG_VERSION"))));
        assert!(output.contains(&format!("contract {CONTRACT_VERSION}")));
        assert!(output.contains(&format!("core {CORE_VERSION}")));
    }

    #[test]
    fn config_check_reads_valid_config() {
        let path = temp_path("config.json");
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

    #[test]
    fn features_status_json_is_stable() {
        let output = execute(Command::Features {
            command: FeaturesCommand::Status {
                output: OutputFormat::Json,
                output_file: None,
            },
        })
        .expect("execute features status json");

        let value: serde_json::Value = serde_json::from_str(&output).expect("json output parses");
        assert_eq!(value["command"], "features status");
        assert_eq!(value["status"], "ok");
        assert_eq!(value["features"][0]["name"], "bpf");
        assert_eq!(value["features"][1]["enabled"], true);
    }

    #[test]
    fn features_status_markdown_writes_requested_output_file() {
        let path = temp_path("features.md");
        let output = execute(Command::Features {
            command: FeaturesCommand::Status {
                output: OutputFormat::Markdown,
                output_file: Some(path.clone()),
            },
        })
        .expect("execute features status markdown");

        let written = std::fs::read_to_string(&path).expect("read markdown file");
        assert_eq!(output, written);
        assert!(written.contains("# features status"));
        assert!(written.contains("| feature | enabled | description |"));
        assert!(written.contains("| hubble | yes | observability scaffold is available |"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sysdump_creates_deterministic_artifact_file() {
        let path = temp_path("sysdump.json");
        let output = execute(Command::Sysdump {
            output_filename: Some(path.clone()),
        })
        .expect("execute sysdump");

        let written = std::fs::read_to_string(&path).expect("read sysdump file");
        assert_eq!(output, written);
        let value: serde_json::Value = serde_json::from_str(&written).expect("sysdump json parses");
        assert_eq!(value["command"], "sysdump");
        assert_eq!(value["status"], "prepared");
        assert_eq!(value["artifacts"][0], "cluster-info.txt");

        let _ = std::fs::remove_file(&path);
    }
}
