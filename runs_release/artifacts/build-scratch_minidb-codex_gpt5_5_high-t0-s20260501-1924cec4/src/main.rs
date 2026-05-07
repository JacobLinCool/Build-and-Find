fn main() {
    let code = match minidb::cli::run() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            err.exit_code()
        }
    };
    std::process::exit(code);
}
