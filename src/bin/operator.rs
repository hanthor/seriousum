fn main() -> anyhow::Result<()> {
    let output = seriousum_operator::run()?;
    println!("{output}");
    Ok(())
}
