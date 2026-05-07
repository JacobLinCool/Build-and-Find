use crate::config::Config;
use crate::error::NanowebError;
use crate::health;
use crate::http::parser::parse_request;
use crate::http::{Method, Response};
use crate::metrics::Metrics;
use crate::middleware::Chain;
use crate::router::Router;
use crate::static_files;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

pub struct App {
    pub router: Router,
    pub middleware: Chain,
    pub config: Config,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            router: Router::new(),
            middleware: Chain::new(),
            config,
        }
    }
}

pub async fn run(app: Arc<App>) -> Result<(), NanowebError> {
    let addr = app
        .config
        .listen
        .clone()
        .unwrap_or_else(|| "127.0.0.1:8080".into());
    let listener = TcpListener::bind(&addr).await?;
    let _ = health::healthz();
    let mut tasks = JoinSet::new();
    let graceful = Duration::from_secs(app.config.graceful_timeout_s.unwrap_or(30));
    let mut shutting_down = false;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            res = listener.accept(), if !shutting_down => {
                let (mut socket, remote) = res?;
                let app = app.clone();
                tasks.spawn(async move {
                    let mut buf = vec![0; 64 * 1024];
                    let n = match socket.read(&mut buf).await { Ok(n) => n, Err(_) => return };
                    let mut req = match parse_request(&buf[..n], Some(remote)) { Ok(r) => r, Err(_) => { let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nbad request").await; return; } };
                    if req.target == "/healthz" { let _ = write_resp(&mut socket, health::healthz()).await; return; }
                    if req.target == "/readyz" { let _ = write_resp(&mut socket, health::readyz(!shutting_down)).await; return; }
                    if req.target == app.config.metrics.clone().unwrap_or_else(|| "/metrics".into()) {
                        let mut resp = Response::new(200, Metrics::global().render());
                        resp.headers.insert("content-type".into(), "text/plain; version=0.0.4".into());
                        let _ = write_resp(&mut socket, resp).await;
                        return;
                    }
                    if let Some(dir) = &app.config.static_dir {
                        if req.target.starts_with("/static/") {
                            let resp = static_files::serve(std::path::Path::new(dir), req.target.trim_start_matches("/static/"), req.header("if-none-match"));
                            let _ = write_resp(&mut socket, resp).await;
                            return;
                        }
                    }
                    if let Some(resp) = app.middleware.run_before(&mut req).await { let _ = write_resp(&mut socket, resp).await; return; }
                    let route = app.router.resolve(&req.method, &req.target);
                    let mut resp = if let Some((handler, params, _)) = route {
                        req.path_params = params;
                        (handler)(req.clone())
                    } else {
                        Response::text(404, "not found")
                    };
                    if matches!(req.method, Method::Head) { resp.body.clear(); }
                    app.middleware.run_after(&req, &mut resp).await;
                    Metrics::global().observe(req.method.as_str(), resp.status, &req.target, 0.0);
                    let _ = write_resp(&mut socket, resp).await;
                });
            }
            _ = tokio::signal::ctrl_c() => {
                shutting_down = true;
            }
            _ = sigterm.recv() => {
                shutting_down = true;
            }
        }
        if shutting_down {
            break;
        }
    }
    let _ = tokio::time::timeout(graceful, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    Ok(())
}

async fn write_resp(
    socket: &mut tokio::net::TcpStream,
    mut resp: Response,
) -> Result<(), std::io::Error> {
    let reason = match resp.status {
        200 => "OK",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "OK",
    };
    resp.headers
        .insert("content-length".into(), resp.body.len().to_string());
    resp.headers
        .entry("x-request-id".into())
        .or_insert_with(|| uuid::Uuid::new_v4().to_string());
    let mut head = format!("HTTP/1.1 {} {}\r\n", resp.status, reason);
    for (k, v) in resp.headers {
        head.push_str(&format!("{}: {}\r\n", canonical(&k), v));
    }
    head.push_str("\r\n");
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(&resp.body).await?;
    Ok(())
}

fn canonical(k: &str) -> String {
    k.split('-')
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}
