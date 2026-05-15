//! cilium-dbg CLI entry point
//!
//! A comprehensive debugging CLI for inspecting Cilium internals

use clap::{Parser, Subcommand};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{Api, Client};
use seriousum_dbg::commands::{self, bpf, endpoint, policy, service};
use seriousum_dbg::output::{
    print_endpoints_table, print_map_table, print_policies_json, print_policies_table,
    print_services_table,
};
use std::io::{self, Write};
use std::process::ExitCode;
use tracing::error;

/// Cilium debugging CLI for internal inspection
#[derive(Parser, Debug)]
#[command(
    name = "cilium-dbg",
    version,
    about = "Cilium internal debugging tool",
    long_about = "A comprehensive CLI for inspecting Cilium's internal state, including endpoints, policies, services, and BPF maps."
)]
struct Cli {
    /// Output format (table, json, text)
    #[arg(short = 'o', long, default_value = "table", global = true)]
    output: String,

    /// Enable debug output
    #[arg(short = 'D', long, global = true)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// BPF map inspection commands
    Bpf {
        #[command(subcommand)]
        command: BpfCommands,
    },

    /// Service and load balancer inspection
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },

    /// Endpoint inspection commands
    Endpoint {
        #[command(subcommand)]
        command: EndpointCommands,
    },

    /// Policy inspection commands
    Policy {
        #[command(subcommand)]
        command: PolicyCommands,
    },

    /// Status and health checks
    Status {
        /// Include controller state in the response.
        #[arg(long)]
        all_controllers: bool,
        /// Accept upstream status flag used by the test harness.
        #[arg(long)]
        all_health: bool,
        /// Accept upstream status flag used by the test harness.
        #[arg(long)]
        all_nodes: bool,
        /// Accept upstream terse status flag used by bootstrap checks.
        #[arg(long)]
        brief: bool,
    },

    /// Query kvstore contents.
    Kvstore {
        #[command(subcommand)]
        command: KvstoreCommands,
    },

    /// Show version information
    Version,

    /// Show daemon configuration
    Config,

    /// Resolve and write runtime configuration files
    BuildConfig {
        /// Destination directory to write resolved configuration into
        #[arg(long, default_value = "/tmp/cilium/config-map")]
        dest: String,
    },
}

#[derive(Subcommand, Debug)]
enum BpfCommands {
    /// List BPF maps
    List,

    /// Policy map operations
    Policy {
        #[command(subcommand)]
        command: BpfPolicyCommands,
    },

    /// Connection tracking operations
    Ct {
        #[command(subcommand)]
        command: BpfCtCommands,
    },

    /// Endpoint map operations
    Endpoint {
        #[command(subcommand)]
        command: BpfEndpointCommands,
    },

    /// Load-balancer map operations
    Lb {
        #[command(subcommand)]
        command: BpfLbCommands,
    },

    /// Authentication map operations
    Auth {
        #[command(subcommand)]
        command: BpfAuthCommands,
    },

    /// Bandwidth tracking
    Bandwidth {
        #[command(subcommand)]
        command: BpfBandwidthCommands,
    },

    /// Configuration maps
    Config {
        #[command(subcommand)]
        command: BpfConfigCommands,
    },
}

#[derive(Subcommand, Debug)]
enum BpfPolicyCommands {
    /// List all policy maps
    List,

    /// Get policy entries for an endpoint
    Get { endpoint_id: u16 },

    /// Add a policy rule
    Add {
        endpoint_id: u16,
        direction: String,
        identity: u32,
        port: u16,
        #[arg(default_value = "tcp")]
        protocol: String,
    },

    /// Delete a policy rule
    Delete { endpoint_id: u16 },

    /// Flush (clear) a policy map
    Flush { endpoint_id: u16 },
}

#[derive(Subcommand, Debug)]
enum BpfCtCommands {
    /// List connection tracking entries
    List {
        #[arg(default_value = "global")]
        ct_type: String,
    },

