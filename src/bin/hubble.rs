fn main() -> anyhow::Result<()> {
    let output = seriousum_hubble::run()?;
    println!("{output}");
    Ok(())
}
