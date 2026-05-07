use crate::config::Config;
use crate::error::{NanowebError, Result};
use crate::health::Health;
use crate::http::parser::{BodyKind, ParsedHead, RequestParser};
use crate::http::{Method, Request, Response, StatusCode};
use crate::metrics::Metrics;
use crate::middleware::{self, Chain};
use crate::router::{Lookup, Router};
use crate::static_files::StaticFiles;
use rand::RngCore;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 16 * 1024 * 1024;

/// The runnable application: combines a router, middleware chain, configured
/// modules, and lifecycle state.
pub struct App {
    pub router: Router,
    pub chain: Chain,
    pub static_files: Option<StaticFiles>,
    pub metrics: Metrics,
    pub health: Health,
    pub config: Arc<Config>,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            router: Router::new(),
            chain: Chain::new(),
            static_files: config.static_dir.as_ref().map(StaticFiles::new),
            metrics: Metrics::new(),
            health: Health::new(),
            config: Arc::new(config),
        }
    }

    pub fn with_static_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.static_files = Some(StaticFiles::new(dir));
        self
    }

    pub fn with_middleware<M: middleware::Middleware + 'static>(mut self, m: M) -> Self {
        self.chain = self.chain.push(m);
        self
    }
}

/// Run an `App` on the address specified in its config. Plain HTTP listener
/// only. TLS support is exposed via `serve_with_tls` when the `tls` feature
/// is enabled. Graceful shutdown is wired via SIGINT / SIGTERM.
pub async fn serve(app: App) -> Result<()> {
    let cfg = app.config.clone();
    let app = Arc::new(app);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let listener = TcpListener::bind(&cfg.listen).await?;
    tracing::info!(addr = %cfg.listen, "nanoweb listening");
    app.health.mark_ready();

    let plain = tokio::spawn(accept_loop(
        app.clone(),
        listener,
        shutdown_rx.clone(),
        false,
    ));

    #[cfg(feature = "tls")]
    let tls_task = if cfg.tls_enabled() {
        let addr = cfg
            .tls_listen_addr()
            .ok_or_else(|| NanowebError::Tls("tls listen address missing".into()))?;
        let listener = TcpListener::bind(&addr).await?;
        let server_config = crate::tls::load_server_config(
            cfg.tls_cert.as_ref().unwrap(),
            cfg.tls_key.as_ref().unwrap(),
        )?;
        tracing::info!(addr = %addr, "nanoweb TLS listening");
        Some(tokio::spawn(accept_loop_tls(
            app.clone(),
            listener,
            server_config,
            shutdown_rx.clone(),
        )))
    } else {
        None
    };

    // Wait for shutdown signal.
    wait_signal().await;
    tracing::info!("shutdown signal received; draining connections");
    app.health.mark_unready();
    let _ = shutdown_tx.send(true);

    // Bound the wait by the configured graceful timeout.
    let timeout = Duration::from_secs(cfg.graceful_timeout_s);
    let _ = tokio::time::timeout(timeout, plain).await;
    #[cfg(feature = "tls")]
    if let Some(t) = tls_task {
        let _ = tokio::time::timeout(timeout, t).await;
    }
    Ok(())
}

#[cfg(unix)]
async fn wait_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT");
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM");
    tokio::select! {
        _ = int.recv() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

pub async fn run_with_listener(
    app: Arc<App>,
    listener: TcpListener,
    shutdown: watch::Receiver<bool>,
) {
    app.health.mark_ready();
    accept_loop(app, listener, shutdown, false).await;
}

async fn accept_loop(
    app: Arc<App>,
    listener: TcpListener,
    mut shutdown: watch::Receiver<bool>,
    is_tls: bool,
) {
    let _ = is_tls;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, addr)) => {
                        let app = app.clone();
                        let shutdown = shutdown.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, app, shutdown).await {
                                tracing::debug!(error=?e, "connection error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error=?e, "accept failed");
                    }
                }
            }
        }
    }
}

#[cfg(feature = "tls")]
async fn accept_loop_tls(
    app: Arc<App>,
    listener: TcpListener,
    cfg: std::sync::Arc<rustls::ServerConfig>,
    mut shutdown: watch::Receiver<bool>,
) {
    let acceptor = tokio_rustls::TlsAcceptor::from(cfg);
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            res = listener.accept() => {
                match res {
                    Ok((stream, addr)) => {
                        let app = app.clone();
                        let shutdown = shutdown.clone();
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            match acceptor.accept(stream).await {
                                Ok(tls) => {
                                    if let Err(e) = handle_connection(tls, addr, app, shutdown).await {
                                        tracing::debug!(error=?e, "tls connection error");
                                    }
                                }
                                Err(e) => tracing::debug!(error=?e, "tls handshake failed"),
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error=?e, "accept failed"),
                }
            }
        }
    }
}

