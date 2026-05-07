fn main() {
    if let Err(err) = minidb::cli::run() {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}