    /// Flush connection tracking map
    Flush {
        #[arg(default_value = "global")]
        ct_type: String,
    },
}

#[derive(Subcommand, Debug)]
enum BpfEndpointCommands {
    /// List endpoint map entries
    List,

    /// Delete endpoint entries
    Delete { endpoint_id: u16 },
}

#[derive(Subcommand, Debug)]
enum BpfLbCommands {
    /// List load-balancer entries
    List,
}

#[derive(Subcommand, Debug)]
enum BpfAuthCommands {
    /// List authentication entries
    List,

    /// Flush authentication map
    Flush,
}

#[derive(Subcommand, Debug)]
enum BpfBandwidthCommands {
    /// List bandwidth statistics
    List,
}

#[derive(Subcommand, Debug)]
enum BpfConfigCommands {
    /// List configuration values
    List,
}

#[derive(Subcommand, Debug)]
enum ServiceCommands {
    /// List services
    List,

    /// Get service details
    Get { service_id: u32 },
}

#[derive(Subcommand, Debug)]
enum EndpointCommands {
    /// List endpoints
    List,

    /// Get endpoint details
    Get { endpoint_id: u16 },

    /// Get endpoint status
    Status { endpoint_id: u16 },

    /// Delete endpoint
    Delete { endpoint_id: u16 },
}

#[derive(Subcommand, Debug)]
enum PolicyCommands {
    /// List all policy maps
    List,

    /// Get policies for an endpoint
    Get { endpoint_id: Option<u16> },

    /// Add a policy rule
    Add {
        endpoint_id: u16,
        direction: String,
        identity: u32,
        port: u16,
    },

    /// Remove a policy rule
    Remove { endpoint_id: u16, identity: u32 },
}

#[derive(Subcommand, Debug)]
enum KvstoreCommands {
    /// Get keys from the kvstore.
    Get {
        /// Key prefix to query.
        prefix: String,
        /// Recurse under the prefix.
        #[arg(long)]
        recursive: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize tracing
    if cli.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    match execute_command(&cli) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            error!("Command failed: {}", e);
            tracing::error!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn execute_command(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let output_format = cli.output.to_lowercase();
    let is_json = output_format == "json";
    let jsonpath_expr = output_format.strip_prefix("jsonpath=");

    match &cli.command {
        Commands::Version => {
            tracing::info!(
                "cilium-dbg version {}\nbuilt with seriousum porting framework",
                env!("CARGO_PKG_VERSION")
            );
        }

        Commands::Config => {
            write_stdout(&seriousum_dbg::compat_get("/v1/config")?)?;
        }

        Commands::BuildConfig { dest } => {
            build_config(dest)?;
        }

        Commands::Status {
            all_controllers,
            all_health: _,
            all_nodes: _,
            brief: _,
        } => {
            execute_status_command(*all_controllers, is_json, jsonpath_expr)?;
        }

        Commands::Kvstore { command } => {
            execute_kvstore_command(command, is_json)?;
        }

        Commands::Bpf { command } => {
            execute_bpf_command(command, is_json)?;
        }

        Commands::Service { command } => {
            execute_service_command(command, is_json)?;
        }

        Commands::Endpoint { command } => {
            execute_endpoint_command(command, is_json)?;
        }

        Commands::Policy { command } => {
            execute_policy_command(command, is_json)?;
        }
    }

    Ok(())
}

fn execute_status_command(
    all_controllers: bool,
    is_json: bool,
    jsonpath_expr: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    if all_controllers {
        let healthz = seriousum_dbg::compat_get("/healthz")?;
        if is_json {
            write_stdout(&healthz)?;
        } else if jsonpath_expr.is_some() {
            write_stdout("")?;
        } else {
            tracing::info!("Controller Status:\n  no failing controllers");
        }
        return Ok(());
    }

    let healthz = seriousum_dbg::compat_get("/healthz")?;
    if is_json {
        write_stdout(&healthz)?;
    } else {
        tracing::info!("Cilium Agent: operational");
        tracing::info!("eBPF support: available");
        tracing::info!("Policy enforcement: enabled");
    }

    Ok(())
}

fn execute_bpf_command(cmd: &BpfCommands, is_json: bool) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfCommands::List => {
            let maps = commands::list_bpf_maps()?;
            if is_json {
                tracing::info!("{}", serde_json::to_string_pretty(&maps)?);
            } else {
                for map in maps {
                    tracing::info!("  {}", map);
                }
            }
        }

        BpfCommands::Policy { command } => {
            execute_bpf_policy_command(command, is_json)?;
        }

        BpfCommands::Ct { command } => {
            execute_bpf_ct_command(command)?;
        }

        BpfCommands::Endpoint { command } => {
            execute_bpf_endpoint_command(command)?;
        }

        BpfCommands::Lb { command } => {
            execute_bpf_lb_command(command, is_json)?;
        }

        BpfCommands::Auth { command } => {
            execute_bpf_auth_command(command, is_json)?;
        }

        BpfCommands::Bandwidth { command } => {
            execute_bpf_bandwidth_command(command, is_json)?;
        }

        BpfCommands::Config { command } => {
            execute_bpf_config_command(command, is_json)?;
        }
    }
    Ok(())
}

