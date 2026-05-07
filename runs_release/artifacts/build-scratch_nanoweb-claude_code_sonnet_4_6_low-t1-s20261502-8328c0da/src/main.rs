use clap::Parser;
use nanoweb::{
    cli::CliArgs,
    config::Config,
    middleware::MiddlewareStack,
    router::Router,
    server::Server,
};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = CliArgs::parse();

    // Load config
    let mut config = Config::load(&cli.config)?;
    config.apply_cli_overrides(&cli);

    // Setup logging
    let log_level = config.log_level.clone();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&log_level)),
        )
        .init();

    info!("Starting nanoweb v{}", env!("CARGO_PKG_VERSION"));

    // Build router and middleware
    let mut router = Router::new();
    let middleware = MiddlewareStack::new();

    // Register routes from config
    for route_cfg in &config.routes {
        let response_body = route_cfg.response.clone();
        let status = route_cfg.status;
        let content_type = route_cfg
            .content_type
            .clone()
            .unwrap_or_else(|| "text/plain".to_string());
        let method = nanoweb::Method::from_str(&route_cfg.method);

        router.route(method, &route_cfg.path, move |_req| {
            let body = response_body.clone();
            let ct = content_type.clone();
            async move {
                let mut resp = nanoweb::Response::new(nanoweb::StatusCode(status));
                resp.headers.set("content-type", ct);
                resp.body = body.into_bytes();
                Ok(resp)
            }
        });
    }

    // Build and run server
    let server = Server::with_state(config.clone(), router, middleware)?;
    let server = Arc::new(server);

    // Setup graceful shutdown
    nanoweb::server::setup_shutdown_signal(server.clone()).await;

    // Check TLS
    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        #[cfg(feature = "tls")]
        {
            use nanoweb::tls::tls_impl::load_tls_config;
            let _acceptor = load_tls_config(cert, key)?;
            info!("TLS enabled");
        }
        #[cfg(not(feature = "tls"))]
        {
            eprintln!("Error: TLS cert/key provided but TLS feature is not enabled");
            std::process::exit(1);
        }
    }

    server.run().await?;

    std::process::exit(0);
}
