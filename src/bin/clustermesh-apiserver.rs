fn main() -> anyhow::Result<()> {
    let output = seriousum_clustermesh::run()?;
    tracing::info!("{output}");
    Ok(())
}
