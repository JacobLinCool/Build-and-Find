//! HTTP/1.1 server runtime: accept loop, per-connection handling, dispatch,
//! and graceful shutdown.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::body::parse_query;
use crate::config::{Config, RouteResponse};
use crate::error::{NanowebError, Result};
use crate::health::HealthState;
use crate::http::parser::{body_mode, decode_chunked, parse_request_head, BodyMode};
use crate::http::{Method, Request, Response, StatusCode};
use crate::log;
use crate::metrics::Metrics;
use crate::middleware::{
    basic_auth::BasicAuthMiddleware, compression::GzipMiddleware, cors::CorsMiddleware,
    csrf::CsrfMiddleware, logging::LoggingMiddleware, rate_limit::RateLimitMiddleware, HandlerFn,
    MiddlewareList,
};
use crate::router::{BoxFuture as RouterBoxFuture, Handler, Router};
use crate::session::{InMemoryStore, SessionManager, SessionStore};
use crate::static_files::StaticFiles;
use crate::template::Renderer;

/// The fully-built application: router, middleware, shared state.
pub struct App {
    pub router: Router,
    pub middleware: MiddlewareList,
    pub metrics: Arc<Metrics>,
    pub health: Arc<HealthState>,
    pub session: Arc<SessionManager>,
    pub renderer: Arc<Renderer>,
    pub static_files: Option<Arc<StaticFiles>>,
    pub metrics_path: String,
    pub graceful_timeout: Duration,
}

impl App {
    pub fn from_config(cfg: &Config) -> Self {
        let router = Router::new();
        let metrics = Arc::new(Metrics::new());
        let health = Arc::new(HealthState::new());
        let session_store: Arc<dyn SessionStore> = Arc::new(InMemoryStore::new());
        let session = Arc::new(SessionManager::new(
            cfg.session_secret.as_bytes().to_vec(),
            session_store,
        ));
        let renderer = Arc::new(Renderer::new());
        let static_files = cfg
            .static_dir
            .as_ref()
            .map(|p| Arc::new(StaticFiles::new(p.clone())));

        // Configured routes.
        for route in &cfg.routes {
            register_route(&router, route, &renderer, &static_files);
        }

        // Static directory route under /static/*path when --static-dir is set
        // and no user-defined route already serves it.
        if let Some(sf) = &static_files {
            let sf_clone = sf.clone();
            let handler: Handler = Arc::new(move |req: &mut Request| {
                let sf = sf_clone.clone();
                let path = req.params.get("path").cloned().unwrap_or_default();
                let req_clone = req.clone();
                Box::pin(async move { sf.serve(&req_clone, &path).await }) as RouterBoxFuture<'_, _>
            });
            router.route(Method::Get, "/static/*path", handler);
        }

        // Metrics endpoint.
        let metrics_clone = metrics.clone();
        router.route(
            Method::Get,
            &cfg.metrics_path,
            Arc::new(move |_req: &mut Request| {
                let m = metrics_clone.clone();
                Box::pin(async move {
                    Ok(Response::ok()
                        .header("content-type", "text/plain; version=0.0.4")
                        .text(m.render()))
                })
            }),
        );

        // Health endpoints.
        let health_live = health.clone();
        router.route(
            Method::Get,
            "/healthz",
            Arc::new(move |_req: &mut Request| {
                let h = health_live.clone();
                Box::pin(async move {
                    if h.is_live() {
                        Ok(Response::ok().text("ok"))
                    } else {
                        Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR).text("unhealthy"))
                    }
                })
            }),
        );
        let health_ready = health.clone();
        router.route(
            Method::Get,
            "/readyz",
            Arc::new(move |_req: &mut Request| {
                let h = health_ready.clone();
                Box::pin(async move {
                    if h.is_ready() {
                        Ok(Response::ok().text("ready"))
                    } else {
                        Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR).text("not ready"))
                    }
                })
            }),
        );

        // Build middleware list.
        let mut mw: MiddlewareList = Vec::new();
        if cfg.middleware.logging {
            mw.push(Arc::new(LoggingMiddleware::new()));
        }
        if let Some(c) = &cfg.middleware.cors {
            mw.push(Arc::new(CorsMiddleware::new(c.allowed_origins.clone())));
        }
        if let Some(rl) = &cfg.middleware.rate_limit {
            mw.push(Arc::new(RateLimitMiddleware::new(
                rl.capacity,
                rl.refill_per_sec,
            )));
        }
        if cfg.middleware.csrf {
            mw.push(Arc::new(CsrfMiddleware::new()));
        }
        if let Some(ba) = &cfg.middleware.basic_auth {
            mw.push(Arc::new(BasicAuthMiddleware::new(
                ba.realm.clone(),
                ba.credentials.clone(),
            )));
        }
        if cfg.middleware.gzip {
            mw.push(Arc::new(GzipMiddleware::new()));
        }

        Self {
            router,
            middleware: mw,
            metrics,
            health,
            session,
            renderer,
            static_files,
            metrics_path: cfg.metrics_path.clone(),
            graceful_timeout: Duration::from_secs(cfg.graceful_timeout_s),
        }
    }
}

