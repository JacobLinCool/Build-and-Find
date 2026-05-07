use clap::Parser;
use nanoweb::{NanowebError, cli::Cli, config::Config, server::Server};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), NanowebError> {
    let cli = Cli::parse();
    let config = Config::load(&cli)?;
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::new(config.log_level.clone()))
        .init();
    if config.tls_cert.is_some() || config.tls_key.is_some() {
        match (&config.tls_cert, &config.tls_key) {
            (Some(cert), Some(key)) => {
                let _ = nanoweb::tls::load_config(cert, key)?;
            }
            _ => {
                return Err(NanowebError::Tls(
                    "both tls_cert and tls_key are required".into(),
                ));
            }
        }
    }
    Server::from_config(config)?.run().await
}
