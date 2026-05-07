use crate::cli::Cli;
use crate::config::{Config, RouteConfig};
use crate::health::HealthState;
use crate::http::parser::parse_request;
use crate::http::{Method, Request, Response};
use crate::metrics::{MetricsRegistry, RequestTimer};
use crate::middleware::basic_auth::BasicAuthMiddleware;
use crate::middleware::cors::CorsMiddleware;
use crate::middleware::csrf::CsrfMiddleware;
use crate::middleware::gzip::GzipMiddleware;
use crate::middleware::logging::LoggingMiddleware;
use crate::middleware::rate_limit::RateLimitMiddleware;
use crate::middleware::MiddlewareChain;
use crate::router::Router;
use crate::session::{InMemorySessionStore, SessionManager};
use crate::static_files::StaticFileHandler;
use crate::template::Renderer;

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

const MAX_REQUEST_SIZE: usize = 1024 * 1024; // 1MB for headers

pub struct AppState {
    pub router: Router,
    pub middleware: MiddlewareChain,
    pub metrics: Arc<MetricsRegistry>,
    pub health: HealthState,
    pub static_handler: Option<StaticFileHandler>,
    pub session_manager: SessionManager,
    pub renderer: Renderer,
    pub config: Config,
}

pub async fn run(config: Config, cli: &Cli) -> Result<(), crate::error::NanowebError> {
    let listen_addr = config.listen.clone();

    // Set up tracing
    let level = match config.log_level.as_str() {
        "trace" => tracing::level_filters::LevelFilter::TRACE,
        "debug" => tracing::level_filters::LevelFilter::DEBUG,
        "info" => tracing::level_filters::LevelFilter::INFO,
        "warn" => tracing::level_filters::LevelFilter::WARN,
        "error" => tracing::level_filters::LevelFilter::ERROR,
        _ => tracing::level_filters::LevelFilter::INFO,
    };
    tracing_subscriber::fmt().with_max_level(level).init();

    let metrics = Arc::new(MetricsRegistry::new());
    let health = HealthState::new();
    let renderer = Renderer::new();

    // Set up static file handler
    let static_handler = cli
        .static_dir
        .as_ref()
        .or(config.static_dir.as_ref())
        .map(|dir| StaticFileHandler::new(dir));

    // Set up session manager
    let session_store = Box::new(InMemorySessionStore::new());
    let session_manager = SessionManager::new(session_store, config.session_secret.as_bytes());

    // Build router
    let mut router = Router::new();
    register_builtin_routes(&mut router, &metrics, &health);

    // Register configured routes
    for route in &config.routes {
        register_config_route(&mut router, route, &renderer, &metrics);
    }

    // Build middleware chain
    let middleware = build_middleware_chain(&config, cli);

    let state = Arc::new(AppState {
        router,
        middleware,
        metrics,
        health: health.clone(),
        static_handler,
        session_manager,
        renderer,
        config: config.clone(),
    });

    // Graceful shutdown signal
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Set up signal handlers
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM");
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
        tracing::info!("shutdown signal received");
        let _ = shutdown_tx_clone.send(true);
    });

    // TLS or plain listener
    #[cfg(feature = "tls")]
    {
        if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
            let tls_config = crate::tls::config::load_tls_config(cert, key)?;
            let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
            let listener = TcpListener::bind(&listen_addr).await?;
            tracing::info!("HTTPS server listening on {listen_addr}");
            accept_loop(listener, state, shutdown_rx.clone(), Some(acceptor)).await;
            health.set_ready(false);
            return Ok(());
        }
    }

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("HTTP server listening on {listen_addr}");
    accept_loop(listener, state, shutdown_rx.clone(), None).await;
    health.set_ready(false);

    Ok(())
}

async fn accept_loop(
    listener: TcpListener,
    state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
    #[cfg(feature = "tls")] tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
    #[cfg(not(feature = "tls"))] _tls_acceptor: Option<()>,
) {
    let graceful_timeout = std::time::Duration::from_secs(state.config.graceful_timeout_s);
    let active_connections = Arc::new(tokio::sync::Semaphore::new(10000));

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        let state = state.clone();
                        let _permit = active_connections.clone().acquire_owned().await.unwrap();
                        #[cfg(feature = "tls")]
                        let acceptor = tls_acceptor.clone();
                        tokio::spawn(async move {
                            let result = if let Some(acceptor) = acceptor {
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => handle_connection(tls_stream, addr, state).await,
                                    Err(e) => {
                                        tracing::error!("TLS handshake failed: {e}");
                                        Ok(())
                                    }
                                }
                            } else {
                                handle_connection(stream, addr, state).await
                            };
                            drop(_permit);
                            if let Err(e) = result {
                                tracing::error!("connection error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("accept error: {e}");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("graceful shutdown: waiting {graceful_timeout:?} for in-flight requests");
                    // Wait for active connections to drain
                    tokio::time::sleep(graceful_timeout).await;
                    break;
                }
            }
        }
    }
}

