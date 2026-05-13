use std::fmt::Write as FmtWrite;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use serde::{Deserialize, Serialize};
use seriousum_config::RuntimeConfig;
use seriousum_core::VERSION as CORE_VERSION;
use thiserror::Error;

pub mod connectivity;
pub mod endpoint;
pub mod flow;
pub mod policy;
pub mod status;

/// Contract version
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// =============================================================================
// Error handling
// =============================================================================

/// CLI errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("connectivity test failed: {0}")]
    ConnectivityTestFailed(String),

    #[error("status collection error: {0}")]
    StatusCollectionError(String),

    #[error("endpoint error: {0}")]
    EndpointError(String),

    #[error("policy validation error: {0}")]
    PolicyValidationError(String),

    #[error("flow verification error: {0}")]
    FlowVerificationError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// CLI result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors for pure CLI command/output helpers.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// The requested output format is not recognized.
    #[error("invalid output format: {0}")]
    InvalidOutputFormat(String),
    /// The agent API returned an error.
    #[error("API error: {0}")]
    ApiError(String),
    /// The requested endpoint could not be found.
    #[error("endpoint not found: {0}")]
    EndpointNotFound(String),
    /// JSON serialization or deserialization failed.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

// =============================================================================
// CLI command structure
// =============================================================================

/// Minimal CLI scaffold for the seriousum control plane.
#[derive(Debug, Parser)]
#[command(
    name = "seriousum-cli",
    about = "seriousum control-plane with Track U extensions"
)]
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

    /// Run connectivity tests (Track U).
    Connectivity {
        /// Connectivity-related commands.
        #[command(subcommand)]
        command: ConnectivityCommand,
    },

    /// Check service and cluster status (Track U).
    Status {
        /// Status-related commands.
        #[command(subcommand)]
        command: StatusCommand,
    },

    /// Validate policy configuration (Track U).
    Policy {
        /// Policy-related commands.
        #[command(subcommand)]
        command: PolicyCommand,
    },

    /// Analyze network flows (Track U).
    Flow {
        /// Flow-related commands.
        #[command(subcommand)]
        command: FlowCommand,
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

/// Connectivity test subcommands (Track U).
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum ConnectivityCommand {
    /// Run all connectivity tests.
    Run {
        /// Test name filter.
        #[arg(long)]
        test_filter: Option<String>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,

        /// Write output to file.
        #[arg(long = "output-file", value_name = "FILE", value_hint = ValueHint::FilePath)]
        output_file: Option<PathBuf>,
    },

    /// Check connectivity between two endpoints.
    Check {
        /// Source endpoint name.
        #[arg(long)]
        source: String,

        /// Destination endpoint name.
        #[arg(long)]
        destination: String,

        /// Protocol (tcp, udp, icmp).
        #[arg(long, default_value = "tcp")]
        protocol: String,

        /// Destination port.
        #[arg(long, default_value = "80")]
        port: u16,
    },

    /// List available tests.
    ListTests,
}

/// Status check subcommands (Track U).
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum StatusCommand {
    /// Get overall cluster status.
    Cluster {
        /// Wait for ready status.
        #[arg(long)]
        wait: bool,

        /// Maximum wait duration.
        #[arg(long, default_value = "5m")]
        wait_duration: String,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },

    /// Check endpoint status.
    Endpoints {
        /// Filter by namespace.
        #[arg(long)]
        namespace: Option<String>,

        /// Filter by pod name.
        #[arg(long)]
        pod_name: Option<String>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },

    /// Check service status.
    Services {
        /// Filter by namespace.
        #[arg(long)]
        namespace: Option<String>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },
}

/// Policy validation subcommands (Track U).
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum PolicyCommand {
    /// Validate policy configuration.
    Validate {
        /// Path to policy file.
        #[arg(long, value_name = "FILE", value_hint = ValueHint::FilePath)]
        policy_file: Option<PathBuf>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },

    /// Check if traffic is allowed by policy.
    Check {
        /// Source pod name.
        #[arg(long)]
        source_pod: String,

        /// Destination pod name.
        #[arg(long)]
        dest_pod: String,

        /// Protocol.
        #[arg(long, default_value = "tcp")]
        protocol: String,

        /// Port.
        #[arg(long, default_value = "80")]
        port: u16,
    },

    /// List active policies.
    List {
        /// Filter by namespace.
        #[arg(long)]
        namespace: Option<String>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },
}

/// Flow analysis subcommands (Track U).
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum FlowCommand {
    /// Analyze recent flows.
    Recent {
        /// Number of flows to show.
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Filter by source pod.
        #[arg(long)]
        source_pod: Option<String>,

        /// Filter by destination pod.
        #[arg(long)]
        dest_pod: Option<String>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },

    /// Get flow statistics.
    Stats {
        /// Filter by namespace.
        #[arg(long)]
        namespace: Option<String>,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },

    /// Filter flows by criteria.
    Filter {
        /// Expression for filtering.
        #[arg(long)]
        expression: String,

        /// Output format.
        #[arg(short = 'o', long = "output", default_value = "summary")]
        output: OutputFormat,
    },
}

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Columnar table output.
    #[default]
    Table,
    /// Compact JSON output.
    Json,
    /// Pretty-printed JSON output.
    #[serde(rename = "json-pretty")]
    #[value(name = "json-pretty", alias = "jsonpretty")]
    JsonPretty,
    /// YAML output.
    Yaml,
    /// Markdown output.
    Markdown,
    /// Summary output (human-readable).
    Summary,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table => write!(f, "table"),
            Self::Json => write!(f, "json"),
            Self::JsonPretty => write!(f, "json-pretty"),
            Self::Yaml => write!(f, "yaml"),
            Self::Markdown => write!(f, "markdown"),
            Self::Summary => write!(f, "summary"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = CliError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "json-pretty" | "jsonpretty" => Ok(Self::JsonPretty),
            "yaml" => Ok(Self::Yaml),
            "markdown" => Ok(Self::Markdown),
            "summary" => Ok(Self::Summary),
            _ => Err(CliError::InvalidOutputFormat(s.into())),
        }
    }
}

