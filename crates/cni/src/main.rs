use std::io::Write;

use seriousum_cni::{CniContext, PluginError, run_plugin};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cilium-cni: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), PluginError> {
    let ctx = CniContext::from_env()?;
    let output = run_plugin(&ctx)?;
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}