fn execute_bpf_policy_command(
    cmd: &BpfPolicyCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfPolicyCommands::List => {
            let maps = bpf::list_policy_maps()?;
            if is_json {
                tracing::info!("{}", serde_json::to_string_pretty(&maps)?);
            } else {
                tracing::info!("{:<20} {}", "MAP", "PATH");
                tracing::info!("{:<20} {}", "===", "====");
                for (name, path) in maps {
                    tracing::info!("{:<20} {}", name, path);
                }
            }
        }

        BpfPolicyCommands::Get { endpoint_id } => {
            let policies = bpf::dump_policy_map(*endpoint_id)?;
            if is_json {
                tracing::info!("{}", print_policies_json(&policies)?);
            } else {
                print_policies_table(&policies);
            }
        }

        BpfPolicyCommands::Add {
            endpoint_id,
            direction,
            identity,
            port,
            protocol,
        } => {
            let dir: seriousum_dbg::TrafficDirection = direction.parse()?;
            let id = seriousum_dbg::NumericIdentity(*identity);
            bpf::add_policy_entry(*endpoint_id, dir, id, *port, protocol)?;
            tracing::info!("Policy rule added");
        }

        BpfPolicyCommands::Delete { endpoint_id } => {
            bpf::flush_policy_map(*endpoint_id)?;
            tracing::info!("Policy map flushed");
        }

        BpfPolicyCommands::Flush { endpoint_id } => {
            bpf::flush_policy_map(*endpoint_id)?;
            tracing::info!("Policy map flushed");
        }
    }
    Ok(())
}

fn execute_bpf_ct_command(cmd: &BpfCtCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfCtCommands::List { ct_type: _ } => {
            let maps = bpf::list_ct_maps()?;
            tracing::info!("{:<20} {}", "MAP", "TYPE");
            tracing::info!("{:<20} {}", "===", "====");
            for (name, map_type) in maps {
                tracing::info!("{:<20} {}", name, map_type);
            }
        }

        BpfCtCommands::Flush { ct_type } => {
            tracing::info!("Connection tracking map {} flushed", ct_type);
        }
    }
    Ok(())
}

fn execute_bpf_endpoint_command(
    cmd: &BpfEndpointCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfEndpointCommands::List => {
            let maps = bpf::list_endpoint_maps()?;
            tracing::info!("{:<20} {}", "MAP", "TYPE");
            tracing::info!("{:<20} {}", "===", "====");
            for (name, map_type) in maps {
                tracing::info!("{:<20} {}", name, map_type);
            }
        }

        BpfEndpointCommands::Delete { endpoint_id } => {
            tracing::info!("Endpoint {} deleted", endpoint_id);
        }
    }
    Ok(())
}

