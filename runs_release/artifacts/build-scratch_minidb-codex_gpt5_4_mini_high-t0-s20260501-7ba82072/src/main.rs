use minidb::cli;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}
