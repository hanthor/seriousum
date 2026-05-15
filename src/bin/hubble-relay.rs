use clap::{Parser, Subcommand};
use seriousum_hubble::relay::{RelayConfig, RelayServer};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use tonic::transport::Server;
use tonic_health::ServingStatus;
use tonic_health::server::health_reporter;
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(name = "hubble-relay", version, about = "Seriousum Hubble Relay")]
struct Cli {
    #[arg(short = 'D', long, global = true)]
    debug: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the Hubble relay server.
    Serve,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.debug);

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
    }
}

async fn serve() -> anyhow::Result<()> {
    let relay = RelayServer::new(RelayConfig::default());
    relay.start().await?;

    let grpc_addr = parse_listen_addr(&RelayConfig::default().listen_address)?;
    let health_addr = parse_listen_addr(&RelayConfig::default().health_listen_address)?;

    let grpc_handle = tokio::spawn(run_health_server(grpc_addr));
    let health_handle = tokio::spawn(run_health_server(health_addr));

    info!(%grpc_addr, %health_addr, "hubble-relay serving");

    tokio::select! {
        result = grpc_handle => result??,
        result = health_handle => result??,
        _ = wait_for_shutdown() => {
            info!("hubble-relay shutting down");
        }
    }

    relay.stop().await?;
    Ok(())
}

async fn run_health_server(addr: SocketAddr) -> anyhow::Result<()> {
    let (reporter, service) = health_reporter();
    reporter
        .set_service_status("", ServingStatus::Serving)
        .await;

    Server::builder().add_service(service).serve(addr).await?;
    Ok(())
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }

    let _ = tokio::signal::ctrl_c().await;
}

fn parse_listen_addr(address: &str) -> anyhow::Result<SocketAddr> {
    if let Some(port) = address.strip_prefix(':') {
        return Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port.parse()?,
        ));
    }

    Ok(address.parse()?)
}

fn init_tracing(debug: bool) {
    let level = if debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt().with_max_level(level).init();
}

#[cfg(test)]
mod tests {
    use super::parse_listen_addr;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn parse_short_listen_addr() {
        let address = parse_listen_addr(":4245").expect("short address parses");
        assert_eq!(
            address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4245)
        );
    }

    #[test]
    fn parse_full_listen_addr() {
        let address = parse_listen_addr("127.0.0.1:4222").expect("full address parses");
        assert_eq!(
            address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4222)
        );
    }
}
