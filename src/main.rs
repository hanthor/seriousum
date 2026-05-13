use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let bin_name = args
        .first()
        .and_then(|arg| std::path::Path::new(arg).file_name())
        .and_then(|f| f.to_str())
        .unwrap_or_default();
    let daemon_mode = args.get(1).is_some_and(|arg| arg == "daemon")
        || bin_name == "cilium-agent"
        || (args.len() == 1 && std::env::var_os("CILIUM_AGENT").is_some());

    if daemon_mode {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .init();

        let daemon_args = daemon_cli_args(args);
        let cli = seriousum_daemon::Cli::parse_from(daemon_args);
        let runtime_config = seriousum_daemon::load_config(cli.config)?;
        let config = seriousum_daemon::daemon_config_from_runtime_config(&runtime_config)?;
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let runtime = seriousum_daemon::DaemonRuntime::new(config);
            runtime
                .run()
                .await
                .map(|_| ())
                .map_err(|error| anyhow::anyhow!("{error}"))
        })?;
    } else {
        let output = seriousum_cli::run()?;
        tracing::info!("{output}");
    }

    Ok(())
}

fn daemon_cli_args(mut args: Vec<String>) -> Vec<String> {
    if args.get(1).is_some_and(|arg| arg == "daemon") {
        args.remove(1);
    }
    args
}