async fn handle_connection(
    mut stream: impl AsyncReadExt + AsyncWriteExt + Unpin,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
) -> Result<(), crate::error::NanowebError> {
    loop {
        let mut buf = vec![0u8; MAX_REQUEST_SIZE];
        let n = match stream.read(&mut buf).await {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(_) => return Ok(()),
        };
        buf.truncate(n);

        let request_id = uuid::Uuid::new_v4().to_string();

        let mut req = match parse_request(&buf, addr, request_id.clone()) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::text(e.status, &e.message);
                let bytes = serialize_response(&resp);
                let _ = stream.write_all(&bytes).await;
                return Ok(());
            }
        };

        // Set request-id header
        req.headers
            .insert("x-request-id".into(), request_id.clone());

        let timer = RequestTimer::start();
        let mut resp = handle_request(&req, &state).await;
        let elapsed = timer.elapsed();

        // Add request-id to response
        resp.set_header("x-request-id", request_id.clone());

        // Record metrics (skip metrics endpoint itself to avoid recursion)
        let route_label = if req.path == state.config.metrics {
            "/metrics".to_string()
        } else {
            req.path.clone()
        };
        state
            .metrics
            .inc_request_counter(req.method.as_str(), resp.status, &route_label);
        state
            .metrics
            .record_duration(req.method.as_str(), resp.status, &route_label, elapsed);

        let bytes = serialize_response(&resp);
        let _ = stream.write_all(&bytes).await;

        // Check connection header
        let keep_alive = req
            .header("connection")
            .map(|v| v.to_ascii_lowercase() != "close")
            .unwrap_or(req.version == "HTTP/1.1");

        if !keep_alive {
            return Ok(());
        }
    }
}

pub async fn handle_request(req: &Request, state: &AppState) -> Response {
    let path = req.path.clone();
    let method = req.method;

    // Built-in routes
    match path.as_str() {
        "/healthz" => return state.health.healthz(),
        "/readyz" => return state.health.readyz(),
        _ if path == state.config.metrics => {
            return Response::text(200, state.metrics.render());
        }
        _ => {}
    }

    // Static files
    if let Some(ref static_handler) = state.static_handler {
        if let Some(resp) = static_handler.handle(req) {
            return resp;
        }
    }

    // Router dispatch
    let mut req = req.clone();

    // Run before middleware
    if let Some(resp) = crate::middleware::run_before(&state.middleware, &mut req).await {
        return resp;
    }

    let mut resp = match state.router.find(method, &path) {
        Some((handler, params)) => {
            req.params = params;
            handler(req.clone()).await
        }
        None if method == Method::Head => {
            // HEAD falls back to GET handler with body suppressed
            if let Some((handler, params)) = state.router.find(Method::Get, &path) {
                req.params = params;
                let mut resp = handler(req.clone()).await;
                resp.body.clear();
                resp
            } else {
                let allowed = state.router.allowed_methods(&path);
                if !allowed.is_empty() {
                    let mut resp = Response::text(405, "Method Not Allowed");
                    let methods: Vec<String> =
                        allowed.iter().map(|m| m.as_str().to_string()).collect();
                    resp.set_header("allow", methods.join(", "));
                    resp
                } else {
                    Response::text(404, "Not Found")
                }
            }
        }
        None => {
            // Check if route exists for other methods (405)
            let allowed = state.router.allowed_methods(&path);
            if !allowed.is_empty() {
                let mut resp = Response::text(405, "Method Not Allowed");
                let methods: Vec<String> = allowed.iter().map(|m| m.as_str().to_string()).collect();
                resp.set_header("allow", methods.join(", "));
                resp
            } else {
                Response::text(404, "Not Found")
            }
        }
    };

    // Run after middleware
    crate::middleware::run_after(&state.middleware, &req, &mut resp).await;

    resp
}

