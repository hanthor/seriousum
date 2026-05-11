fn main() -> anyhow::Result<()> {
    let output = seriousum_cli::run()?;
    println!("{output}");
    Ok(())
}
