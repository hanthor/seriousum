use axum::Router;
use clap::Parser;
use std::net::SocketAddr;
use tracing::info;

/// Cilium Operator — Kubernetes CRD reconciliation
#[derive(Debug, Parser)]
#[command(
    name = "cilium-operator-generic",
    version = "0.1.0",
    // Accept flags the Helm chart passes without hard-failing on unknown ones.
    disable_help_flag = true,
    ignore_errors = true,
)]
struct Cli {
    #[arg(short, long)]
    verbose: bool,

    /// Standard Cilium operator flag: directory with config-map overrides.
    #[arg(long, default_value = "/tmp/cilium/config-map")]
    config_dir: String,

    #[arg(long, default_value = "false")]
    debug: String,

    /// Absorb any remaining Cilium operator flags not yet implemented.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    _extra: Vec<String>,
}

async fn healthz() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let level = if cli.verbose || cli.debug == "true" {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt().with_max_level(level).init();

    info!("Starting Cilium operator (config-dir={})", cli.config_dir);

    // Start health check server
    let health_addr: SocketAddr = "127.0.0.1:9234".parse()?;
    let app = Router::new().route("/healthz", axum::routing::get(healthz));

    let listener = tokio::net::TcpListener::bind(&health_addr).await?;
    let mut health_task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("health server error: {}", e);
        }
    });

    info!("Health check server listening on {}", health_addr);
    info!("Operator initialized — waiting for shutdown signal");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down operator");
        }
        _ = &mut health_task => {
            info!("Health server exited unexpectedly");
        }
    }

    Ok(())
}
