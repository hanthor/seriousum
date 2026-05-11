fn main() -> anyhow::Result<()> {
    let output = seriousum_clustermesh::run()?;
    println!("{output}");
    Ok(())
}
