use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::config::Config;
use crate::handler::HandlerFn;
use crate::health::HealthState;
use crate::http::parser::parse_request;
use crate::http::{Method, Response};
use crate::metrics::Metrics;
use crate::middleware::Middleware;
use crate::router::Router;
use crate::static_files::StaticFiles;

pub struct Server {
    config: Config,
    router: Arc<Router>,
    handlers: Arc<HashMap<usize, HandlerFn>>,
    global_middleware: Vec<Arc<dyn Middleware>>,
    health_state: Arc<HealthState>,
    metrics: Arc<Metrics>,
    running: Arc<AtomicBool>,
    notify: Arc<Notify>,
    static_files: Option<StaticFiles>,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Server {
            config,
            router: Arc::new(Router::new()),
            handlers: Arc::new(HashMap::new()),
            global_middleware: Vec::new(),
            health_state: Arc::new(HealthState::new()),
            metrics: Arc::new(Metrics::new()),
            running: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
            static_files: None,
        }
    }

    pub fn set_router(&mut self, router: Router, handlers: HashMap<usize, HandlerFn>) {
        self.router = Arc::new(router);
        self.handlers = Arc::new(handlers);
    }

    pub fn add_global_middleware(&mut self, mw: Arc<dyn Middleware>) {
        self.global_middleware.push(mw);
    }

    pub fn set_static_files(&mut self, dir: &str) {
        self.static_files = Some(StaticFiles::new(dir));
    }

    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    pub fn health_state(&self) -> Arc<HealthState> {
        self.health_state.clone()
    }

    pub async fn run(self) -> crate::error::Result<()> {
        let addr: SocketAddr = self.config.listen.parse().map_err(|e| {
            crate::error::NanowebError::Config(format!("Invalid listen address: {}", e))
        })?;

        let listener = TcpListener::bind(addr).await?;
        tracing::info!("Listening on {}", addr);

        self.health_state.mark_started();
        self.running.store(true, Ordering::SeqCst);

        let config = self.config.clone();
        let router = self.router.clone();
        let handlers = self.handlers.clone();
        let global_middleware = self.global_middleware.clone();
        let health_state = self.health_state.clone();
        let metrics = self.metrics.clone();
        let running = self.running.clone();
        let _notify = self.notify.clone();
        let static_files = self.static_files.clone();

        // Handle TLS if configured
        #[cfg(feature = "tls")]
        let _tls_config = if let (Some(ref cert), Some(ref key)) =
            (config.tls_cert.as_ref(), config.tls_key.as_ref())
        {
            match crate::tls::load_tls_config(std::path::Path::new(cert), std::path::Path::new(key))
            {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    tracing::error!("TLS config error: {}", e);
                    None
                }
            }
        } else {
            None
        };

        #[cfg(not(feature = "tls"))]
        let tls_config: Option<Arc<rustls::ServerConfig>> = None;

        let graceful_timeout = Duration::from_secs(config.graceful_timeout_s);

        // Accept loop
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            let router = router.clone();
                            let handlers = handlers.clone();
                            let global_middleware = global_middleware.clone();
                            let health_state = health_state.clone();
                            let metrics = metrics.clone();
                            let static_files = static_files.clone();

                            tokio::spawn(async move {
                                handle_connection(
                                    stream,
                                    peer_addr,
                                    router,
                                    handlers,
                                    global_middleware,
                                    health_state,
                                    metrics,
                                    static_files,
                                ).await;
                            });
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                            if !running.load(Ordering::SeqCst) {
                                break;
                            }
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received shutdown signal");
                    break;
                }
            }
        }

        // Graceful shutdown
        self.health_state.mark_shutting_down();
        self.running.store(false, Ordering::SeqCst);
        tracing::info!("Shutting down, waiting for in-flight requests...");

        let shutdown_start = Instant::now();
        loop {
            if shutdown_start.elapsed() >= graceful_timeout {
                tracing::warn!("Graceful timeout reached, forcing shutdown");
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        tracing::info!("Server stopped");
        Ok(())
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    router: Arc<Router>,
    handlers: Arc<HashMap<usize, HandlerFn>>,
    global_middleware: Vec<Arc<dyn Middleware>>,
    health_state: Arc<HealthState>,
    metrics: Arc<Metrics>,
    static_files: Option<StaticFiles>,
) {
    let mut buf = Vec::with_capacity(8192);
    let mut temp = [0u8; 4096];

    // Read request with timeout
    loop {
        match tokio::time::timeout(Duration::from_secs(30), stream.read(&mut temp)).await {
            Ok(Ok(0)) => return,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&temp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Ok(Err(_)) | Err(_) => return,
        }
    }

    // Parse request
    let mut request = match parse_request(&buf) {
        Ok((req, _)) => req,
        Err(_) => {
            let mut resp = Response::bad_request("Bad Request");
            let _ = write_response(&mut stream, &mut resp, &request_id(), &[]).await;
            return;
        }
    };

    // Set client IP
    request
        .extensions
        .insert("client_ip".into(), peer_addr.ip().to_string());

    let request_id = request.request_id.clone();

    // Health endpoints
    if request.path == "/healthz" {
        let resp = health_state.healthz(&request);
        let _ = write_response(&mut stream, &mut resp.clone(), &request_id, &[]).await;
        return;
    }

    if request.path == "/readyz" {
        let resp = health_state.readyz(&request);
        let _ = write_response(&mut stream, &mut resp.clone(), &request_id, &[]).await;
        return;
    }

    // Metrics endpoint
    let metrics_endpoint = "/metrics";
    if request.path == metrics_endpoint {
        let output = metrics.render();
        let mut resp = Response::new();
        resp.set_text(&output);
        resp.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        let _ = write_response(&mut stream, &mut resp, &request_id, &[]).await;
        return;
    }

    // Run global middleware (before)
    for mw in &global_middleware {
        if let Some(resp) = mw.before(&mut request) {
            let _ = write_response(&mut stream, &mut resp.clone(), &request_id, &[]).await;
            return;
        }
    }

    // Route
    let route_result = router.lookup(&request.method, &request.path);

    let mut response = if let Some((route_idx, params, route_middleware)) = route_result {
        request.params = params;

        // Run route-specific middleware (before)
        let mut early_response = None;
        for mw in &route_middleware {
            if let Some(resp) = mw.before(&mut request) {
                early_response = Some(resp);
                break;
            }
        }

        if let Some(resp) = early_response {
            resp
        } else if request.method == Method::Options {
            let methods = router.route_methods(route_idx);
            let allowed: Vec<&str> = methods.iter().map(|m| m.as_str()).collect();
            Response::method_not_allowed(&allowed.join(", "))
        } else if let Some(handler) = handlers.get(&route_idx) {
            let mut resp = (handler)(request.clone()).await;
            // Run global middleware (after)
            for mw in &global_middleware {
                mw.after(&request, &mut resp);
            }
            resp
        } else {
            Response::internal_error("Handler not found")
        }
    } else {
        // Check static files
        if let Some(ref sf) = static_files {
            if let Some(resp) = sf.handle(&request, &request.path) {
                resp
            } else {
                Response::not_found()
            }
        } else {
            Response::not_found()
        }
    };

    // Handle HEAD: suppress body
    if request.method == Method::Head {
        response.body.clear();
    }

    // Add X-Request-Id header
    let _ = write_response(&mut stream, &mut response, &request_id, &global_middleware).await;
}

fn request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn write_response(
    stream: &mut TcpStream,
    response: &mut Response,
    request_id: &str,
    _global_middleware: &[Arc<dyn Middleware>],
) -> Result<(), std::io::Error> {
    response
        .headers
        .insert("x-request-id".into(), request_id.to_string());

    let status_line = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        response.status.canonical_reason()
    );

    let mut header_str = String::new();
    header_str.push_str(&status_line);
    for (name, value) in response.headers.iter() {
        header_str.push_str(&format!("{}: {}\r\n", name, value));
    }
    header_str.push_str("\r\n");

    stream.write_all(header_str.as_bytes()).await?;
    if !response.body.is_empty() {
        stream.write_all(&response.body).await?;
    }
    stream.flush().await?;
    Ok(())
}
