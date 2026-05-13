// Operator is built as a library with its own binary in crates/operator/src/main.rs
// This file is kept for compatibility but the actual binary runs via:
// cargo run --bin seriousum-operator (from workspace) or
// cargo run (from crates/operator/)

fn main() -> anyhow::Result<()> {
    tracing::error!("Please run: cargo run --bin seriousum-operator from workspace root");
    Ok(())
}
