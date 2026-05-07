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

use std::env;
use std::process;

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    let cli_args = match cli::CliArgs::parse(&args) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(2);
        }
    };

    let exit_code = cli::run(&cli_args);
    process::exit(exit_code);
}