async fn handle_connection<S>(
    mut stream: S,
    addr: SocketAddr,
    app: Arc<App>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()>
where
    S: tokio::io::AsyncRead + AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut tmp = [0u8; 8 * 1024];
    loop {
        // shutdown observed: stop reading new requests from this connection.
        if *shutdown.borrow() {
            break;
        }
        // Read request head.
        let head = loop {
            match RequestParser::parse_head(&buf) {
                Ok(Some(h)) => break h,
                Ok(None) => {}
                Err(e) => {
                    let resp = build_error_response(StatusCode::BAD_REQUEST, &e.to_string());
                    write_response(&mut stream, &resp, false, false).await.ok();
                    return Ok(());
                }
            }
            if buf.len() > MAX_HEAD {
                let resp = build_error_response(StatusCode::BAD_REQUEST, "request head too large");
                write_response(&mut stream, &resp, false, false).await.ok();
                return Ok(());
            }
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() && buf.is_empty() {
                        return Ok(());
                    }
                }
                n = stream.read(&mut tmp) => {
                    let n = match n {
                        Ok(0) => return Ok(()),
                        Ok(n) => n,
                        Err(_) => return Ok(()),
                    };
                    buf.extend_from_slice(&tmp[..n]);
                }
            }
        };

        let body = match read_body(&mut stream, &mut buf, &head).await {
            Ok(b) => b,
            Err(e) => {
                let resp = build_error_response(StatusCode::BAD_REQUEST, &e.to_string());
                write_response(&mut stream, &resp, false, false).await.ok();
                return Ok(());
            }
        };

        let keep_alive = head.keep_alive;
        let head_method = head.method;
        let request = build_full_request(head, body, addr);
        let head_request_was_head = request.method == Method::Head;
        let is_options = request.method == Method::Options;

        let started = Instant::now();
        let route_label;
        let resp_status;
        let mut resp;

        // Dispatch: routes first, then static files, then 404.
        let lookup_method = if head_method == Method::Head {
            Method::Get
        } else {
            head_method
        };
        let _ = lookup_method;
        let lookup = app.router.lookup(request.method, &request.path);
        match lookup {
            Lookup::Found(m) => {
                let mut req = request.clone();
                for (k, v) in m.params.iter() {
                    req.params.insert(k.clone(), v.clone());
                }
                let route_name = req.path.clone();
                route_label = route_name;
                let head_fb = m.head_fallback;
                let handler = m.handler.clone();
                resp = middleware::run(&app.chain, req, |r| async move { handler.handle(r).await })
                    .await;
                if head_fb {
                    resp.body.clear();
                }
            }
            Lookup::MethodNotAllowed(am) => {
                if is_options {
                    let allow = am
                        .allowed
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let mut r = Response::new(StatusCode::NO_CONTENT);
                    r.headers.insert("Allow", allow);
                    resp =
                        middleware::run(&app.chain, request.clone(), |_r| async move { r }).await;
                } else {
                    let allow = am
                        .allowed
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let mut r = Response::new(StatusCode::METHOD_NOT_ALLOWED);
                    r.headers.insert("Allow", allow);
                    r.body = b"method not allowed".to_vec();
                    resp = r;
                }
                route_label = request.path.clone();
            }
            Lookup::NotFound => {
                // Static fallback.
                if let Some(sf) = &app.static_files {
                    if let Some(rest) = request.path.strip_prefix("/static/") {
                        let r = sf.serve(&request, rest).await;
                        let r = middleware::run(&app.chain, request.clone(), |_r| async move { r })
                            .await;
                        resp = r;
                    } else {
                        let mut r = Response::new(StatusCode::NOT_FOUND);
                        r.body = b"not found".to_vec();
                        resp = r;
                    }
                } else {
                    let mut r = Response::new(StatusCode::NOT_FOUND);
                    r.body = b"not found".to_vec();
                    resp = r;
                }
                route_label = request.path.clone();
            }
        }

        // Always set X-Request-Id.
        if !request.request_id.is_empty() {
            resp.headers
                .insert("X-Request-Id", request.request_id.clone());
        }

        resp_status = resp.status.as_u16();
        let elapsed = started.elapsed().as_secs_f64();
        app.metrics
            .record(head_method, resp_status, &route_label, elapsed);

        write_response(&mut stream, &resp, head_request_was_head, false).await?;
        if !keep_alive {
            break;
        }
    }
    Ok(())
}

fn build_full_request(head: ParsedHead, body: Vec<u8>, addr: SocketAddr) -> Request {
    let target = head.target.clone();
    let mut req = Request::new(head.method, target, head.version);
    req.headers = head.headers;
    req.body = body;
    req.remote_addr = Some(addr);
    req.request_id = req
        .header("X-Request-Id")
        .map(|s| s.to_string())
        .unwrap_or_else(generate_request_id);
    req
}