/// Global CLI options shared across all commands.
#[derive(Debug, Clone)]
pub struct GlobalOptions {
    /// cilium-agent API host, for example `localhost:9900`.
    pub host: String,
    /// Requested output format.
    pub output: OutputFormat,
    /// Enables verbose logging.
    pub verbose: bool,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            host: "localhost:9900".into(),
            output: OutputFormat::Table,
            verbose: false,
            timeout_secs: 30,
        }
    }
}

/// Row in `cilium endpoint list` table output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointRow {
    /// Endpoint identifier.
    pub id: u16,
    /// Policy enablement summary such as `ingress+egress`.
    pub policy_enabled: String,
    /// Endpoint IPv4 address.
    pub ipv4: String,
    /// Endpoint IPv6 address.
    pub ipv6: String,
    /// Security identity assigned to the endpoint.
    pub identity: u32,
    /// Effective endpoint labels.
    pub labels: Vec<String>,
    /// Current endpoint state.
    pub state: String,
}

/// Output of `cilium endpoint get <id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDetails {
    /// Endpoint identifier.
    pub id: u16,
    /// Current endpoint state.
    pub state: String,
    /// Security identity assigned to the endpoint.
    pub identity: u32,
    /// Endpoint IPv4 address if present.
    pub ipv4: Option<String>,
    /// Endpoint IPv6 address if present.
    pub ipv6: Option<String>,
    /// Effective endpoint labels.
    pub labels: Vec<String>,
    /// Summary of policy map decisions.
    pub policy_map_state: PolicyMapSummary,
}

/// Aggregated counters for an endpoint policy map.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyMapSummary {
    /// Number of allowed ingress entries.
    pub ingress_allowed: u32,
    /// Number of allowed egress entries.
    pub egress_allowed: u32,
    /// Number of denied ingress entries.
    pub ingress_denied: u32,
    /// Number of denied egress entries.
    pub egress_denied: u32,
}

/// Output of `cilium policy get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyOutput {
    /// Policy repository revision.
    pub revision: i64,
    /// JSON-encoded policy payload.
    pub policy: String,
}

/// Row in `cilium policy selectors` output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorRow {
    /// Selector expression.
    pub selector: String,
    /// Number of identities selected.
    pub identity_count: u32,
    /// Number of selector users.
    pub users: u32,
}

/// Row in `cilium identity list` output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRow {
    /// Numeric identity.
    pub id: u32,
    /// Labels attached to the identity.
    pub labels: Vec<String>,
}

