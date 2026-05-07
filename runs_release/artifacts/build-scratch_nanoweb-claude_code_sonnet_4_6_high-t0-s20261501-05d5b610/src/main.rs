use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};

use nanoweb::cli::{init_logging, Cli};
use nanoweb::health::HealthState;
use nanoweb::http::Response;
use nanoweb::metrics;
use nanoweb::middleware::cors::CorsConfig;
use nanoweb::middleware::{
    BasicAuthMiddleware, CompressionMiddleware, CorsMiddleware, CsrfMiddleware, LoggingMiddleware,
    RateLimitMiddleware,
};
use nanoweb::router::Router;
use nanoweb::server::{start_http, Server};
use nanoweb::static_files::StaticFileHandler;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let config = match cli.into_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    init_logging(&config.log_level);
    info!("Starting nanoweb server");

    // Initialize metrics
    if let Err(e) = metrics::init() {
        error!("Failed to initialize metrics: {}", e);
    }

    let health = Arc::new(HealthState::new());
    let mut router = Router::new();

    // Register global middleware
    if config.middleware.logging {
        router.use_middleware(LoggingMiddleware);
    }
    if config.middleware.compression {
        router.use_middleware(CompressionMiddleware::new());
    }
    if let Some(cors_cfg) = &config.middleware.cors {
        let cors = CorsMiddleware::new(CorsConfig {
            allowed_origins: cors_cfg.allowed_origins.clone(),
            allowed_methods: cors_cfg.allowed_methods.clone(),
            allowed_headers: cors_cfg.allowed_headers.clone(),
            allow_credentials: cors_cfg.allow_credentials,
            ..Default::default()
        });
        router.use_middleware(cors);
    }
    if let Some(rl_cfg) = &config.middleware.rate_limit {
        router.use_middleware(RateLimitMiddleware::new(
            rl_cfg.max_requests,
            rl_cfg.refill_rate,
        ));
    }
    if config.middleware.csrf {
        router.use_middleware(CsrfMiddleware::new());
    }
    if let Some(ba_cfg) = config.middleware.basic_auth.as_ref() {
        let username = ba_cfg.username.clone();
        let password = ba_cfg.password.clone();
        router.use_middleware(BasicAuthMiddleware::new(
            ba_cfg.realm.clone(),
            move |u, p| u == username && p == password,
        ));
    }

    // Health endpoints
    {
        let h = health.clone();
        let path = config.healthz_path.clone();
        router.get(&path, move |req| {
            let h = h.clone();
            Box::pin(async move { nanoweb::health::healthz_handler(h, req).await })
        });
    }
    {
        let h = health.clone();
        let path = config.readyz_path.clone();
        router.get(&path, move |req| {
            let h = h.clone();
            Box::pin(async move { nanoweb::health::readyz_handler(h, req).await })
        });
    }

    // Metrics endpoint
    {
        let metrics_path = config.metrics_path.clone();
        router.get(&metrics_path, |_req| {
            Box::pin(async move {
                match metrics::gather_text() {
                    Ok(text) => {
                        let mut resp = Response::new(200);
                        resp.headers
                            .insert("Content-Type", "text/plain; version=0.0.4");
                        resp.headers
                            .insert("Content-Length", text.len().to_string());
                        resp.body = text.into_bytes();
                        Ok(resp)
                    }
                    Err(e) => Err(e),
                }
            })
        });
    }

    // Static files
    if let Some(static_dir) = config.static_dir.clone() {
        let handler = Arc::new(StaticFileHandler::new(static_dir));
        router.get("/static/*path", move |req| {
            let h = handler.clone();
            Box::pin(async move { h.handle(&req).await })
        });
    }

    let server = Arc::new(Server::new(
        router,
        health.clone(),
        config.graceful_timeout_s,
    ));

    // Shutdown channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Start HTTP server
    let http_addr = config.listen.clone();
    let http_handle = match start_http(&http_addr, server.clone(), shutdown_rx.clone()).await {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to start HTTP server: {}", e);
            std::process::exit(1);
        }
    };

    // Start TLS server if configured
    #[cfg(feature = "tls")]
    let _tls_handle = if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        // HTTPS typically on a different port
        let tls_addr = derive_tls_addr(&config.listen);
        match nanoweb::server::start_https(
            &tls_addr,
            server.clone(),
            cert,
            key,
            shutdown_rx.clone(),
        )
        .await
        {
            Ok(h) => {
                info!("HTTPS listening on {}", tls_addr);
                Some(h)
            }
            Err(e) => {
                error!("Failed to start HTTPS server: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Wait for shutdown signal (SIGINT or SIGTERM)
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Received CTRL-C, shutting down");
        }
        _ = async {
            #[cfg(unix)]
            {
                let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to install SIGTERM handler");
                sigterm.recv().await;
                info!("Received SIGTERM, shutting down");
            }
            #[cfg(not(unix))]
            {
                std::future::pending::<()>().await
            }
        } => {}
    }

    // Trigger graceful shutdown
    let _ = shutdown_tx.send(true);

    // Wait for server to finish
    let _ = http_handle.await;

    info!("Server exited cleanly");
    std::process::exit(0);
}

#[cfg(feature = "tls")]
fn derive_tls_addr(http_addr: &str) -> String {
    // If HTTP is on port X, TLS is on port X+1 unless already specified
    if let Some(colon) = http_addr.rfind(':') {
        let host = &http_addr[..colon];
        let port: u16 = http_addr[colon + 1..].parse().unwrap_or(8080);
        format!("{}:{}", host, port + 1)
    } else {
        format!("{}:8443", http_addr)
    }
}