fn serialize_response(resp: &Response) -> Vec<u8> {
    let status_text = crate::error::HttpError::status_text(resp.status);
    let mut output = format!("HTTP/1.1 {} {}\r\n", resp.status, status_text);

    for (name, value) in &resp.headers {
        output.push_str(&format!("{name}: {value}\r\n"));
    }

    // Ensure content-length is set
    let has_content_length = resp
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    if !has_content_length {
        output.push_str(&format!("content-length: {}\r\n", resp.body.len()));
    }

    output.push_str("connection: keep-alive\r\n");
    output.push_str("\r\n");

    let mut bytes = output.into_bytes();
    bytes.extend_from_slice(&resp.body);
    bytes
}

fn register_builtin_routes(
    _router: &mut Router,
    _metrics: &Arc<MetricsRegistry>,
    _health: &HealthState,
) {
    // Built-in routes are handled directly in handle_request
}

fn register_config_route(
    router: &mut Router,
    route: &RouteConfig,
    _renderer: &Renderer,
    _metrics: &Arc<MetricsRegistry>,
) {
    let method = match route.method.to_uppercase().as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "DELETE" => Method::Delete,
        "PATCH" => Method::Patch,
        "HEAD" => Method::Head,
        "OPTIONS" => Method::Options,
        _ => {
            tracing::warn!("unsupported method: {}", route.method);
            return;
        }
    };

    let handler_name = route.handler.clone();
    let body = route.body.clone();
    let status = route.status.unwrap_or(200);
    let content_type = route.content_type.clone();
    let file_path = route.file.clone();

    let handler: crate::router::Handler = Box::new(move |_req: Request| {
        let handler_name = handler_name.clone();
        let body = body.clone();
        let content_type = content_type.clone();
        let file_path = file_path.clone();
        Box::pin(async move {
            if let Some(file) = file_path {
                match std::fs::read_to_string(&file) {
                    Ok(content) => {
                        let ct = content_type.unwrap_or_else(|| "text/html; charset=utf-8".into());
                        let mut resp = Response::new(status);
                        resp.set_header("content-type", ct);
                        resp.set_header("content-length", content.len().to_string());
                        resp.body = content.into_bytes();
                        resp
                    }
                    Err(e) => Response::text(500, format!("error reading file: {e}")),
                }
            } else if let Some(body) = body {
                let ct = content_type.unwrap_or_else(|| "text/plain; charset=utf-8".into());
                let mut resp = Response::new(status);
                resp.set_header("content-type", ct);
                resp.set_header("content-length", body.len().to_string());
                resp.body = body.into_bytes();
                resp
            } else {
                match handler_name.as_str() {
                    "hello" => Response::text(200, "Hello, World!"),
                    "echo" => Response::text(200, "echo"),
                    _ => Response::text(500, format!("unknown handler: {handler_name}")),
                }
            }
        })
    });

    router.add(method, &route.path, handler);
}

fn build_middleware_chain(config: &Config, _cli: &Cli) -> MiddlewareChain {
    let mut chain: MiddlewareChain = Vec::new();

    if config.middleware.logging {
        chain.push(Box::new(LoggingMiddleware));
    }

    if config.middleware.csrf {
        chain.push(Box::new(CsrfMiddleware::new()));
    }

    if let Some(ref cors_config) = config.middleware.cors {
        chain.push(Box::new(CorsMiddleware::new(
            cors_config.allowed_origins.clone(),
        )));
    }

    if let Some(ref rl_config) = config.middleware.rate_limit {
        chain.push(Box::new(RateLimitMiddleware::new(
            rl_config.capacity,
            rl_config.refill_rate,
        )));
    }

    if let Some(ref auth_config) = config.middleware.basic_auth {
        chain.push(Box::new(BasicAuthMiddleware::new(
            auth_config.username.clone(),
            auth_config.password.clone(),
        )));
    }

    if config.middleware.gzip {
        chain.push(Box::new(GzipMiddleware));
    }

    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_serialize_response() {
        let resp = Response::text(200, "OK");
        let bytes = serialize_response(&resp);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("content-length: 2\r\n"));
        assert!(text.contains("\r\n\r\nOK"));
    }

    #[test]
    fn test_build_middleware_chain() {
        let config = Config::default_config();
        let cli = Cli::parse_from(["nanoweb"]);
        let chain = build_middleware_chain(&config, &cli);
        assert!(chain.is_empty());
    }
}
