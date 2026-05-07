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
    std::process::exit(cli::run());
}