fn register_route(
    router: &Router,
    route: &crate::config::RouteConfig,
    renderer: &Arc<Renderer>,
    static_files: &Option<Arc<StaticFiles>>,
) {
    let method = Method::parse(&route.method);
    let pattern = route.path.clone();
    let response = route.response.clone();
    let renderer = renderer.clone();
    let static_files = static_files.clone();
    let handler: Handler = Arc::new(move |req: &mut Request| {
        let response = response.clone();
        let renderer = renderer.clone();
        let static_files = static_files.clone();
        let req_clone = req.clone();
        Box::pin(async move {
            match response {
                RouteResponse::Text(s) => Ok(Response::ok().text(s)),
                RouteResponse::Json(v) => Response::ok().json(&v),
                RouteResponse::Html(s) => Ok(Response::ok().html(s)),
                RouteResponse::Template { template, context } => {
                    let body = renderer.render(&template, &context)?;
                    Ok(Response::ok().html(body))
                }
                RouteResponse::StaticDir => match static_files {
                    Some(sf) => {
                        let path = req_clone.params.get("path").cloned().unwrap_or_default();
                        sf.serve(&req_clone, &path).await
                    }
                    None => Ok(Response::new(StatusCode::INTERNAL_SERVER_ERROR)
                        .text("static dir not configured")),
                },
            }
        })
    });
    router.route(method, &pattern, handler);
}

/// Serve a single plain-HTTP listener until `shutdown` is notified. Useful in
/// tests where the caller already owns a bound `TcpListener`.
pub async fn serve(listener: TcpListener, app: Arc<App>, shutdown: Arc<Notify>) -> Result<()> {
    app.health.set_ready(true);
    accept_loop_plain(listener, app, shutdown).await
}

/// Run the HTTP and (optionally) HTTPS listeners until shutdown is requested.
pub async fn run(cfg: Config, app: Arc<App>) -> Result<()> {
    log::info(&format!("starting nanoweb on {}", cfg.listen));
    app.health.set_ready(true);

    let shutdown = Arc::new(Notify::new());
    let shutdown_signal = shutdown.clone();
    let signal_task = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        log::info("shutdown signal received");
        shutdown_signal.notify_waiters();
    });

    let mut tasks: Vec<JoinHandle<Result<()>>> = Vec::new();

    let plain_addr: SocketAddr = cfg
        .listen
        .parse()
        .map_err(|e| NanowebError::Config(format!("bad listen address {}: {}", cfg.listen, e)))?;
    let plain_listener = TcpListener::bind(plain_addr).await?;
    log::info(&format!("plain HTTP listening on {}", plain_addr));
    let app_plain = app.clone();
    let shutdown_plain = shutdown.clone();
    tasks.push(tokio::spawn(async move {
        accept_loop_plain(plain_listener, app_plain, shutdown_plain).await
    }));

    if let (Some(cert), Some(key)) = (&cfg.tls_cert, &cfg.tls_key) {
        spawn_tls_listener(
            &app,
            cert.clone(),
            key.clone(),
            shutdown.clone(),
            &mut tasks,
        )?;
    } else if cfg.tls_cert.is_some() || cfg.tls_key.is_some() {
        return Err(NanowebError::Tls(
            "both --tls-cert and --tls-key must be supplied for HTTPS".into(),
        ));
    }

    for t in tasks {
        match t.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::error(&format!("listener error: {}", e)),
            Err(e) => log::error(&format!("listener join error: {}", e)),
        }
    }
    signal_task.abort();
    app.health.set_ready(false);
    Ok(())
}