/// Row in `cilium service list` output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRow {
    /// Service identifier.
    pub id: u16,
    /// Frontend address such as `10.0.0.1:80/TCP`.
    pub frontend: String,
    /// Number of configured backends.
    pub backend_count: u32,
    /// Service type string.
    pub service_type: String,
}

/// Single backend in `cilium service get <id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceBackendRow {
    /// Backend identifier.
    pub id: u16,
    /// Backend address.
    pub addr: String,
    /// Backend state.
    pub state: String,
    /// Backend weight.
    pub weight: u16,
}

/// Simple columnar table formatter.
pub struct TableFormatter {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl TableFormatter {
    /// Creates a formatter with the provided headers.
    pub fn new(headers: Vec<impl Into<String>>) -> Self {
        Self {
            headers: headers.into_iter().map(Into::into).collect(),
            rows: vec![],
        }
    }

    /// Adds a row to the table.
    pub fn add_row(&mut self, row: Vec<impl Into<String>>) {
        self.rows.push(row.into_iter().map(Into::into).collect());
    }

    /// Renders the table as an aligned string.
    pub fn render(&self) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        let mut widths: Vec<usize> = self.headers.iter().map(std::string::String::len).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        let mut out = String::new();
        for (i, header) in self.headers.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            let _ = write!(out, "{header:<width$}", width = widths[i]);
        }
        out.push('\n');

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    out.push_str("  ");
                }
                let width = widths.get(i).copied().unwrap_or(0);
                let _ = write!(out, "{cell:<width$}");
            }
            out.push('\n');
        }

        out
    }

    /// Returns the number of rows added to the formatter.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

// =============================================================================
// Execution functions
// =============================================================================

/// Runs the CLI from process arguments.
pub fn run() -> Result<String> {
    run_from(std::env::args_os())
}

/// Runs the CLI from an arbitrary iterator of arguments.
pub fn run_from<I, T>(args: I) -> Result<String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args).map_err(|error| Error::Config(error.to_string()))?;
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
        Command::Connectivity { command } => execute_connectivity(command),
        Command::Status { command } => execute_status(command),
        Command::Policy { command } => execute_policy(command),
        Command::Flow { command } => execute_flow(command),
    }
}

