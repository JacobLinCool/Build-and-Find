use nanoweb::cli::parse_args;
use nanoweb::config::Config;
use nanoweb::server::Server;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let cli = parse_args();

    // Initialize logging
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    // Load config
    let mut config = match cli.config.as_deref() {
        Some(path) if std::path::Path::new(path).exists() => {
            Config::load(path).unwrap_or_else(|e| {
                tracing::warn!("Failed to load config from {}: {}, using defaults", path, e);
                Config::default()
            })
        }
        _ => Config::default(),
    };

    config.merge_cli(&cli);

    // Validate TLS config
    if config.tls_cert.is_some() != config.tls_key.is_some() {
        eprintln!("Error: both --tls-cert and --tls-key must be provided together");
        std::process::exit(1);
    }

    // Create and configure server
    let mut server = Server::new(config.clone());

    if let Some(ref static_dir) = config.static_dir {
        server.set_static_dir(static_dir);
    }

    // Run server
    if let Err(e) = server.run().await {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}