#[cfg(feature = "tls")]
fn spawn_tls_listener(
    app: &Arc<App>,
    cert: PathBuf,
    key: PathBuf,
    shutdown: Arc<Notify>,
    tasks: &mut Vec<JoinHandle<Result<()>>>,
) -> Result<()> {
    use tokio_rustls::TlsAcceptor;
    let server_config = crate::tls::load_server_config(&cert, &key)?;
    let acceptor = TlsAcceptor::from(server_config);
    let app_tls = app.clone();
    tasks.push(tokio::spawn(async move {
        let addr: SocketAddr = "0.0.0.0:8443".parse().unwrap();
        let listener = TcpListener::bind(addr).await?;
        log::info(&format!("TLS listening on {}", addr));
        accept_loop_tls(listener, acceptor, app_tls, shutdown).await
    }));
    Ok(())
}

#[cfg(not(feature = "tls"))]
fn spawn_tls_listener(
    _app: &Arc<App>,
    _cert: PathBuf,
    _key: PathBuf,
    _shutdown: Arc<Notify>,
    _tasks: &mut Vec<JoinHandle<Result<()>>>,
) -> Result<()> {
    crate::tls::fail_disabled()
}

async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = sigterm.recv() => {},
        _ = sigint.recv() => {},
    }
}

/// The accept loop is intentionally split from per-connection handling so that
/// graceful shutdown can be reasoned about locally.
async fn accept_loop_plain(
    listener: TcpListener,
    app: Arc<App>,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let active = Arc::new(tokio::sync::Semaphore::new(0)); // tracks in-flight connections
    let active_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let shutdown_inner = shutdown.clone();
    loop {
        tokio::select! {
            _ = shutdown_inner.notified() => {
                log::info("plain listener stopping accept loop");
                drop(listener);
                break;
            }
            res = listener.accept() => {
                let (stream, peer) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        log::error(&format!("accept error: {}", e));
                        continue;
                    }
                };
                let app = app.clone();
                let active_count = active_count.clone();
                active_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, peer, app).await {
                        log::debug(&format!("connection ended: {}", e));
                    }
                    active_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
            }
        }
    }
    drop(active);
    // Wait for in-flight connections to complete (bounded by graceful timeout).
    let deadline = Instant::now() + app.graceful_timeout;
    while active_count.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        if Instant::now() >= deadline {
            log::warn("graceful shutdown deadline reached; aborting in-flight connections");
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}

#[cfg(feature = "tls")]
async fn accept_loop_tls(
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    app: Arc<App>,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let active_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let shutdown_inner = shutdown.clone();
    loop {
        tokio::select! {
            _ = shutdown_inner.notified() => {
                drop(listener);
                break;
            }
            res = listener.accept() => {
                let (stream, peer) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        log::error(&format!("accept error: {}", e));
                        continue;
                    }
                };
                let app = app.clone();
                let acceptor = acceptor.clone();
                let active_count = active_count.clone();
                active_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            if let Err(e) = handle_connection(tls_stream, peer, app).await {
                                log::debug(&format!("connection ended: {}", e));
                            }
                        }
                        Err(e) => log::error(&format!("tls handshake: {}", e)),
                    }
                    active_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
            }
        }
    }
    Ok(())
}

