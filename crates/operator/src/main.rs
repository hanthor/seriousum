use clap::Parser;

/// Minimal CLI for the seriousum operator scaffold.
#[derive(Debug, Parser)]
#[command(name = "seriousum-operator", about = "seriousum operator scaffold")]
struct Cli {
    /// Human-readable summary for the initial operator state.
    #[arg(long, default_value = "operator scaffold ready")]
    summary: String,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match seriousum_operator::run_with_summary(cli.summary) {
        Ok(payload) => {
            println!("{payload}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
