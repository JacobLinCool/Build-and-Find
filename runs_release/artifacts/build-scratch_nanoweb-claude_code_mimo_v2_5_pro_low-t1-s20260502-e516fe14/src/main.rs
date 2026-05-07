use nanoweb::cli::Cli;
use nanoweb::config::Config;
use nanoweb::metrics::Metrics;
use nanoweb::middleware::{
    basic_auth::BasicAuthMiddleware, compression::CompressionMiddleware, cors::CorsMiddleware,
    csrf::CsrfMiddleware, logging::LoggingMiddleware, rate_limit::RateLimitMiddleware,
    BoxMiddleware,
};
use nanoweb::router::Router;
use nanoweb::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse_args();

    if cli.version {
        println!("nanoweb {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Initialize logging
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    // Load configuration
    let config = Config::load(
        &cli.config,
        Some(&cli.listen),
        cli.tls_cert.as_deref(),
        cli.tls_key.as_deref(),
        Some(&cli.metrics),
        cli.static_dir.as_deref(),
        Some(&cli.log_level),
    )?;

    tracing::info!("nanoweb starting with config: {}", config.config_path);

    // Build router from config
    let mut router = Router::new();

    // Register configured routes
    for route in &config.routes {
        let method = nanoweb::Method::from_str(&route.method.to_uppercase())
            .ok_or_else(|| format!("Unknown method: {}", route.method))?;
        let handler_name = route.handler.clone();
        router.route(method, &route.path, move |_req| {
            let handler_name = handler_name.clone();
            Box::pin(async move {
                nanoweb::Response::text(
                    nanoweb::StatusCode::OK,
                    format!("Handler: {}", handler_name),
                )
            })
        });
    }

    // Build middleware stack from config
    let mut middleware: Vec<BoxMiddleware> = Vec::new();

    // Always add logging
    middleware.push(Box::new(LoggingMiddleware::new()));

    for mw_config in &config.middleware {
        match mw_config.name.as_str() {
            "logging" => {
                // Already added above
            }
            "compression" => {
                middleware.push(Box::new(CompressionMiddleware::new()));
            }
            "rate_limit" => {
                let max = mw_config
                    .options
                    .get("max_requests")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(100) as u32;
                let window = mw_config
                    .options
                    .get("window_seconds")
                    .and_then(|v| v.as_integer())
                    .unwrap_or(60) as u32;
                middleware.push(Box::new(RateLimitMiddleware::new(max, window)));
            }
            "cors" => {
                let origins: Vec<String> = mw_config
                    .options
                    .get("allowed_origins")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["*".into()]);
                middleware.push(Box::new(CorsMiddleware::new(origins)));
            }
            "csrf" => {
                middleware.push(Box::new(CsrfMiddleware::new()));
            }
            "basic_auth" => {
                let username = mw_config
                    .options
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("admin");
                let password = mw_config
                    .options
                    .get("password")
                    .and_then(|v| v.as_str())
                    .unwrap_or("password");
                middleware.push(Box::new(BasicAuthMiddleware::new(username, password)));
            }
            _ => {
                tracing::warn!("Unknown middleware: {}", mw_config.name);
            }
        }
    }

    let metrics = Metrics::new();

    // TLS setup
    #[cfg(feature = "tls")]
    {
        if config.tls_cert.is_some() && config.tls_key.is_some() {
            tracing::info!("TLS configured");
            // TLS is handled at the accept level if needed
        }
    }

    #[cfg(not(feature = "tls"))]
    {
        if config.tls_cert.is_some() || config.tls_key.is_some() {
            eprintln!("Error: TLS support not compiled in. Enable the 'tls' feature.");
            std::process::exit(1);
        }
    }

    let server = Server::new(config, router, middleware, metrics);
    server.run().await?;

    Ok(())
}
