use crate::error::{NanowebError, Result};
use crate::health::Health;
use crate::http::parser::{read_body, read_request_head, split_target};
use crate::http::{Method, Request, Response, StatusCode, Version};
use crate::metrics::Metrics;
use crate::middleware::Stack;
use crate::router::{RouteResolution, Router};
use crate::session::SessionManager;
use crate::static_files::StaticFiles;
use crate::template::Renderer;
use rand::RngCore;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinSet;

/// Root application: bundles router, middleware, metrics, health, sessions,
/// static files, and the template engine.
pub struct App {
    pub router: Router,
    pub middleware: Stack,
    pub metrics: Metrics,
    pub health: Health,
    pub session: Option<Arc<SessionManager>>,
    pub static_files: Option<Arc<StaticFiles>>,
    pub static_prefix: String,
    pub renderer: Arc<Renderer>,
    pub metrics_path: String,
    pub graceful_timeout: Duration,
    #[cfg(feature = "tls")]
    pub tls: Option<(SocketAddr, tokio_rustls::TlsAcceptor)>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            router: Router::new(),
            middleware: Stack::new(),
            metrics: Metrics::new(),
            health: Health::new(),
            session: None,
            static_files: None,
            static_prefix: "/static/".into(),
            renderer: Arc::new(Renderer::new()),
            metrics_path: "/metrics".into(),
            graceful_timeout: Duration::from_secs(30),
            #[cfg(feature = "tls")]
            tls: None,
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }
}

/// HTTP/1.1 server. Runs an accept loop on one or more listeners and dispatches
/// each connection through the configured app.
pub struct Server {
    pub app: Arc<App>,
}

impl Server {
    pub fn new(app: App) -> Self {
        Self { app: Arc::new(app) }
    }

    /// Bind to `addr` plus the optional TLS listener configured on the app, then run until shutdown is signalled.
    pub async fn run(self, addr: SocketAddr) -> Result<()> {
        let app = self.app.clone();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();
        // Signal handler
        tokio::spawn(async move {
            let mut sigterm =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
            tracing::info!("shutdown signal received");
            shutdown_clone.notify_waiters();
        });

        let mut tasks: JoinSet<()> = JoinSet::new();

        // Plain HTTP listener
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("listening on http://{}", addr);
        app.health.set_ready(true);

        let plain_app = app.clone();
        let plain_shutdown = shutdown.clone();
        tasks.spawn(async move {
            run_accept_loop(listener, plain_app, plain_shutdown, false, None).await;
        });

        #[cfg(feature = "tls")]
        if let Some((tls_addr, acceptor)) = app.tls.clone() {
            let tls_listener = TcpListener::bind(tls_addr).await?;
            tracing::info!("listening on https://{}", tls_addr);
            let tls_app = app.clone();
            let tls_shutdown = shutdown.clone();
            tasks.spawn(async move {
                run_accept_loop(tls_listener, tls_app, tls_shutdown, true, Some(acceptor)).await;
            });
        }

        // Wait for shutdown
        shutdown.notified().await;
        app.health.shutdown();
        let timeout = app.graceful_timeout;
        tracing::info!("graceful shutdown, draining for up to {:?}", timeout);
        let drain = async { while tasks.join_next().await.is_some() {} };
        let _ = tokio::time::timeout(timeout, drain).await;
        Ok(())
    }
}

#[cfg(not(feature = "tls"))]
type TlsAcceptorOpt = Option<()>;
#[cfg(feature = "tls")]
type TlsAcceptorOpt = Option<tokio_rustls::TlsAcceptor>;

async fn run_accept_loop(
    listener: TcpListener,
    app: Arc<App>,
    shutdown: Arc<Notify>,
    _is_tls: bool,
    acceptor: TlsAcceptorOpt,
) {
    let mut conns: JoinSet<()> = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                tracing::info!("accept loop stopping");
                break;
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, peer)) => {
                        let app = app.clone();
                        let acceptor_clone = acceptor.clone();
                        conns.spawn(async move {
                            if let Err(e) = handle_connection(stream, peer, app, acceptor_clone).await {
                                tracing::debug!("conn error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("accept error: {}", e);
                    }
                }
            }
        }
    }
    while conns.join_next().await.is_some() {}
}

async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    app: Arc<App>,
    acceptor: TlsAcceptorOpt,
) -> Result<()> {
    #[cfg(feature = "tls")]
    {
        if let Some(acc) = acceptor {
            let tls_stream = acc
                .accept(stream)
                .await
                .map_err(|e| NanowebError::Tls(e.to_string()))?;
            return handle_io(tls_stream, peer, app).await;
        }
    }
    #[cfg(not(feature = "tls"))]
    let _ = acceptor;
    handle_io(stream, peer, app).await
}

