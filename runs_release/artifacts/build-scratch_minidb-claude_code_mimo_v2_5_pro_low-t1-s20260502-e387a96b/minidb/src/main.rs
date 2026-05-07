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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().collect();
    let config = match cli::parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    let code = cli::run(config);
    std::process::exit(code);
}
