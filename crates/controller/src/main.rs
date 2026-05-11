fn main() -> std::process::ExitCode {
    match seriousum_controller::run() {
        Ok(output) => {
            println!("{output}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