async fn handle_connection<S>(mut stream: S, peer: SocketAddr, app: Arc<App>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut tmp = [0u8; 8 * 1024];

    loop {
        // Read request head into buffer.
        let head_used;
        let request_partial = loop {
            // Try to find an end-of-head in current buf.
            if let Some(end) = find_double_crlf(&buf) {
                match parse_request_head(&buf[..end + 4]) {
                    Ok((req, used)) => {
                        head_used = used;
                        break req;
                    }
                    Err(e) => {
                        let resp = Response::new(StatusCode::BAD_REQUEST)
                            .text(format!("{}", e))
                            .header("connection", "close");
                        stream.write_all(&resp.to_bytes(false)).await?;
                        stream.flush().await?;
                        return Ok(());
                    }
                }
            }
            if buf.len() > crate::http::parser::MAX_HEAD_SIZE {
                let resp = Response::new(StatusCode::BAD_REQUEST).text("head too large");
                stream.write_all(&resp.to_bytes(false)).await?;
                stream.flush().await?;
                return Ok(());
            }
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&tmp[..n]);
        };

        let mut request = request_partial;
        request.remote_ip = peer.ip().to_string();
        request.request_id = uuid::Uuid::new_v4().to_string();

        // Read body.
        let mode = match body_mode(&request) {
            Ok(m) => m,
            Err(e) => {
                let resp = Response::new(StatusCode::BAD_REQUEST).text(format!("{}", e));
                stream.write_all(&resp.to_bytes(false)).await?;
                stream.flush().await?;
                return Ok(());
            }
        };
        let body_bytes = read_body(&mut stream, &mut buf, head_used, mode).await;
        let body_bytes = match body_bytes {
            Ok(b) => b,
            Err(e) => {
                let resp = Response::new(StatusCode::BAD_REQUEST).text(format!("{}", e));
                stream.write_all(&resp.to_bytes(false)).await?;
                stream.flush().await?;
                return Ok(());
            }
        };
        request.body = body_bytes.body;
        // Drain consumed bytes.
        buf.drain(..body_bytes.consumed);

        let keep_alive = should_keep_alive(&request);

        let response = dispatch(&app, &mut request).await;

        let mut response = match response {
            Ok(r) => r,
            Err(e) => err_to_response(e),
        };

        // X-Request-Id header.
        response
            .headers
            .insert("x-request-id", request.request_id.clone());

        // Connection header.
        if keep_alive {
            response.headers.insert("connection", "keep-alive");
        } else {
            response.headers.insert("connection", "close");
        }

        let suppress_body = request.method == Method::Head;
        let bytes = response.to_bytes(suppress_body);
        stream.write_all(&bytes).await?;
        stream.flush().await?;

        if !keep_alive {
            return Ok(());
        }
    }
}

struct ReadBody {
    body: Vec<u8>,
    /// Number of bytes consumed from the connection buffer (head + body bytes
    /// for length-prefixed; for chunked, head + raw chunked frame bytes).
    consumed: usize,
}

async fn read_body<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut Vec<u8>,
    head_used: usize,
    mode: BodyMode,
) -> Result<ReadBody> {
    let mut tmp = [0u8; 8 * 1024];
    match mode {
        BodyMode::None => Ok(ReadBody {
            body: Vec::new(),
            consumed: head_used,
        }),
        BodyMode::Length(n) => {
            while buf.len() < head_used + n {
                let r = stream.read(&mut tmp).await?;
                if r == 0 {
                    return Err(NanowebError::Parse("unexpected eof in body".into()));
                }
                buf.extend_from_slice(&tmp[..r]);
            }
            let body = buf[head_used..head_used + n].to_vec();
            Ok(ReadBody {
                body,
                consumed: head_used + n,
            })
        }
        BodyMode::Chunked => {
            // Read until we can decode a complete chunked body.
            loop {
                let slice = &buf[head_used..];
                if let Some(end) = find_chunked_end(slice) {
                    let raw = &slice[..end];
                    let body = decode_chunked(raw)?;
                    return Ok(ReadBody {
                        body,
                        consumed: head_used + end,
                    });
                }
                if buf.len()
                    > crate::http::parser::MAX_HEAD_SIZE + crate::http::parser::MAX_BODY_SIZE
                {
                    return Err(NanowebError::Parse("chunked body too large".into()));
                }
                let r = stream.read(&mut tmp).await?;
                if r == 0 {
                    return Err(NanowebError::Parse("unexpected eof in chunked body".into()));
                }
                buf.extend_from_slice(&tmp[..r]);
            }
        }
    }
}

/// Detect the end of a chunked body within `buf`. Returns the byte offset just
/// past the terminating CRLF after the trailer section (or just past the
/// final empty CRLF if no trailers).
fn find_chunked_end(buf: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < buf.len() {
        // Each chunk: hex size [\r\n] body [\r\n].
        let line_end = find_crlf_at(buf, i)?;
        let line = std::str::from_utf8(&buf[i..line_end]).ok()?;
        let size_str = line.split(';').next()?.trim();
        let size = usize::from_str_radix(size_str, 16).ok()?;
        let after_header = line_end + 2;
        if size == 0 {
            // Trailer section: zero or more lines, terminated by CRLF.
            let mut j = after_header;
            loop {
                let trailer_end = find_crlf_at(buf, j)?;
                let len = trailer_end - j;
                if len == 0 {
                    return Some(trailer_end + 2);
                }
                j = trailer_end + 2;
            }
        }
        let next = after_header + size + 2;
        if next > buf.len() {
            return None;
        }
        if &buf[after_header + size..after_header + size + 2] != b"\r\n" {
            return None;
        }
        i = next;
    }
    None
}

