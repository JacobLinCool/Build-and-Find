use clap::Parser;
use nanoweb::cli::Cli;
use nanoweb::config::Config;
use nanoweb::server;

fn main() {
    let cli = Cli::parse();

    // Load config
    let mut config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            // If config file doesn't exist, use defaults
            if std::path::Path::new(&cli.config).exists() {
                eprintln!("Error loading config: {e}");
                std::process::exit(1);
            }
            Config::default_config()
        }
    };

    // CLI overrides TOML
    config.merge_cli(&cli);

    // Set up tokio runtime
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    if let Err(e) = runtime.block_on(server::run(config, &cli)) {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}