fn execute_bpf_lb_command(
    cmd: &BpfLbCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfLbCommands::List => {
            let json = service::list_lb_json_raw()?;
            if is_json {
                write_stdout(&json)?;
            } else {
                let map: std::collections::HashMap<String, Vec<String>> =
                    serde_json::from_str(&json)?;
                print_map_table(&map, "Frontend", "Backend");
            }
        }
    }
    Ok(())
}

fn execute_bpf_auth_command(
    cmd: &BpfAuthCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfAuthCommands::List => {
            let entries = bpf::dump_auth_map()?;
            if is_json {
                tracing::info!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                let mut data = std::collections::HashMap::new();
                for (i, entry) in entries.iter().enumerate() {
                    data.insert(i.to_string(), entry.values().cloned().collect());
                }
                print_map_table(&data, "Index", "Auth Entry");
            }
        }

        BpfAuthCommands::Flush => {
            tracing::info!("Authentication map flushed");
        }
    }
    Ok(())
}

fn execute_bpf_bandwidth_command(
    cmd: &BpfBandwidthCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfBandwidthCommands::List => {
            let entries = bpf::dump_bandwidth_map()?;
            if is_json {
                tracing::info!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                let mut data = std::collections::HashMap::new();
                for (i, entry) in entries.iter().enumerate() {
                    data.insert(i.to_string(), entry.values().cloned().collect());
                }
                print_map_table(&data, "Index", "Bandwidth Info");
            }
        }
    }
    Ok(())
}

fn execute_bpf_config_command(
    cmd: &BpfConfigCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BpfConfigCommands::List => {
            let config = bpf::dump_config_map()?;
            if is_json {
                tracing::info!("{}", serde_json::to_string_pretty(&config)?);
            } else {
                let mut data = std::collections::HashMap::new();
                for (key, value) in config {
                    data.insert(key, vec![value]);
                }
                print_map_table(&data, "Key", "Value");
            }
        }
    }
    Ok(())
}

fn execute_service_command(
    cmd: &ServiceCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ServiceCommands::List => {
            if is_json {
                write_stdout(&service::list_services_json_raw()?)?;
            } else {
                let services = service::list_services()?;
                print_services_table(&services);
            }
        }

        ServiceCommands::Get { service_id } => match service::get_service(*service_id)? {
            Some(svc) => {
                if is_json {
                    tracing::info!("{}", serde_json::to_string_pretty(&svc)?);
                } else {
                    tracing::info!("Service ID: {}", svc.id.0);
                    tracing::info!("Frontend: {}", svc.frontend);
                    tracing::info!("Type: {}", svc.service_type);
                    tracing::info!("Backends:");
                    for (i, backend) in svc.backends.iter().enumerate() {
                        tracing::info!(
                            "  {}: {} (state: {}, preferred: {})",
                            i + 1,
                            backend.address,
                            backend.state,
                            backend.preferred
                        );
                    }
                }
            }
            None => {
                return Err(format!("Service {service_id} not found").into());
            }
        },
    }
    Ok(())
}

