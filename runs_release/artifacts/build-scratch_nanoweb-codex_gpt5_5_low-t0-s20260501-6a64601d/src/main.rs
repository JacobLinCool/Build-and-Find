use nanoweb::{NanowebError, cli, config, server::Server};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), NanowebError> {
    let cli = cli::parse();
    let config = config::load(&cli)?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    if config.tls_cert.is_some() || config.tls_key.is_some() {
        let tls_config = match (&config.tls_cert, &config.tls_key) {
            (Some(cert), Some(key)) => nanoweb::tls::load_config(cert, key)?,
            _ => {
                return Err(NanowebError::Tls(
                    "both --tls-cert and --tls-key are required".into(),
                ));
            }
        };
        return Server::from_config(config)?.run_tls(tls_config).await;
    }

    Server::from_config(config)?.run().await
}
