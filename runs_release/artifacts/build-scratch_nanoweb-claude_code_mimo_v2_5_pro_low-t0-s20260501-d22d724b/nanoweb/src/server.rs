use crate::config::Config;
use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::{Method, Request, Response, StatusCodeClass};
use crate::metrics::Metrics;
use crate::middleware;
use crate::router::Router;
use crate::session::InMemorySessionStore;
use crate::static_files::StaticFileServer;
use crate::template::Renderer;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

pub struct Server {
    pub config: Config,
    pub router: Router,
    pub metrics: Arc<Metrics>,
    pub health: HealthState,
    pub session_store: Arc<InMemorySessionStore>,
    pub static_server: Option<StaticFileServer>,
    pub renderer: Renderer,
}

impl Server {
    pub fn new(config: Config) -> Self {
        let session_secret = config.session_secret.clone();
        Self {
            config,
            router: Router::new(),
            metrics: Arc::new(Metrics::new()),
            health: HealthState::new(),
            session_store: Arc::new(InMemorySessionStore::new(session_secret)),
            static_server: None,
            renderer: Renderer::new(),
        }
    }

    pub fn router_mut(&mut self) -> &mut Router {
        &mut self.router
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn session_store(&self) -> Arc<InMemorySessionStore> {
        self.session_store.clone()
    }

    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    pub fn health(&self) -> &HealthState {
        &self.health
    }

    pub fn set_static_dir(&mut self, dir: impl Into<std::path::PathBuf>) {
        self.static_server = Some(StaticFileServer::new(dir));
    }

    pub async fn run(self) -> Result<(), NanowebError> {
        let addr: SocketAddr = self
            .config
            .listen
            .parse()
            .map_err(|e| NanowebError::Config(format!("Invalid listen address: {e}")))?;

        let listener = TcpListener::bind(addr).await?;
        tracing::info!("Listening on {}", addr);

        let accepting = Arc::new(AtomicBool::new(true));
        let shutdown = Arc::new(Notify::new());

        self.health.set_ready(true);

        let metrics_path = self
            .config
            .metrics_path
            .clone()
            .unwrap_or_else(|| "/metrics".to_string());
        let healthz_path = self
            .config
            .healthz_path
            .clone()
            .unwrap_or_else(|| "/healthz".to_string());
        let readyz_path = self
            .config
            .readyz_path
            .clone()
            .unwrap_or_else(|| "/readyz".to_string());
        let graceful_timeout = self.config.graceful_timeout_s;

        let router = Arc::new(self.router);
        let metrics = self.metrics.clone();
        let health = self.health.clone();
        let static_server = self.static_server.map(Arc::new);

        let accept_shutdown = shutdown.clone();
        let accept_accepting = accepting.clone();

        let accept_handle = tokio::spawn(async move {
            loop {
                if !accept_accepting.load(Ordering::SeqCst) {
                    break;
                }

                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                metrics.connection_opened();
                                let router = router.clone();
                                let metrics_inner = metrics.clone();
                                let health = health.clone();
                                let static_server = static_server.clone();
                                let mp = metrics_path.clone();
                                let hp = healthz_path.clone();
                                let rp = readyz_path.clone();

                                tokio::spawn(async move {
                                    let start = Instant::now();
                                    if let Err(e) = handle_connection(
                                        stream, addr, router, metrics_inner.clone(), &health,
                                        static_server.as_deref(), &mp, &hp, &rp,
                                    ).await {
                                        tracing::error!("Connection error from {}: {}", addr, e);
                                    }
                                    metrics_inner.record_duration(start.elapsed());
                                    metrics_inner.connection_closed();
                                });
                            }
                            Err(e) => {
                                tracing::error!("Accept error: {}", e);
                            }
                        }
                    }
                    _ = accept_shutdown.notified() => {
                        tracing::info!("Shutdown signal received");
                        accept_accepting.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });

        // Wait for SIGINT/SIGTERM
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutting down...");
        self.health.set_ready(false);
        accepting.store(false, Ordering::SeqCst);
        shutdown.notify_one();

        let timeout = tokio::time::timeout(
            std::time::Duration::from_secs(graceful_timeout),
            accept_handle,
        );

        match timeout.await {
            Ok(_) => tracing::info!("Server shut down gracefully"),
            Err(_) => tracing::warn!("Graceful timeout reached, forcing shutdown"),
        }

        Ok(())
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    router: Arc<Router>,
    metrics: Arc<Metrics>,
    health: &HealthState,
    static_server: Option<&StaticFileServer>,
    metrics_path: &str,
    healthz_path: &str,
    readyz_path: &str,
) -> Result<(), NanowebError> {
    let mut buf = vec![0u8; 8192];
    let mut parser = crate::http::HttpParser::new();

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }

        parser.feed(&buf[..n]);

        loop {
            match parser.parse() {
                Ok(Some(mut req)) => {
                    req.remote_addr = Some(addr);
                    req.request_id = uuid::Uuid::new_v4().to_string();

                    let resp = route_request(
                        &mut req,
                        &router,
                        metrics.clone(),
                        health,
                        static_server,
                        metrics_path,
                        healthz_path,
                        readyz_path,
                    )
                    .await;

                    let status_class = StatusCodeClass::from_status(resp.status);
                    metrics.record_request(req.method.as_str(), status_class.as_str(), &req.path);

                    write_response(&mut stream, &resp, req.method == Method::Head).await?;

                    let keep_alive = req
                        .header("connection")
                        .map(|v| v.to_ascii_lowercase() != "close")
                        .unwrap_or(true);

                    if !keep_alive {
                        return Ok(());
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("Parse error from {}: {}", addr, e);
                    let resp = Response::new(400).with_body("Bad Request");
                    write_response(&mut stream, &resp, false).await?;
                    return Ok(());
                }
            }
        }
    }
}

async fn route_request(
    req: &mut Request,
    router: &Router,
    metrics: Arc<Metrics>,
    health: &HealthState,
    static_server: Option<&StaticFileServer>,
    metrics_path: &str,
    healthz_path: &str,
    readyz_path: &str,
) -> Response {
    if req.path == metrics_path {
        return Response::ok()
            .with_header("content-type", "text/plain; version=0.0.4; charset=utf-8")
            .with_body(metrics.render_prometheus());
    }

    if req.path == healthz_path {
        return health.healthz();
    }

    if req.path == readyz_path {
        return health.readyz();
    }

    if req.method == Method::Options {
        let methods = router.allowed_methods(&req.path);
        if !methods.is_empty() {
            let allowed: Vec<&str> = methods.iter().map(|m| m.as_str()).collect();
            return Response::new(204).with_header("allow", &allowed.join(", "));
        }
    }

    if let Some((route, params)) = router.lookup(req.method, &req.path) {
        req.params = params;

        if let Some(resp) = middleware::run_middleware_before(&route.middleware, req).await {
            return resp;
        }

        let handler = route.handler.clone();
        let mut resp = (handler)(req.clone()).await;

        middleware::run_middleware_after(&route.middleware, req, &mut resp);

        return resp;
    }

    if let Some(static_server) = static_server {
        match static_server.serve(req).await {
            Ok(resp) if resp.status != 404 => return resp,
            _ => {}
        }
    }

    Response::new(404).with_body("Not Found")
}

async fn write_response(
    stream: &mut TcpStream,
    resp: &Response,
    head_request: bool,
) -> Result<(), NanowebError> {
    let mut header = format!("HTTP/1.1 {} {}\r\n", resp.status, resp.status_text());

    let has_content_length = resp
        .headers
        .iter()
        .any(|(k, _)| k.to_ascii_lowercase() == "content-length");
    if !has_content_length && !head_request {
        header.push_str(&format!("content-length: {}\r\n", resp.body.len()));
    }
    if !resp
        .headers
        .iter()
        .any(|(k, _)| k.to_ascii_lowercase() == "connection")
    {
        header.push_str("connection: keep-alive\r\n");
    }

    for (name, value) in &resp.headers {
        header.push_str(&format!("{name}: {value}\r\n"));
    }
    header.push_str("\r\n");

    stream.write_all(header.as_bytes()).await?;
    if !head_request {
        stream.write_all(&resp.body).await?;
    }
    stream.flush().await?;

    Ok(())
}