fn find_crlf_at(buf: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    for i in 0..buf.len() - 3 {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n' {
            return Some(i);
        }
    }
    None
}

fn should_keep_alive(req: &Request) -> bool {
    let conn = req.header("connection").map(|s| s.to_ascii_lowercase());
    match req.version {
        crate::http::Version::Http11 => conn.as_deref() != Some("close"),
        crate::http::Version::Http10 => conn.as_deref() == Some("keep-alive"),
    }
}

async fn dispatch(app: &App, req: &mut Request) -> Result<Response> {
    let start = Instant::now();
    let (lookup, allowed) = app.router.lookup(req.method, &req.path);
    let response = match lookup {
        Some((handler, params)) => {
            req.params = params;
            let handler = handler.clone();
            // Wrap the router handler as our middleware HandlerFn.
            let chain = app.middleware.clone();
            let route = req.path.clone();
            // Dispatch through middleware chain.
            // Construct a closure that calls the handler.
            let wrapped: Box<HandlerFn> = Box::new(move |r: &mut Request| {
                let h = handler.clone();
                h(r)
            });
            crate::middleware::run_chain(&chain, wrapped.as_ref(), req)
                .await
                .map(|mut resp| {
                    // Record metrics.
                    app.metrics.record_request(
                        req.method.as_str(),
                        resp.status.class(),
                        &route,
                        start.elapsed(),
                    );
                    if req.method == Method::Head {
                        // Body suppression handled at write time.
                    }
                    resp.headers.insert("server", "nanoweb/0.1");
                    resp
                })
        }
        None => {
            if !allowed.is_empty() {
                if req.method == Method::Options {
                    let mut methods: Vec<String> =
                        allowed.iter().map(|m| m.as_str().to_string()).collect();
                    if !methods.iter().any(|m| m == "OPTIONS") {
                        methods.push("OPTIONS".into());
                    }
                    if !methods.iter().any(|m| m == "HEAD") && allowed.contains(&Method::Get) {
                        methods.push("HEAD".into());
                    }
                    methods.sort();
                    methods.dedup();
                    let mut resp = Response::new(StatusCode::NO_CONTENT);
                    resp.headers.insert("allow", methods.join(", "));
                    app.metrics.record_request(
                        req.method.as_str(),
                        resp.status.class(),
                        &req.path,
                        start.elapsed(),
                    );
                    return Ok(resp);
                }
                let mut methods: Vec<String> =
                    allowed.iter().map(|m| m.as_str().to_string()).collect();
                methods.sort();
                let mut resp =
                    Response::new(StatusCode::METHOD_NOT_ALLOWED).text("method not allowed");
                resp.headers.insert("allow", methods.join(", "));
                app.metrics.record_request(
                    req.method.as_str(),
                    resp.status.class(),
                    &req.path,
                    start.elapsed(),
                );
                Ok(resp)
            } else {
                let resp = Response::new(StatusCode::NOT_FOUND).text("not found");
                app.metrics.record_request(
                    req.method.as_str(),
                    resp.status.class(),
                    &req.path,
                    start.elapsed(),
                );
                Ok(resp)
            }
        }
    };
    // Suppress unused.
    let _ = parse_query;
    response
}

fn err_to_response(e: NanowebError) -> Response {
    match e {
        NanowebError::BadRequest(m) => Response::new(StatusCode::BAD_REQUEST).text(m),
        NanowebError::NotFound => Response::new(StatusCode::NOT_FOUND).text("not found"),
        NanowebError::Forbidden(m) => Response::new(StatusCode::FORBIDDEN).text(m),
        NanowebError::Unauthorized(m) => Response::new(StatusCode::UNAUTHORIZED).text(m),
        other => {
            log::error(&format!("handler error: {}", other));
            Response::new(StatusCode::INTERNAL_SERVER_ERROR).text("internal error")
        }
    }
}
