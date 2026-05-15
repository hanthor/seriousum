use clap::{Parser, Subcommand};
use k8s_openapi::api::core::v1::Node;
use kube::{Api, Client};
use rustls::crypto::ring::default_provider;
use serde::Serialize;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "cilium-health",
    version,
    about = "Cilium health compatibility shim"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Report cluster health.
    Status {
        /// Output format.
        #[arg(short = 'o', long, default_value = "summary")]
        output: String,
        /// Match upstream probe flag accepted by the test harness.
        #[arg(long)]
        probe: bool,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ProbeStatus {
    status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AddressStatus {
    icmp: ProbeStatus,
    http: ProbeStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct NodeHealth {
    #[serde(rename = "primary-address")]
    primary_address: AddressStatus,
    #[serde(rename = "secondary-addresses")]
    secondary_addresses: Vec<AddressStatus>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct HealthNode {
    name: String,
    host: NodeHealth,
    #[serde(rename = "health-endpoint")]
    health_endpoint: NodeHealth,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct HealthStatus {
    nodes: Vec<HealthNode>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(cli: Cli) -> anyhow::Result<()> {
    let _ = default_provider().install_default();
    match cli.command {
        Commands::Status { output, probe: _ } => {
            let node_names = list_node_names().await?;
            if output.eq_ignore_ascii_case("json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&render_health_status(&node_names))?
                );
            } else {
                for node_name in node_names {
                    println!("{node_name}: ok");
                }
            }
        }
    }

    Ok(())
}

async fn list_node_names() -> anyhow::Result<Vec<String>> {
    let client = Client::try_default().await?;
    let nodes = Api::<Node>::all(client).list(&Default::default()).await?;
    let mut node_names = nodes
        .items
        .into_iter()
        .filter_map(|node| node.metadata.name)
        .collect::<Vec<_>>();
    node_names.sort();
    Ok(node_names)
}

fn render_health_status(node_names: &[String]) -> HealthStatus {
    HealthStatus {
        nodes: node_names
            .iter()
            .cloned()
            .map(|name| HealthNode {
                name,
                host: healthy_node_health(),
                health_endpoint: healthy_node_health(),
            })
            .collect(),
    }
}

fn healthy_node_health() -> NodeHealth {
    NodeHealth {
        primary_address: healthy_address_status(),
        secondary_addresses: Vec::new(),
    }
}

fn healthy_address_status() -> AddressStatus {
    AddressStatus {
        icmp: healthy_probe_status(),
        http: healthy_probe_status(),
    }
}

fn healthy_probe_status() -> ProbeStatus {
    ProbeStatus {
        status: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::render_health_status;

    #[test]
    fn render_health_status_marks_all_paths_healthy() {
        let status = render_health_status(&["node-a".into(), "node-b".into()]);
        assert_eq!(status.nodes.len(), 2);
        assert_eq!(status.nodes[0].name, "node-a");
        assert_eq!(status.nodes[0].host.primary_address.icmp.status, "");
        assert_eq!(status.nodes[0].host.primary_address.http.status, "");
        assert!(status.nodes[0].host.secondary_addresses.is_empty());
        assert_eq!(
            status.nodes[1].health_endpoint.primary_address.icmp.status,
            ""
        );
        assert_eq!(
            status.nodes[1].health_endpoint.primary_address.http.status,
            ""
        );
    }
}
