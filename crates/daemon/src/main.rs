use clap::Parser;

fn main() -> std::process::ExitCode {
    seriousum_daemon::init_tracing();
    let cli = seriousum_daemon::Cli::parse();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match rt.block_on(seriousum_daemon::execute(cli)) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
