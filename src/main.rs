fn main() -> anyhow::Result<()> {
    let output = seriousum_cli::run()?;
    tracing::info!("{output}");
    Ok(())
}
