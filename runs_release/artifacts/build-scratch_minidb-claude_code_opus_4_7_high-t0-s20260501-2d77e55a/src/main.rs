//! Binary entry point for `minidb`.

use std::process::ExitCode;

use minidb::cli::{parse_args, run};

fn main() -> ExitCode {
    // Logging: respect RUST_LOG, default off.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();

    let args = match parse_args(std::env::args()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::from(e.exit_code() as u8);
        }
    };
    match run(args) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("error: {}", e);
            ExitCode::from(e.exit_code() as u8)
        }
    }
}
