use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Buf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::timeout;

use crate::config::Config;
use crate::error::Result;
use crate::health::HealthState;
use crate::http::parser;
use crate::http::types::{Method, Request, Response};
use crate::metrics::Metrics;
use crate::middleware::MiddlewareStack;
use crate::router::Router;
use crate::static_files::StaticFiles;

#[derive(Clone)]
struct AppState {
    router: Arc<Router>,
    metrics: Arc<Metrics>,
    health_state: Arc<HealthState>,
    config: Arc<Config>,
    middleware_stack: Arc<MiddlewareStack>,
}

pub struct Server {
    state: AppState,
}

impl Server {
    pub fn new(config: Config) -> Self {
        let metrics = Arc::new(Metrics::new());
        let health_state = Arc::new(HealthState::new());

        let mut middleware_stack = MiddlewareStack::new();

        // Register standard middleware based on config
        for mw_name in &config.middleware {
            match mw_name.as_str() {
                "logging" => {
                    middleware_stack.push(Box::new(crate::middleware::logging::RequestLogging));
                }
                "gzip" => {
                    middleware_stack.push(Box::new(crate::middleware::gzip::GzipCompression));
                }
                "cors" => {
                    middleware_stack.push(Box::new(crate::middleware::cors::Cors::new(
                        config.cors.clone(),
                    )));
                }
                "csrf" => {
                    middleware_stack.push(Box::new(crate::middleware::csrf::CsrfProtection::new()));
                }
                "rate-limit" => {
                    middleware_stack.push(Box::new(
                        crate::middleware::ratelimit::RateLimiter::new(
                            config.rate_limit.requests_per_second,
                            config.rate_limit.burst,
                        ),
                    ));
                }
                _ => {}
            }
        }

        let mut router = Router::new();

        // Register built-in routes
        let m = metrics.clone();
        let mp = config.metrics_path.clone();
        router.route(Method::Get, &mp, move |req| {
            crate::metrics::metrics_handler(&m, &req)
        });

        let _h = health_state.clone();
        router.route(Method::Get, "/healthz", move |_req| {
            crate::health::healthz_handler(&_req)
        });

        let h2 = health_state.clone();
        router.route(Method::Get, "/readyz", move |req| {
            crate::health::readyz_handler(&h2, &req)
        });

        let state = AppState {
            router: Arc::new(router),
            metrics,
            health_state,
            config: Arc::new(config),
            middleware_stack: Arc::new(middleware_stack),
        };

        Self { state }
    }

    pub fn router_mut(&mut self) -> &mut Router {
        Arc::get_mut(&mut self.state.router).expect("Router has multiple references")
    }

    pub fn push_middleware(&mut self, m: Box<dyn crate::middleware::Middleware>) {
        Arc::get_mut(&mut self.state.middleware_stack)
            .expect("MiddlewareStack has multiple references")
            .push(m);
    }

    pub async fn run(self) -> Result<()> {
        let config = &self.state.config;
        let addr: SocketAddr = config.listen.parse().map_err(|e| {
            crate::error::NanowebError::Config(format!("Invalid listen address: {e}"))
        })?;

        let listener = TcpListener::bind(addr).await?;
        tracing::info!(address = %addr, "Server listening");

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let running = Arc::new(AtomicBool::new(true));

        // Signal handler
        let shutdown_tx_clone = shutdown_tx.clone();
        let running_clone = running.clone();
        tokio::spawn(async move {
            wait_for_shutdown_signal().await;
            tracing::info!("Shutdown signal received");
            running_clone.store(false, Ordering::SeqCst);
            let _ = shutdown_tx_clone.send(());
        });

        // Set ready
        self.state.health_state.set_ready(true);

        let mut shutdown_rx = shutdown_tx.subscribe();
        let mut join_handles = Vec::new();
        let graceful_timeout = Duration::from_secs(self.state.config.graceful_timeout_s);

        // Start TLS listener if configured
        let tls_handle = if config.tls_configured() {
            #[cfg(feature = "tls")]
            {
                let cert_path = config.tls_cert.as_ref().unwrap().clone();
                let key_path = config.tls_key.as_ref().unwrap().clone();
                let state = self.state.clone();
                let running = running.clone();
                let mut tls_shutdown_rx = shutdown_tx.subscribe();

                Some(tokio::spawn(async move {
                    match crate::tls::TlsAcceptorWrapper::new(&cert_path, &key_path) {
                        Ok(tls_acceptor) => {
                            let tls_addr = format!("{}:{}", addr.ip(), addr.port() + 1);
                            let tls_addr: SocketAddr = tls_addr.parse().unwrap();
                            let tls_listener = TcpListener::bind(tls_addr).await.unwrap();
                            tracing::info!(address = %tls_addr, "TLS server listening");

                            loop {
                                tokio::select! {
                                    result = tls_listener.accept() => {
                                        match result {
                                            Ok((tcp_stream, _)) => {
                                                if !running.load(Ordering::SeqCst) {
                                                    break;
                                                }
                                                let state = state.clone();
                                                match tls_acceptor.accept(tcp_stream).await {
                                                    Ok(tls_stream) => {
                                                        tokio::spawn(async move {
                                                            if let Err(e) = handle_tls_connection(tls_stream, state).await {
                                                                tracing::debug!(error = %e, "TLS connection error");
                                                            }
                                                        });
                                                    }
                                                    Err(e) => {
                                                        tracing::debug!(error = %e, "TLS handshake failed");
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(error = %e, "TLS accept error");
                                            }
                                        }
                                    }
                                    _ = tls_shutdown_rx.recv() => {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to initialize TLS");
                        }
                    }
                }))
            }

            #[cfg(not(feature = "tls"))]
            {
                tracing::error!("TLS configured but not compiled with 'tls' feature");
                None
            }
        } else {
            None
        };

        // Accept loop
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _peer)) => {
                            if !running.load(Ordering::SeqCst) {
                                break;
                            }

                            let state = self.state.clone();
                            let handle = tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, state).await {
                                    tracing::debug!(error = %e, "Connection error");
                                }
                            });
                            join_handles.push(handle);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Accept error");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }

        // Graceful shutdown
        tracing::info!(
            timeout_secs = graceful_timeout.as_secs(),
            "Waiting for in-flight requests"
        );

        let deadline = Instant::now() + graceful_timeout;
        for handle in join_handles {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                tracing::warn!("Graceful timeout reached, aborting remaining connections");
                break;
            }
            match timeout(remaining, handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::debug!(error = %e, "Connection task panicked"),
                Err(_) => tracing::warn!("Connection timed out during graceful shutdown"),
            }
        }

        if let Some(tls_handle) = tls_handle {
            let _ = tls_handle.await;
        }

        tracing::info!("Server stopped");
        Ok(())
    }
}