fn execute_config(command: ConfigCommand) -> Result<String> {
    match command {
        ConfigCommand::Check { path } => {
            let config = RuntimeConfig::load(path.as_path())
                .map_err(|e| Error::Config(format!("failed to load config: {e}")))?;
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
            let report = serde_json::json!({
                "status": "healthy",
                "operator": "scaffold ready",
                "contract": "0.1.0"
            });
            serde_json::to_string_pretty(&report)
                .map_err(|error| Error::Config(format!("json serialization error: {error}")))
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
                OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
                    serialize_output(&report, output)?
                }
                OutputFormat::Markdown => features_status_markdown(&report),
                OutputFormat::Table | OutputFormat::Summary => features_status_summary(&report),
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
        .map_err(|error| Error::Config(format!("json error: {error}")))?;

    if let Some(path) = output_filename {
        write_text_file(&path, &rendered)?;
    }

    Ok(rendered)
}

fn execute_connectivity(command: ConnectivityCommand) -> Result<String> {
    match command {
        ConnectivityCommand::Run {
            test_filter,
            output,
            output_file,
        } => {
            let test_suite = connectivity::ConnectivityTestSuite::new();
            let results = test_suite.run_tests(test_filter.as_deref())?;
            let rendered = format_connectivity_results(&results, output)?;

            if let Some(path) = output_file {
                write_text_file(&path, &rendered)?;
            }

            Ok(rendered)
        }

        ConnectivityCommand::Check {
            source,
            destination,
            protocol,
            port,
        } => {
            let tester = connectivity::ConnectivityTester::new();
            let result = tester.check_connectivity(&source, &destination, &protocol, port)?;

            let summary = format!(
                "Connectivity check: {} -> {} ({}:{})\nResult: {}\nLatency: {}ms",
                source,
                destination,
                protocol,
                port,
                if result.is_connected {
                    "✓ Connected"
                } else {
                    "✗ Disconnected"
                },
                result.latency_ms
            );

            Ok(summary)
        }

        ConnectivityCommand::ListTests => {
            let test_suite = connectivity::ConnectivityTestSuite::new();
            let tests = test_suite.list_available_tests();
            let formatted = tests
                .iter()
                .map(|t| format!("  - {}: {}", t.name, t.description))
                .collect::<Vec<_>>()
                .join("\n");

            Ok(format!("Available connectivity tests:\n{formatted}"))
        }
    }
}

fn execute_status(command: StatusCommand) -> Result<String> {
    match command {
        StatusCommand::Cluster {
            wait: _,
            wait_duration: _,
            output,
        } => {
            let collector = status::StatusCollector::new();
            let cluster_status = collector.collect_cluster_status()?;

            let rendered = format_status_result(&cluster_status, output)?;
            Ok(rendered)
        }

        StatusCommand::Endpoints {
            namespace,
            pod_name,
            output,
        } => {
            let collector = status::StatusCollector::new();
            let endpoints = collector.collect_endpoint_status(namespace, pod_name)?;

            let rendered = format_endpoint_results(&endpoints, output)?;
            Ok(rendered)
        }

        StatusCommand::Services { namespace, output } => {
            let collector = status::StatusCollector::new();
            let services = collector.collect_service_status(namespace)?;

            let rendered = format_service_results(&services, output)?;
            Ok(rendered)
        }
    }
}

fn execute_policy(command: PolicyCommand) -> Result<String> {
    match command {
        PolicyCommand::Validate {
            policy_file,
            output,
        } => {
            let validator = policy::PolicyValidator::new();

            let validation_result = if let Some(path) = policy_file {
                validator.validate_policy_file(&path)?
            } else {
                validator.validate_default_policies()?
            };

            let rendered = format_policy_result(&validation_result, output)?;
            Ok(rendered)
        }

        PolicyCommand::Check {
            source_pod,
            dest_pod,
            protocol,
            port,
        } => {
            let checker = policy::PolicyChecker::new();
            let allowed = checker.check_traffic_allowed(&source_pod, &dest_pod, &protocol, port)?;

            let summary = format!(
                "Policy check: {} -> {} ({}:{})\nAllowed: {}",
                source_pod,
                dest_pod,
                protocol,
                port,
                if allowed { "✓ Yes" } else { "✗ No" }
            );

            Ok(summary)
        }

        PolicyCommand::List { namespace, output } => {
            let lister = policy::PolicyLister::new();
            let policies = lister.list_policies(namespace)?;

            let rendered = format_policy_list(&policies, output)?;
            Ok(rendered)
        }
    }
}

fn execute_flow(command: FlowCommand) -> Result<String> {
    match command {
        FlowCommand::Recent {
            limit,
            source_pod,
            dest_pod,
            output,
        } => {
            let analyzer = flow::FlowAnalyzer::new();
            let flows =
                analyzer.get_recent_flows(limit, source_pod.as_deref(), dest_pod.as_deref())?;

            let rendered = format_flow_results(&flows, output)?;
            Ok(rendered)
        }

        FlowCommand::Stats { namespace, output } => {
            let analyzer = flow::FlowAnalyzer::new();
            let stats = analyzer.get_flow_statistics(namespace.as_deref())?;

            let rendered = format_flow_stats(&stats, output)?;
            Ok(rendered)
        }

        FlowCommand::Filter { expression, output } => {
            let analyzer = flow::FlowAnalyzer::new();
            let flows = analyzer.filter_flows(&expression)?;

            let rendered = format_flow_results(&flows, output)?;
            Ok(rendered)
        }
    }
}

// =============================================================================
// Formatting helpers
// =============================================================================

fn serialize_output<T: Serialize + ?Sized>(value: &T, output: OutputFormat) -> Result<String> {
    match output {
        OutputFormat::Json => serde_json::to_string(value)
            .map_err(|error| Error::Config(format!("json error: {error}"))),
        OutputFormat::JsonPretty => serde_json::to_string_pretty(value)
            .map_err(|error| Error::Config(format!("json error: {error}"))),
        OutputFormat::Yaml => {
            let json = serde_json::to_value(value)
                .map_err(|error| Error::Config(format!("json error: {error}")))?;
            Ok(render_yaml_value_to_string(&json, 0))
        }
        _ => Err(Error::Config(format!(
            "unsupported serialized output format: {output}"
        ))),
    }
}

fn render_yaml_value_to_string(value: &serde_json::Value, indent: usize) -> String {
    let mut out = String::new();
    render_yaml_value(&mut out, value, indent);
    out
}

fn render_yaml_value(out: &mut String, value: &serde_json::Value, indent: usize) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        serde_json::Value::Number(value) => out.push_str(&value.to_string()),
        serde_json::Value::String(value) => {
            if let Ok(quoted) = serde_json::to_string(value) {
                out.push_str(&quoted);
            } else {
                out.push('"');
                out.push_str(value);
                out.push('"');
            }
        }
        serde_json::Value::Array(values) => {
            if values.is_empty() {
                out.push_str("[]");
                return;
            }

            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push('\n');
                }
                out.push_str(&" ".repeat(indent));
                out.push_str("- ");
                if yaml_is_scalar(value) {
                    render_yaml_value(out, value, indent + 2);
                } else {
                    out.push('\n');
                    render_yaml_value(out, value, indent + 2);
                }
            }
        }
        serde_json::Value::Object(values) => {
            if values.is_empty() {
                out.push_str("{}");
                return;
            }

            for (index, (key, value)) in values.iter().enumerate() {
                if index > 0 {
                    out.push('\n');
                }
                out.push_str(&" ".repeat(indent));
                out.push_str(key);
                out.push(':');
                if yaml_is_scalar(value) {
                    out.push(' ');
                    render_yaml_value(out, value, indent + 2);
                } else {
                    out.push('\n');
                    render_yaml_value(out, value, indent + 2);
                }
            }
        }
    }
}

