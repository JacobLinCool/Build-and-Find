//! Application assembly, accept loop, and graceful shutdown.

use crate::config::Config;
use crate::health::Health;
use crate::http::parser::read_request;
use crate::http::{write_response, Method, Request, Response, StatusCode};
use crate::metrics::Metrics;
use crate::middleware::ArcMiddleware;
use crate::router::{ArcHandler, Router};
use crate::static_files::StaticFiles;
use crate::NanowebError;
use rand::RngCore;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Notify};
use tokio::time::{timeout, Duration};

pub struct App {
    router: Router,
    middleware: Vec<ArcMiddleware>,
    metrics: Arc<Metrics>,
    health: Arc<Health>,
    config: Config,
    static_dir: Option<PathBuf>,
    metrics_path: String,
}

impl App {
    pub fn new(config: Config) -> Self {
        let metrics_path = config.metrics_path.clone();
        let static_dir = config.static_dir.clone();
        Self {
            router: Router::new(),
            middleware: Vec::new(),
            metrics: Arc::new(Metrics::new()),
            health: Arc::new(Health::new()),
            config,
            static_dir,
            metrics_path,
        }
    }

    pub fn with_middleware(mut self, mw: ArcMiddleware) -> Self {
        self.middleware.push(mw);
        self
    }