async fn handle_connection(mut stream: TcpStream, state: AppState) -> Result<()> {
    let mut buf = bytes::BytesMut::with_capacity(8192);

    loop {
        let n = stream.read_buf(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        match parser::parse_request(&buf) {
            Ok(Some(mut req)) => {
                let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
                let body_start = header_end + 4;

                let content_length = req
                    .headers
                    .get("content-length")
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let is_chunked = req
                    .headers
                    .get("transfer-encoding")
                    .map(|v| v.to_lowercase().contains("chunked"))
                    .unwrap_or(false);

                let consumed = if is_chunked {
                    let remaining = &buf[body_start..];
                    parser::find_chunked_end(remaining)
                        .map(|end| body_start + end)
                        .unwrap_or(buf.len())
                } else {
                    body_start + content_length
                };

                buf.advance(consumed);

                let start = Instant::now();
                let mut resp = handle_request(&mut req, &state).await;
                let duration = start.elapsed().as_secs_f64();

                state
                    .metrics
                    .record_request(req.method.as_str(), resp.status, &req.path, duration);

                // HEAD falls back to GET but body must be suppressed
                if req.method == Method::Head {
                    resp.body.clear();
                }

                resp.headers.insert("x-request-id", req.request_id.clone());

                let response_bytes = parser::format_response(&resp);
                stream.write_all(&response_bytes).await?;

                let close = req
                    .header("connection")
                    .map(|v| v.to_lowercase() == "close")
                    .unwrap_or(false);

                if close || req.version == "HTTP/1.0" {
                    break;
                }
            }
            Ok(None) => {
                if buf.len() > 1024 * 1024 {
                    let resp = Response::bad_request("Request too large");
                    let response_bytes = parser::format_response(&resp);
                    stream.write_all(&response_bytes).await?;
                    break;
                }
                continue;
            }
            Err(e) => {
                let resp = Response::bad_request(&e.to_string());
                let response_bytes = parser::format_response(&resp);
                stream.write_all(&response_bytes).await?;
                break;
            }
        }
    }

    Ok(())
}

#[cfg(feature = "tls")]
async fn handle_tls_connection(
    mut stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    state: AppState,
) -> Result<()> {
    let mut buf = bytes::BytesMut::with_capacity(8192);

    loop {
        let n = stream.read_buf(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        match parser::parse_request(&buf) {
            Ok(Some(mut req)) => {
                let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
                let body_start = header_end + 4;

                let content_length = req
                    .headers
                    .get("content-length")
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let is_chunked = req
                    .headers
                    .get("transfer-encoding")
                    .map(|v| v.to_lowercase().contains("chunked"))
                    .unwrap_or(false);

                let consumed = if is_chunked {
                    let remaining = &buf[body_start..];
                    parser::find_chunked_end(remaining)
                        .map(|end| body_start + end)
                        .unwrap_or(buf.len())
                } else {
                    body_start + content_length
                };

                buf.advance(consumed);

                let start = Instant::now();
                let mut resp = handle_request(&mut req, &state).await;
                let duration = start.elapsed().as_secs_f64();

                state
                    .metrics
                    .record_request(req.method.as_str(), resp.status, &req.path, duration);

                // HEAD falls back to GET but body must be suppressed
                if req.method == Method::Head {
                    resp.body.clear();
                }

                resp.headers.insert("x-request-id", req.request_id.clone());

                let response_bytes = parser::format_response(&resp);
                stream.write_all(&response_bytes).await?;

                let close = req
                    .header("connection")
                    .map(|v| v.to_lowercase() == "close")
                    .unwrap_or(false);

                if close || req.version == "HTTP/1.0" {
                    break;
                }
            }
            Ok(None) => {
                if buf.len() > 1024 * 1024 {
                    let resp = Response::bad_request("Request too large");
                    let response_bytes = parser::format_response(&resp);
                    stream.write_all(&response_bytes).await?;
                    break;
                }
                continue;
            }
            Err(e) => {
                let resp = Response::bad_request(&e.to_string());
                let response_bytes = parser::format_response(&resp);
                stream.write_all(&response_bytes).await?;
                break;
            }
        }
    }

    Ok(())
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
    }
}

async fn handle_request(req: &mut Request, state: &AppState) -> Response {
    // Handle OPTIONS specially
    if req.method == Method::Options {
        return state.router.handle_options(&req.path);
    }

    // Static files
    if let Some(ref static_dir) = state.config.static_dir {
        let static_files = StaticFiles::new(static_dir);
        if let Some(resp) = static_files.serve(req) {
            return resp;
        }
    }

    // Route through middleware and router
    state
        .middleware_stack
        .run(req, &|r: &mut Request| state.router.handle(r.clone()))
}