fn yaml_is_scalar(value: &serde_json::Value) -> bool {
    matches!(
        value,
        serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_)
    )
}

fn format_connectivity_results(
    results: &[connectivity::ConnectivityTestResult],
    output: OutputFormat,
) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(results, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = String::from("Connectivity Test Results\n");
            s.push_str("===========================\n\n");
            for result in results {
                let _ = writeln!(
                    s,
                    "{}: {}",
                    result.test_name,
                    if result.passed {
                        "✓ PASS"
                    } else {
                        "✗ FAIL"
                    }
                );
            }
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Connectivity Test Results\n\n");
            s.push_str("| Test | Status |\n");
            s.push_str("| --- | --- |\n");
            for result in results {
                let _ = writeln!(
                    s,
                    "| {} | {} |",
                    result.test_name,
                    if result.passed {
                        "✓ PASS"
                    } else {
                        "✗ FAIL"
                    }
                );
            }
            Ok(s)
        }
    }
}

fn format_status_result(status: &status::ClusterStatus, output: OutputFormat) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(status, output)
        }
        OutputFormat::Table | OutputFormat::Summary => Ok(format!(
            "Cluster Status\n==============\nNodes: {}\nEndpoints: {}\nHealthy: {}\n",
            status.node_count, status.endpoint_count, status.is_healthy
        )),
        OutputFormat::Markdown => Ok(format!(
            "# Cluster Status\n\n- **Nodes**: {}\n- **Endpoints**: {}\n- **Status**: {}\n",
            status.node_count,
            status.endpoint_count,
            if status.is_healthy {
                "✓ Healthy"
            } else {
                "✗ Unhealthy"
            }
        )),
    }
}

fn format_endpoint_results(
    endpoints: &[endpoint::EndpointStatus],
    output: OutputFormat,
) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(endpoints, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = format!("Endpoints ({})\n", endpoints.len());
            s.push_str("================\n");
            for ep in endpoints {
                let _ = writeln!(s, "{}: {} - {}", ep.name, ep.pod_name, ep.status);
            }
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Endpoints\n\n");
            s.push_str("| Name | Pod | Status |\n");
            s.push_str("| --- | --- | --- |\n");
            for ep in endpoints {
                let _ = writeln!(s, "| {} | {} | {} |", ep.name, ep.pod_name, ep.status);
            }
            Ok(s)
        }
    }
}

fn format_service_results(
    services: &[status::ServiceStatus],
    output: OutputFormat,
) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(services, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = format!("Services ({})\n", services.len());
            s.push_str("===============\n");
            for svc in services {
                let _ = writeln!(
                    s,
                    "{}: {} - {} backends",
                    svc.name, svc.service_type, svc.backend_count
                );
            }
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Services\n\n");
            s.push_str("| Name | Type | Backends |\n");
            s.push_str("| --- | --- | --- |\n");
            for svc in services {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} |",
                    svc.name, svc.service_type, svc.backend_count
                );
            }
            Ok(s)
        }
    }
}