async fn handle_io<S>(stream: S, peer: SocketAddr, app: Arc<App>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (rd, mut wr) = tokio::io::split(stream);
    let mut reader = BufReader::new(rd);
    loop {
        // Peek a byte to detect close
        let head = match read_request_head(&mut reader).await {
            Ok(h) => h,
            Err(NanowebError::BadRequest(msg)) => {
                let mut r = Response::new(StatusCode::BAD_REQUEST);
                r.body = msg.into_bytes();
                let _ = wr.write_all(&r.serialize(false)).await;
                return Ok(());
            }
            Err(NanowebError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(());
            }
            Err(_) => return Ok(()),
        };
        let body = read_body(&mut reader, &head.headers).await?;
        let (path, query) = split_target(&head.target);
        let request_id = generate_request_id();
        let mut req = Request {
            method: head.method,
            target: head.target.clone(),
            path,
            query,
            version: head.version,
            headers: head.headers,
            body,
            params: Default::default(),
            remote_addr: Some(peer),
            request_id: request_id.clone(),
            extensions: Default::default(),
        };

        let keep_alive = should_keep_alive(&req);
        let started = Instant::now();
        let mut route_label = req.path.clone();

        let (mut response, suppress_body) = dispatch(&app, &mut req, &mut route_label).await;
        // Always include X-Request-Id
        response.headers.insert("X-Request-Id", request_id);
        if !keep_alive {
            response.headers.insert("Connection", "close");
        } else {
            response.headers.insert("Connection", "keep-alive");
        }
        // Record metric
        app.metrics.record_request(
            req.method,
            response.status.0,
            &route_label,
            started.elapsed(),
        );

        let bytes = response.serialize(suppress_body);
        if let Err(e) = wr.write_all(&bytes).await {
            tracing::debug!("write error: {}", e);
            return Ok(());
        }
        if !keep_alive {
            let _ = wr.shutdown().await;
            return Ok(());
        }
    }
}

fn should_keep_alive(req: &Request) -> bool {
    let conn = req.header("Connection").unwrap_or("").to_ascii_lowercase();
    if req.version == Version::Http11 {
        !conn.contains("close")
    } else {
        conn.contains("keep-alive")
    }
}

fn generate_request_id() -> String {
    let mut buf = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

async fn dispatch(app: &App, req: &mut Request, route_label: &mut String) -> (Response, bool) {
    // Run middleware "before"
    for mw in &app.middleware.items {
        if let Some(resp) = mw.before(req).await {
            // Walk back through after()
            return run_after(app, req, resp, false).await;
        }
    }

    // Built-in endpoints
    if req.path == app.metrics_path && req.method == Method::Get {
        *route_label = app.metrics_path.clone();
        let resp = app.metrics.response();
        return run_after(app, req, resp, false).await;
    }
    if req.path == "/healthz" {
        *route_label = "/healthz".into();
        let resp = app.health.healthz();
        let suppress = req.method == Method::Head;
        return run_after(app, req, resp, suppress).await;
    }
    if req.path == "/readyz" {
        *route_label = "/readyz".into();
        let resp = app.health.readyz();
        let suppress = req.method == Method::Head;
        return run_after(app, req, resp, suppress).await;
    }
    // Static files
    if let Some(sf) = &app.static_files {
        if req.path.starts_with(&app.static_prefix) {
            let rel = &req.path[app.static_prefix.len()..];
            *route_label = format!("{}*", app.static_prefix);
            let resp = sf.serve(req, rel).await;
            let suppress = req.method == Method::Head;
            return run_after(app, req, resp, suppress).await;
        }
    }

    // Router
    let resolution = app.router.resolve(req.method, &req.path);
    let (resp, suppress) = match resolution {
        RouteResolution::Found(m) => {
            req.params = m.params;
            let r = m.handler.call(req.clone()).await;
            (r, m.head_fallback)
        }
        RouteResolution::MethodNotAllowed { allowed } => {
            if req.method == Method::Options {
                let allowed = app.router.allowed_methods(&req.path);
                (crate::router::build_options_response(&allowed), false)
            } else {
                let mut s: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
                s.sort();
                let mut r = Response::new(StatusCode::METHOD_NOT_ALLOWED);
                r.headers.insert("Allow", s.join(", "));
                r.body = b"method not allowed".to_vec();
                (r, false)
            }
        }
        RouteResolution::NotFound => {
            let mut r = Response::new(StatusCode::NOT_FOUND);
            r.headers.insert("Content-Type", "text/plain");
            r.body = b"not found".to_vec();
            (r, false)
        }
    };

    run_after(app, req, resp, suppress).await
}

async fn run_after(
    app: &App,
    req: &Request,
    mut resp: Response,
    suppress: bool,
) -> (Response, bool) {
    // After hooks run in reverse registration order so outermost is last seen.
    for mw in app.middleware.items.iter().rev() {
        mw.after(req, &mut resp).await;
    }
    (resp, suppress)
}

// Re-exports
pub use crate::router::handler;
