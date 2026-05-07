//! Binary entry point. All logic is implemented in the `minidb` library
//! crate; this just forwards the command-line arguments.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(minidb::cli::run(args));
}