fn format_policy_result(
    result: &policy::PolicyValidationResult,
    output: OutputFormat,
) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(result, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = String::from("Policy Validation\n");
            s.push_str("=================\n");
            let _ = writeln!(s, "Valid: {}", result.is_valid);
            let _ = writeln!(s, "Policies checked: {}", result.policies_checked);
            if !result.errors.is_empty() {
                s.push_str("\nErrors:\n");
                for err in &result.errors {
                    let _ = writeln!(s, "  - {err}");
                }
            }
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Policy Validation\n\n");
            let _ = writeln!(s, "- **Valid**: {}", result.is_valid);
            let _ = writeln!(s, "- **Policies Checked**: {}", result.policies_checked);
            if !result.errors.is_empty() {
                s.push_str("\n## Errors\n\n");
                for err in &result.errors {
                    let _ = writeln!(s, "- {err}");
                }
            }
            Ok(s)
        }
    }
}

fn format_policy_list(policies: &[policy::PolicyInfo], output: OutputFormat) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(policies, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = format!("Policies ({})\n", policies.len());
            s.push_str("===============\n");
            for policy in policies {
                let _ = writeln!(s, "{}: {} rules", policy.name, policy.rule_count);
            }
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Policies\n\n");
            s.push_str("| Name | Namespace | Rules |\n");
            s.push_str("| --- | --- | --- |\n");
            for policy in policies {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} |",
                    policy.name, policy.namespace, policy.rule_count
                );
            }
            Ok(s)
        }
    }
}

fn format_flow_results(flows: &[flow::NetworkFlow], output: OutputFormat) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(flows, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = format!("Network Flows ({})\n", flows.len());
            s.push_str("==================\n");
            for flow in flows {
                let _ = writeln!(
                    s,
                    "{} -> {} ({}:{}): {}",
                    flow.source_pod, flow.dest_pod, flow.protocol, flow.dest_port, flow.status
                );
            }
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Network Flows\n\n");
            s.push_str("| Source | Dest | Protocol | Port | Status |\n");
            s.push_str("| --- | --- | --- | --- | --- |\n");
            for flow in flows {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} | {} |",
                    flow.source_pod, flow.dest_pod, flow.protocol, flow.dest_port, flow.status
                );
            }
            Ok(s)
        }
    }
}

