fn main() -> anyhow::Result<()> {
    let output = seriousum_hubble::run()?;
    tracing::info!("{output}");
    Ok(())
}