    pub fn route(&mut self, method: Method, pattern: &str, handler: ArcHandler) -> &mut Self {
        self.router.route(method, pattern, handler);
        self
    }

    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }
    pub fn health(&self) -> Arc<Health> {
        self.health.clone()
    }
    pub fn router_mut(&mut self) -> &mut Router {
        &mut self.router
    }

    /// Bind and serve on an ephemeral or specific address until `shutdown` fires.
    /// Returns the local address bound. For tests and programmatic use.
    pub async fn serve(
        self,
        addr: SocketAddr,
        shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), NanowebError> {
        let app = Arc::new(self);
        app.health.set_ready(true);
        let listener = TcpListener::bind(addr).await.map_err(NanowebError::Io)?;
        let local = listener.local_addr().map_err(NanowebError::Io)?;
        let (tx, rx) = watch::channel(false);
        let app2 = app.clone();
        let handle = tokio::spawn(async move {
            tokio::pin!(shutdown);
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, peer)) => {
                                let app3 = app2.clone();
                                let rx2 = rx.clone();
                                tokio::spawn(async move {
                                    let _ = handle_plain(app3, stream, peer, rx2).await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                    _ = &mut shutdown => {
                        let _ = tx.send(true);
                        break;
                    }
                }
            }
        });
        Ok((local, handle))
    }

    pub async fn run(self) -> Result<(), NanowebError> {
        let app = Arc::new(self);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let plain_addr: SocketAddr = app
            .config
            .listen
            .parse()
            .map_err(|e: std::net::AddrParseError| NanowebError::Config(e.to_string()))?;
        let listener = TcpListener::bind(plain_addr)
            .await
            .map_err(NanowebError::Io)?;
        let actual = listener.local_addr().ok();
        tracing::info!(addr = ?actual, "nanoweb listening");
        app.health.set_ready(true);
        let in_flight = Arc::new(Notify::new());
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let app_for_loop = app.clone();
        let active_for_loop = active.clone();
        let in_flight_for_loop = in_flight.clone();
        let mut sd_rx_loop = shutdown_rx.clone();
        let plain_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, peer)) => {
                                let app2 = app_for_loop.clone();
                                let active2 = active_for_loop.clone();
                                let notify2 = in_flight_for_loop.clone();
                                let sd2 = sd_rx_loop.clone();
                                active_for_loop.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                tokio::spawn(async move {
                                    let _ = handle_plain(app2, stream, peer, sd2).await;
                                    if active2.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                                        notify2.notify_waiters();
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "accept failed");
                            }
                        }
                    }
                    _ = sd_rx_loop.changed() => {
                        if *sd_rx_loop.borrow() {
                            break;
                        }
                    }
                }
            }
        });

        // Optional TLS listener.
        let mut tls_task: Option<tokio::task::JoinHandle<()>> = None;
        #[cfg(feature = "tls")]
        if let (Some(cert), Some(key)) = (&app.config.tls_cert, &app.config.tls_key) {
            let acceptor = crate::tls::load_acceptor(cert, key)?;
            let tls_listener = TcpListener::bind(plain_addr).await.ok();
            if let Some(_l) = tls_listener {
                // Bind a separate port +1 for TLS by default if desired.
                // For simplicity, only bind the same port if the user supplied a different listen for TLS via env;
                // Here we accept that TLS uses the same port only if plain isn't bound; otherwise skip.
            }
            // Bind a separate TLS port: take listen port + 443? We'll skip auto-binding to avoid conflict.
            // Instead, bind to listen address with port adjusted from env NANOWEB_TLS_LISTEN, fallback 127.0.0.1:8443.
            let tls_addr: SocketAddr = std::env::var("NANOWEB_TLS_LISTEN")
                .unwrap_or_else(|_| "127.0.0.1:8443".to_string())
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:8443".parse().unwrap());
            let tls_l = TcpListener::bind(tls_addr)
                .await
                .map_err(NanowebError::Io)?;
            tracing::info!(addr = ?tls_l.local_addr().ok(), "tls listening");
            let app3 = app.clone();
            let active3 = active.clone();
            let in_flight3 = in_flight.clone();
            let mut sd3 = shutdown_rx.clone();
            tls_task = Some(tokio::spawn(async move {
                loop {
                    tokio::select! {
                        accept = tls_l.accept() => {
                            match accept {
                                Ok((stream, peer)) => {
                                    let acc = acceptor.clone();
                                    let app4 = app3.clone();
                                    let active4 = active3.clone();
                                    let notify4 = in_flight3.clone();
                                    let sd4 = sd3.clone();
                                    active3.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                    tokio::spawn(async move {
                                        if let Ok(tls_stream) = acc.accept(stream).await {
                                            let _ = handle_stream(app4, tls_stream, peer, sd4).await;
                                        }
                                        if active4.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                                            notify4.notify_waiters();
                                        }
                                    });
                                }
                                Err(e) => tracing::warn!(error=%e, "tls accept failed"),
                            }
                        }
                        _ = sd3.changed() => { if *sd3.borrow() { break; } }
                    }
                }
            }));
        }

        #[cfg(not(feature = "tls"))]
        if app.config.tls_cert.is_some() || app.config.tls_key.is_some() {
            return Err(crate::tls::tls_unsupported());
        }

        // Wait for shutdown signal.
        wait_for_signal().await;
        tracing::info!("shutdown signal received");
        app.health.set_ready(false);
        let _ = shutdown_tx.send(true);

        // Give in-flight requests a bounded grace period.
        let grace = Duration::from_secs(app.config.graceful_timeout_s);
        let _ = timeout(grace, async {
            while active.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                in_flight.notified().await;
            }
        })
        .await;

        plain_task.abort();
        if let Some(t) = tls_task {
            t.abort();
        }
        Ok(())
    }
}

async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).unwrap();
    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
}

async fn handle_plain(
    app: Arc<App>,
    stream: TcpStream,
    peer: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), NanowebError> {
    handle_stream(app, stream, peer, shutdown_rx).await
}