fn format_flow_stats(stats: &flow::FlowStatistics, output: OutputFormat) -> Result<String> {
    match output {
        OutputFormat::Json | OutputFormat::JsonPretty | OutputFormat::Yaml => {
            serialize_output(stats, output)
        }
        OutputFormat::Table | OutputFormat::Summary => {
            let mut s = String::from("Flow Statistics\n");
            s.push_str("================\n");
            let _ = writeln!(s, "Total flows: {}", stats.total_flows);
            let _ = writeln!(s, "Allowed: {}", stats.allowed_flows);
            let _ = writeln!(s, "Denied: {}", stats.denied_flows);
            Ok(s)
        }
        OutputFormat::Markdown => {
            let mut s = String::from("# Flow Statistics\n\n");
            let _ = writeln!(s, "- **Total Flows**: {}", stats.total_flows);
            let _ = writeln!(s, "- **Allowed**: {}", stats.allowed_flows);
            let _ = writeln!(s, "- **Denied**: {}", stats.denied_flows);
            Ok(s)
        }
    }
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

fn config_summary(config: &RuntimeConfig) -> String {
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
            FeatureState {
                name: "connectivity-tests",
                enabled: true,
                description: "Track U connectivity tests available",
            },
            FeatureState {
                name: "policy-validation",
                enabled: true,
                description: "Track U policy validation available",
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

fn features_status_summary(report: &FeaturesStatusReport) -> String {
    let mut rendered = String::from("features status\n");
    rendered.push_str("================\n\n");
    for feature in &report.features {
        let _ = writeln!(
            rendered,
            "{}: {} - {}",
            feature.name,
            if feature.enabled {
                "enabled"
            } else {
                "disabled"
            },
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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

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
    fn temp_path_includes_name() {
        let path = temp_path("config");
        let rendered = path.display().to_string();
        assert!(rendered.contains("seriousum-cli-config-"));
    }

    // === Version command tests ===

    #[test]
    fn parses_version_command() {
        let cli = Cli::try_parse_from(["seriousum-cli", "version"]).expect("parse version command");
        assert_eq!(cli.command, Command::Version);
    }

    #[test]
    fn version_execution_includes_contract_and_core_versions() {
        let output = execute(Command::Version).expect("execute version command");
        assert!(output.contains(&format!("seriousum-cli {}", env!("CARGO_PKG_VERSION"))));
        assert!(output.contains(&format!("contract {CONTRACT_VERSION}")));
        assert!(output.contains(&format!("core {CORE_VERSION}")));
    }

    // === Config command tests ===

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

    // === Features command tests ===

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
    fn features_status_includes_track_u_features() {
        let output = execute(Command::Features {
            command: FeaturesCommand::Status {
                output: OutputFormat::Json,
                output_file: None,
            },
        })
        .expect("execute features status json");

        let value: serde_json::Value = serde_json::from_str(&output).expect("json output parses");
        assert_eq!(value["command"], "features status");

        // Verify Track U features are present
        let features_str = value["features"].to_string();
        assert!(features_str.contains("connectivity-tests"));
        assert!(features_str.contains("policy-validation"));
    }

    // === Sysdump command tests ===

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

    // === Connectivity command tests ===

    #[test]
    fn parses_connectivity_run_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "connectivity",
            "run",
            "--test-filter",
            "basic-connectivity",
            "-o",
            "json",
        ])
        .expect("parse connectivity run command");

        match cli.command {
            Command::Connectivity {
                command: ConnectivityCommand::Run { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_connectivity_check_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "connectivity",
            "check",
            "--source",
            "client",
            "--destination",
            "server",
            "--protocol",
            "tcp",
            "--port",
            "8080",
        ])
        .expect("parse connectivity check command");

        match cli.command {
            Command::Connectivity {
                command: ConnectivityCommand::Check { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_connectivity_list_tests_command() {
        let cli = Cli::try_parse_from(["seriousum-cli", "connectivity", "list-tests"])
            .expect("parse connectivity list-tests command");

        match cli.command {
            Command::Connectivity {
                command: ConnectivityCommand::ListTests,
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    // === Status command tests ===

    #[test]
    fn parses_status_cluster_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "status",
            "cluster",
            "--wait",
            "-o",
            "summary",
        ])
        .expect("parse status cluster command");

        match cli.command {
            Command::Status {
                command: StatusCommand::Cluster { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_status_endpoints_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "status",
            "endpoints",
            "--namespace",
            "default",
            "-o",
            "json",
        ])
        .expect("parse status endpoints command");

        match cli.command {
            Command::Status {
                command: StatusCommand::Endpoints { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_status_services_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "status",
            "services",
            "--namespace",
            "default",
        ])
        .expect("parse status services command");

        match cli.command {
            Command::Status {
                command: StatusCommand::Services { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    // === Policy command tests ===

    #[test]
    fn parses_policy_validate_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "policy",
            "validate",
            "--policy-file",
            "/tmp/policy.yaml",
            "-o",
            "json",
        ])
        .expect("parse policy validate command");

        match cli.command {
            Command::Policy {
                command: PolicyCommand::Validate { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_policy_check_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "policy",
            "check",
            "--source-pod",
            "client",
            "--dest-pod",
            "server",
            "--protocol",
            "tcp",
            "--port",
            "443",
        ])
        .expect("parse policy check command");

        match cli.command {
            Command::Policy {
                command: PolicyCommand::Check { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_policy_list_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "policy",
            "list",
            "--namespace",
            "kube-system",
        ])
        .expect("parse policy list command");

        match cli.command {
            Command::Policy {
                command: PolicyCommand::List { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    // === Flow command tests ===

    #[test]
    fn parses_flow_recent_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "flow",
            "recent",
            "--limit",
            "20",
            "--source-pod",
            "client",
            "-o",
            "json",
        ])
        .expect("parse flow recent command");

        match cli.command {
            Command::Flow {
                command: FlowCommand::Recent { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_flow_stats_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "flow",
            "stats",
            "--namespace",
            "default",
            "-o",
            "summary",
        ])
        .expect("parse flow stats command");

        match cli.command {
            Command::Flow {
                command: FlowCommand::Stats { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_flow_filter_command() {
        let cli = Cli::try_parse_from([
            "seriousum-cli",
            "flow",
            "filter",
            "--expression",
            "source.pod==client && dest.pod==server",
            "-o",
            "json",
        ])
        .expect("parse flow filter command");

        match cli.command {
            Command::Flow {
                command: FlowCommand::Filter { .. },
            } => {}
            _ => panic!("unexpected command"),
        }
    }

    // === Output format tests ===

    #[test]
    fn output_format_parsing() {
        assert_eq!(OutputFormat::Json, OutputFormat::Json);
        assert_eq!(OutputFormat::Markdown, OutputFormat::Markdown);
        assert_eq!(OutputFormat::Summary, OutputFormat::Summary);
    }

    #[test]
    fn test_output_format_roundtrip() {
        assert_eq!(
            <OutputFormat as std::str::FromStr>::from_str("json").unwrap(),
            OutputFormat::Json
        );
        assert_eq!(
            <OutputFormat as std::str::FromStr>::from_str("table").unwrap(),
            OutputFormat::Table
        );
        assert_eq!(
            <OutputFormat as std::str::FromStr>::from_str("yaml").unwrap(),
            OutputFormat::Yaml
        );
        assert!(<OutputFormat as std::str::FromStr>::from_str("invalid").is_err());
    }

    #[test]
    fn test_output_format_display() {
        assert_eq!(OutputFormat::Json.to_string(), "json");
        assert_eq!(OutputFormat::JsonPretty.to_string(), "json-pretty");
    }

    #[test]
    fn test_table_formatter_basic() {
        let mut tbl = TableFormatter::new(vec!["ID", "STATE", "IP"]);
        tbl.add_row(vec!["1", "ready", "10.0.0.1"]);
        tbl.add_row(vec!["2", "waiting", "10.0.0.2"]);
        let output = tbl.render();
        assert!(output.contains("ID"));
        assert!(output.contains("STATE"));
        assert!(output.contains("ready"));
        assert_eq!(tbl.row_count(), 2);
    }

    #[test]
    fn test_table_formatter_alignment() {
        let mut tbl = TableFormatter::new(vec!["SHORT", "A LONGER HEADER"]);
        tbl.add_row(vec!["x", "y"]);
        let output = tbl.render();
        assert!(output.lines().next().unwrap().contains("SHORT"));
    }

    #[test]
    fn test_table_formatter_empty() {
        let tbl = TableFormatter::new(Vec::<String>::new());
        assert_eq!(tbl.render(), "");
    }

    #[test]
    fn test_policy_map_summary_default() {
        let summary = PolicyMapSummary::default();
        assert_eq!(summary.ingress_allowed, 0);
        assert_eq!(summary.egress_denied, 0);
    }

    #[test]
    fn test_global_options_default() {
        let opts = GlobalOptions::default();
        assert_eq!(opts.host, "localhost:9900");
        assert_eq!(opts.output, OutputFormat::Table);
        assert_eq!(opts.timeout_secs, 30);
    }

    #[test]
    fn test_endpoint_row_serde() {
        let row = EndpointRow {
            id: 42,
            policy_enabled: "ingress+egress".into(),
            ipv4: "10.0.0.1".into(),
            ipv6: String::new(),
            identity: 100,
            labels: vec!["k8s:app=nginx".into()],
            state: "ready".into(),
        };
        let json = serde_json::to_string(&row).unwrap();
        let row2: EndpointRow = serde_json::from_str(&json).unwrap();
        assert_eq!(row2.id, 42);
    }

    // === Helper function tests ===

    #[test]
    fn features_status_report_has_track_u_features() {
        let report = features_status_report();
        assert_eq!(report.status, "ok");

        let feature_names: Vec<_> = report.features.iter().map(|f| f.name).collect();
        assert!(feature_names.contains(&"connectivity-tests"));
        assert!(feature_names.contains(&"policy-validation"));
    }

    #[test]
    fn features_status_markdown_renders() {
        let report = features_status_report();
        let markdown = features_status_markdown(&report);

        assert!(markdown.contains("# features status"));
        assert!(markdown.contains("| feature | enabled | description |"));
        assert!(markdown.contains("connectivity-tests"));
    }

    #[test]
    fn features_status_summary_renders() {
        let report = features_status_report();
        let summary = features_status_summary(&report);

        assert!(summary.contains("features status"));
        assert!(summary.contains("enabled"));
        assert!(summary.contains("connectivity-tests"));
    }
}
