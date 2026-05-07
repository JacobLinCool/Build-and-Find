use clap::Parser;
use nanoweb::cli::Cli;
use nanoweb::config::{CliConfig, Config};
use nanoweb::http::{Method, Response};
use nanoweb::router::{RouteEntry, Router};
use nanoweb::server::Server;
use nanoweb::NanowebError;
use std::sync::Arc;

fn main() -> Result<(), NanowebError> {
    let cli = Cli::parse();

    // Initialize tracing
    let log_level = match cli.log_level.as_str() {
        "trace" => tracing::level_filters::LevelFilter::TRACE,
        "debug" => tracing::level_filters::LevelFilter::DEBUG,
        "info" => tracing::level_filters::LevelFilter::INFO,
        "warn" => tracing::level_filters::LevelFilter::WARN,
        "error" => tracing::level_filters::LevelFilter::ERROR,
        _ => tracing::level_filters::LevelFilter::INFO,
    };
    tracing_subscriber::fmt().with_max_level(log_level).init();

    tracing::info!("nanoweb v{}", env!("CARGO_PKG_VERSION"));

    // Load config
    let cli_config = CliConfig {
        listen: cli.listen.clone(),
        config: cli.config.clone(),
        tls_cert: cli.tls_cert.clone(),
        tls_key: cli.tls_key.clone(),
        metrics: cli.metrics.clone(),
        static_dir: cli.static_dir.clone(),
        log_level: cli.log_level.clone(),
    };

    let mut config = if cli.config.exists() {
        Config::load(cli.config.to_str().unwrap_or("./nanoweb.toml")).unwrap_or_else(|e| {
            tracing::warn!("Could not load config file, using defaults: {e}");
            Config::default()
        })
    } else {
        Config::default()
    };

    config.merge_cli(&cli_config);

    // Validate TLS config
    if config.tls_cert.is_some() || config.tls_key.is_some() {
        nanoweb::tls::check_tls_available()?;
        if config.tls_cert.is_none() || config.tls_key.is_none() {
            return Err(NanowebError::Tls(
                "Both --tls-cert and --tls-key must be provided for TLS".to_string(),
            ));
        }
    }

    // Build router with default routes
    let router = build_router(&config);

    // Create and run server
    let runtime = tokio::runtime::Runtime::new().map_err(|e| NanowebError::Io(e))?;
    runtime.block_on(async {
        let server = Arc::new(Server::new(config, router));
        server.run().await
    })
}

fn build_router(_config: &Config) -> Router {
    let mut router = Router::new();

    // Health endpoints are handled directly in server dispatch
    // Add a default hello endpoint
    router.add_route(Method::Get, "/", RouteEntry {
        handler: Box::new(|_req| {
            Box::pin(async {
                Response::html(200, "<!DOCTYPE html><html><body><h1>Welcome to nanoweb</h1><p>A self-contained HTTP/1.1 web framework and server.</p></body></html>")
            })
        }),
        middleware: vec![],
    });

    router.add_route(
        Method::Get,
        "/hello",
        RouteEntry {
            handler: Box::new(|req| {
                Box::pin(async move {
                    let name = req.query_param("name").unwrap_or("World");
                    Response::text(200, &format!("Hello, {name}!"))
                })
            }),
            middleware: vec![],
        },
    );

    // Echo endpoint
    router.add_route(
        Method::Post,
        "/echo",
        RouteEntry {
            handler: Box::new(|req| {
                Box::pin(async move {
                    let body = String::from_utf8_lossy(&req.body).to_string();
                    let mut resp = Response::text(200, &body);
                    resp.set_header("content-type", req.content_type().unwrap_or("text/plain"));
                    resp
                })
            }),
            middleware: vec![],
        },
    );

    router
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_router() {
        let config = Config::default();
        let router = build_router(&config);
        assert!(router.route(Method::Get, "/").is_some());
        assert!(router.route(Method::Get, "/hello").is_some());
        assert!(router.route(Method::Post, "/echo").is_some());
    }
}