async fn handle_stream<S>(
    app: Arc<App>,
    stream: S,
    peer: SocketAddr,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), NanowebError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (rd, mut wr) = tokio::io::split(stream);
    let mut reader = BufReader::new(rd);
    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        let req = tokio::select! {
            r = read_request(&mut reader) => r,
            _ = shutdown_rx.changed() => { break; }
        };
        let mut req = match req {
            Ok(Some(r)) => r,
            Ok(None) => break,
            Err(e) => {
                let mut bad = Response::new(StatusCode::BAD_REQUEST);
                bad.body = format!("bad request: {}\n", e).into_bytes();
                bad.headers
                    .insert("Content-Type", "text/plain; charset=utf-8");
                bad.headers.insert("Connection", "close");
                let _ = wr.write_all(&write_response(&bad, false)).await;
                break;
            }
        };
        req.remote_addr = Some(peer);
        req.request_id = new_request_id();
        let want_close = req
            .header("connection")
            .map(|h| h.eq_ignore_ascii_case("close"))
            .unwrap_or(false);
        let resp = process(&app, &mut req).await;
        let mut resp = resp.unwrap_or_else(|e| {
            let mut r = Response::new(StatusCode::INTERNAL_SERVER_ERROR);
            r.body = format!("internal: {}\n", e).into_bytes();
            r.headers
                .insert("Content-Type", "text/plain; charset=utf-8");
            r
        });
        resp.headers.insert("X-Request-Id", req.request_id.clone());
        if want_close {
            resp.headers.insert("Connection", "close");
        }
        let suppress = req.method == Method::Head;
        let bytes = write_response(&resp, suppress);
        if wr.write_all(&bytes).await.is_err() {
            break;
        }
        if want_close {
            break;
        }
    }
    Ok(())
}

async fn process(app: &Arc<App>, req: &mut Request) -> Result<Response, NanowebError> {
    let start = std::time::Instant::now();
    // Built-in routes.
    if req.path == "/healthz" {
        let r = if app.health.is_up() {
            Response::text("ok")
        } else {
            Response::new(StatusCode::INTERNAL_SERVER_ERROR)
        };
        return Ok(record(app, req, r, start));
    }
    if req.path == "/readyz" {
        let r = if app.health.is_ready() {
            Response::text("ready")
        } else {
            Response::new(StatusCode::INTERNAL_SERVER_ERROR)
        };
        return Ok(record(app, req, r, start));
    }
    if req.path == app.metrics_path {
        let body = app.metrics.render();
        let mut r = Response::new(StatusCode::OK);
        r.headers
            .insert("Content-Type", "text/plain; version=0.0.4");
        r.body = body.into_bytes();
        return Ok(record(app, req, r, start));
    }

    // Run middleware before-phase.
    for mw in &app.middleware {
        if let Some(r) = mw.before(req).await? {
            let r = run_after(app, req, r).await?;
            return Ok(record(app, req, r, start));
        }
    }

    // Routing.
    let m = app.router.lookup(req.method, &req.path);
    let resp = match m {
        Some(m) => {
            req.params = m.params;
            m.handler.call(req.clone()).await?
        }
        None => {
            // Allow OPTIONS auto-listing or 405 if path matches different methods.
            let allowed = app.router.allowed_for(&req.path);
            if !allowed.is_empty() && req.method == Method::Options {
                let mut r = Response::new(StatusCode::NO_CONTENT);
                let allow = allowed
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                r.headers.insert("Allow", allow);
                r
            } else if !allowed.is_empty() {
                let mut r = Response::new(StatusCode::METHOD_NOT_ALLOWED);
                let allow = allowed
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                r.headers.insert("Allow", allow);
                r
            } else if let Some(dir) = &app.static_dir {
                let sf = StaticFiles::new(dir.clone());
                sf.serve(req, &req.path)?
            } else {
                let mut r = Response::new(StatusCode::NOT_FOUND);
                r.body = b"not found".to_vec();
                r.headers
                    .insert("Content-Type", "text/plain; charset=utf-8");
                r
            }
        }
    };
    let resp = run_after(app, req, resp).await?;
    Ok(record(app, req, resp, start))
}

async fn run_after(
    app: &Arc<App>,
    req: &Request,
    mut resp: Response,
) -> Result<Response, NanowebError> {
    for mw in app.middleware.iter().rev() {
        resp = mw.after(req, resp).await?;
    }
    Ok(resp)
}

fn record(app: &Arc<App>, req: &Request, resp: Response, start: std::time::Instant) -> Response {
    let dur = start.elapsed().as_secs_f64();
    app.metrics
        .record(req.method, &req.path, resp.status.class(), dur);
    resp
}

fn new_request_id() -> String {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_format() {
        let id = new_request_id();
        assert_eq!(id.len(), 24);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
