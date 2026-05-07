//! Binary entry point. All real work lives in `minidb::cli`.

fn main() {
    let code = minidb::cli::run();
    std::process::exit(code);
}