fn generate_request_id() -> String {
    let mut buf = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

async fn read_body<S>(stream: &mut S, buf: &mut Vec<u8>, head: &ParsedHead) -> Result<Vec<u8>>
where
    S: tokio::io::AsyncRead + Unpin,
{
    match head.body_kind {
        BodyKind::None => {
            buf.drain(..head.head_len);
            Ok(Vec::new())
        }
        BodyKind::ContentLength(n) => {
            if n > MAX_BODY {
                return Err(NanowebError::InvalidRequest("body too large".into()));
            }
            // Ensure we've read enough data for the body.
            while buf.len() < head.head_len + n {
                let mut tmp = [0u8; 8 * 1024];
                let r = stream.read(&mut tmp).await?;
                if r == 0 {
                    return Err(NanowebError::InvalidRequest("incomplete body".into()));
                }
                buf.extend_from_slice(&tmp[..r]);
            }
            let body = buf[head.head_len..head.head_len + n].to_vec();
            buf.drain(..head.head_len + n);
            Ok(body)
        }
        BodyKind::Chunked => {
            // Consume chunks from buf, reading more as needed.
            loop {
                let body_buf = &buf[head.head_len..];
                match RequestParser::parse_chunked(body_buf) {
                    Ok(Some((decoded, used))) => {
                        if decoded.len() > MAX_BODY {
                            return Err(NanowebError::InvalidRequest("body too large".into()));
                        }
                        buf.drain(..head.head_len + used);
                        return Ok(decoded);
                    }
                    Ok(None) => {
                        let mut tmp = [0u8; 8 * 1024];
                        let r = stream.read(&mut tmp).await?;
                        if r == 0 {
                            return Err(NanowebError::InvalidRequest("incomplete chunked".into()));
                        }
                        buf.extend_from_slice(&tmp[..r]);
                    }
                    Err(e) => return Err(NanowebError::InvalidRequest(e.to_string())),
                }
            }
        }
    }
}

fn build_error_response(status: StatusCode, message: &str) -> Response {
    let mut r = Response::new(status);
    r.headers
        .insert("Content-Type", "text/plain; charset=utf-8");
    r.body = message.as_bytes().to_vec();
    r
}

async fn write_response<S>(
    stream: &mut S,
    resp: &Response,
    head_request: bool,
    force_close: bool,
) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    let mut out: Vec<u8> = Vec::with_capacity(256 + resp.body.len());
    out.extend_from_slice(
        format!(
            "HTTP/1.1 {} {}\r\n",
            resp.status.as_u16(),
            resp.status.reason()
        )
        .as_bytes(),
    );
    let mut headers = resp.headers.clone();
    if !headers.contains("Content-Length") {
        let len = if head_request { 0 } else { resp.body.len() };
        headers.insert("Content-Length", len.to_string());
    }
    if !headers.contains("Content-Type") && !resp.body.is_empty() {
        headers.insert("Content-Type", "application/octet-stream");
    }
    if !headers.contains("Date") {
        headers.insert("Date", httpdate());
    }
    if !headers.contains("Server") {
        headers.insert("Server", "nanoweb/0.1");
    }
    if force_close && !headers.contains("Connection") {
        headers.insert("Connection", "close");
    }
    for (k, v) in headers.iter() {
        out.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if !head_request {
        out.extend_from_slice(&resp.body);
    }
    stream.write_all(&out).await?;
    stream.flush().await.ok();
    Ok(())
}

fn httpdate() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    fmt_imf(secs)
}

fn fmt_imf(secs: u64) -> String {
    // RFC 7231 IMF-fixdate, e.g. "Sun, 06 Nov 1994 08:49:37 GMT"
    let days_total = (secs / 86400) as i64;
    let secs_of_day = secs % 86400;
    let h = (secs_of_day / 3600) as u32;
    let m = ((secs_of_day % 3600) / 60) as u32;
    let s = (secs_of_day % 60) as u32;

    // Convert days since 1970-01-01 (Thursday) to date.
    let (y, mo, d, dow) = days_to_date(days_total);
    const DOW: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        DOW[dow],
        d,
        MON[(mo - 1) as usize],
        y,
        h,
        m,
        s
    )
}

fn days_to_date(days: i64) -> (i32, u32, u32, usize) {
    // Algorithm from "Date Algorithms" by Howard Hinnant.
    let z = days + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0,399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let dow = ((days % 7 + 4) % 7) as usize; // 1970-01-01 was Thursday (idx 4)
    (y as i32, m as u32, d, dow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imf_known_value() {
        // 0 secs = 1970-01-01 00:00:00 GMT (Thursday)
        let s = fmt_imf(0);
        assert_eq!(s, "Thu, 01 Jan 1970 00:00:00 GMT");
    }
}
