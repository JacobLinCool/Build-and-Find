use nanoweb::{
    cli::Cli,
    config::{Config, MiddlewareConfig},
    error::NanowebError,
    health::HealthState,
    metrics::Metrics,
    middleware::{
        basic_auth::BasicAuthMiddleware,
        cors::{CorsConfig, CorsMiddleware},
        csrf::CsrfMiddleware,
        gzip::GzipMiddleware,
        logging::LoggingMiddleware,
        rate_limit::RateLimitMiddleware,
        FnHandler,
    },
    server::Server,
    static_files::StaticFileHandler,
    Request, Response, Router,
};
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse_args();

    // Load config
    let mut config = Config::from_file_or_default(&cli.config);
    config.apply_cli_overrides(
        cli.listen,
        cli.tls_cert,
        cli.tls_key,
        cli.metrics,
        cli.static_dir,
        cli.log_level.clone(),
    );

    // Setup logging
    let log_level = config.log_level.clone();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));
    fmt().with_env_filter(filter).with_target(false).init();

    let mut server = Server::new(config.clone())?;

    // Clone references before borrowing router mutably
    let metrics = server.metrics.clone();
    let health = server.health.clone();
    let health_clone = health.clone();

    // Configure global middleware and routes
    {
        let router = server.router_mut();
        for mw in &config.middleware {
            match mw {
                MiddlewareConfig::Logging => {
                    router.add_global_middleware(Arc::new(LoggingMiddleware));
                }
                MiddlewareConfig::Gzip { min_size } => {
                    router
                        .add_global_middleware(Arc::new(GzipMiddleware::with_min_size(*min_size)));
                }
                MiddlewareConfig::RateLimit {
                    requests_per_second,
                } => {
                    router.add_global_middleware(Arc::new(RateLimitMiddleware::new(
                        *requests_per_second,
                    )));
                }
                MiddlewareConfig::Cors {
                    allowed_origins,
                    allow_credentials,
                } => {
                    let cors_config = CorsConfig {
                        allowed_origins: allowed_origins.clone(),
                        allow_credentials: *allow_credentials,
                        ..CorsConfig::default()
                    };
                    router.add_global_middleware(Arc::new(CorsMiddleware::new(cors_config)));
                }
                MiddlewareConfig::Csrf => {
                    router.add_global_middleware(Arc::new(CsrfMiddleware::new()));
                }
                MiddlewareConfig::BasicAuth {
                    realm,
                    username,
                    password,
                } => {
                    router.add_global_middleware(Arc::new(BasicAuthMiddleware::single(
                        realm, username, password,
                    )));
                }
            }
        }

        // Register metrics endpoint
        let metrics_path = config.metrics_path.clone();
        let metrics_handler = nanoweb::metrics::metrics_handler(metrics);
        router.get(&metrics_path, Arc::new(FnHandler::new(metrics_handler)));

        // Register health endpoints
        router.get(
            "/healthz",
            Arc::new(FnHandler::new(nanoweb::health::liveness_handler(health))),
        );
        router.get(
            "/readyz",
            Arc::new(FnHandler::new(nanoweb::health::readiness_handler(
                health_clone,
            ))),
        );

        // Register configured static file routes
        for static_cfg in &config.static_files {
            let handler = StaticFileHandler::new(static_cfg.dir.clone(), static_cfg.route.clone());
            let handler = Arc::new(handler);
            let route = format!("{}/*path", static_cfg.route.trim_end_matches('/'));
            let h = handler.clone();
            router.route(
                "GET",
                &route,
                Arc::new(FnHandler::new(move |req: Request| {
                    let h = h.clone();
                    async move { h.serve(&req).await }
                })),
                vec![],
            );
        }

        // Register static_dir shortcut if provided
        if let Some(static_dir) = &config.static_dir {
            let handler = StaticFileHandler::new(static_dir.clone(), "/static");
            let handler = Arc::new(handler);
            let h = handler.clone();
            router.route(
                "GET",
                "/static/*path",
                Arc::new(FnHandler::new(move |req: Request| {
                    let h = h.clone();
                    async move { h.serve(&req).await }
                })),
                vec![],
            );
        }

        // TLS setup
        #[cfg(feature = "tls")]
        if let Some(tls_config) = &config.tls {
            match nanoweb::tls::acceptor::load_tls_config(&tls_config.cert, &tls_config.key) {
                Ok(_tls_cfg) => {
                    tracing::info!("TLS configured, listening on {}", tls_config.listen);
                }
                Err(e) => {
                    tracing::error!("Failed to load TLS config: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    server.run().await?;

    std::process::exit(0);
}
