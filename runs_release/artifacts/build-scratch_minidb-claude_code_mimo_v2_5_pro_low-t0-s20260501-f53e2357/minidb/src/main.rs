#![allow(dead_code)]

mod catalog;
mod cli;
mod error;
mod exec;
mod lexer;
mod parser;
mod plan;
mod storage;
mod txn;
mod types;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    // Skip program name
    let args = &args[1..];
    let cli_args = match cli::CliArgs::parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(2);
        }
    };
    let code = cli::run(cli_args);
    std::process::exit(code);
}
