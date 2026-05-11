use clap::Parser;
use seriousum_operator::Operator;

/// Minimal CLI for the seriousum operator scaffold.
#[derive(Debug, Parser)]
#[command(name = "seriousum-operator", about = "seriousum operator scaffold")]
struct Cli {
    /// Human-readable summary for the initial operator state.
    #[arg(long, default_value = "operator scaffold ready")]
    summary: String,
}

fn main() {
    let cli = Cli::parse();
    let operator = Operator::new(cli.summary);
    let scaffold = operator.scaffold_payloads();

    match serde_json::to_string_pretty(&scaffold) {
        Ok(payload) => println!("{payload}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
