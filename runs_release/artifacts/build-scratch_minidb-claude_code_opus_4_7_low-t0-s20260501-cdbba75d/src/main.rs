use minidb::cli::{parse_args, run};
use minidb::error::exit_code_for;
use std::env;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = env::args().collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(exit_code_for(&e));
        }
    };
    match run(parsed) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(exit_code_for(&e));
        }
    }
}
