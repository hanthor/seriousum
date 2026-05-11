use clap::Parser;

fn main() -> anyhow::Result<()> {
    seriousum_daemon::init_tracing();
    let cli = seriousum_daemon::Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(seriousum_daemon::execute(cli))
}