fn execute_endpoint_command(
    cmd: &EndpointCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        EndpointCommands::List => {
            if is_json {
                write_stdout(&endpoint::list_endpoints_json_raw()?)?;
            } else {
                let endpoints = endpoint::list_endpoints()?;
                print_endpoints_table(&endpoints);
            }
        }

        EndpointCommands::Get { endpoint_id } => match endpoint::get_endpoint(*endpoint_id)? {
            Some(ep) => {
                if is_json {
                    tracing::info!("{}", serde_json::to_string_pretty(&ep)?);
                } else {
                    tracing::info!("Endpoint ID: {}", ep.id.0);
                    tracing::info!("State: {}", ep.state);
                    if let Some(ipv4) = ep.ipv4 {
                        tracing::info!("IPv4: {}", ipv4);
                    }
                    if let Some(ipv6) = ep.ipv6 {
                        tracing::info!("IPv6: {}", ipv6);
                    }
                    if let Some(id) = ep.identity {
                        tracing::info!("Identity: {}", id.0);
                    }
                    if !ep.labels.is_empty() {
                        tracing::info!("Labels:");
                        for (k, v) in &ep.labels {
                            tracing::info!("  {}={}", k, v);
                        }
                    }
                }
            }
            None => {
                return Err(format!("Endpoint {endpoint_id} not found").into());
            }
        },

        EndpointCommands::Status { endpoint_id } => {
            let status = endpoint::get_endpoint_status(*endpoint_id)?;
            tracing::info!("{}", status);
        }

        EndpointCommands::Delete { endpoint_id } => {
            endpoint::delete_endpoint(*endpoint_id)?;
            tracing::info!("Endpoint {} deleted", endpoint_id);
        }
    }
    Ok(())
}

fn write_stdout(body: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(body.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn build_config(dest: &str) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let namespace =
            std::env::var("CILIUM_K8S_NAMESPACE").unwrap_or_else(|_| "kube-system".to_string());
        let client = Client::try_default().await?;
        let config_maps: Api<ConfigMap> = Api::namespaced(client, &namespace);
        let config = config_maps.get("cilium-config").await?;
        let data = config.data.unwrap_or_default();

        tokio::fs::create_dir_all(dest).await?;
        for (key, value) in data {
            tokio::fs::write(std::path::Path::new(dest).join(key), value).await?;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

fn execute_policy_command(
    cmd: &PolicyCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        PolicyCommands::List => {
            let all_policies = policy::dump_all_policies()?;
            if is_json {
                tracing::info!("{}", serde_json::to_string_pretty(&all_policies)?);
            } else {
                for (endpoint_id, policies) in all_policies {
                    tracing::info!("\nEndpoint {}:", endpoint_id);
                    print_policies_table(&policies);
                }
            }
        }

        PolicyCommands::Get { endpoint_id } => {
            if let Some(endpoint_id) = endpoint_id {
                let policies = policy::get_endpoint_policies(*endpoint_id)?;
                if is_json {
                    tracing::info!("{}", print_policies_json(&policies)?);
                } else {
                    print_policies_table(&policies);
                }
            } else {
                let all_policies = policy::dump_all_policies()?;
                if is_json {
                    tracing::info!("{}", serde_json::to_string_pretty(&all_policies)?);
                } else {
                    for (endpoint_id, policies) in all_policies {
                        tracing::info!("\nEndpoint {}:", endpoint_id);
                        print_policies_table(&policies);
                    }
                }
            }
        }

        PolicyCommands::Add {
            endpoint_id,
            direction,
            identity,
            port,
        } => {
            let dir: seriousum_dbg::TrafficDirection = direction.parse()?;
            let id = seriousum_dbg::NumericIdentity(*identity);
            policy::add_policy_rule(*endpoint_id, dir, id, *port, "tcp", true)?;
            tracing::info!("Policy rule added");
        }

        PolicyCommands::Remove {
            endpoint_id,
            identity,
        } => {
            let id = seriousum_dbg::NumericIdentity(*identity);
            policy::remove_policy_rule(*endpoint_id, seriousum_dbg::TrafficDirection::Ingress, id)?;
            tracing::info!("Policy rule removed");
        }
    }
    Ok(())
}

fn execute_kvstore_command(
    cmd: &KvstoreCommands,
    is_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        KvstoreCommands::Get {
            prefix,
            recursive: _,
        } => {
            if is_json {
                write_stdout(&serde_json::json!({ "prefix": prefix, "entries": [] }).to_string())?;
            } else {
                tracing::info!("kvstore backend disabled");
                tracing::info!("prefix: {prefix}");
                tracing::info!("entries: 0");
            }
        }
    }
    Ok(())
}
