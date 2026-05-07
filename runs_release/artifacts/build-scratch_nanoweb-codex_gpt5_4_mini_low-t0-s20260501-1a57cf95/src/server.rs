use crate::config::Config;
use crate::error::NanowebError;
use crate::http::parser::parse_request;
use crate::http::types::{Method, Request, Response};
use crate::metrics::Metrics;
use crate::middleware::Middleware;
use crate::router::Router;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct App {
    pub router: Router,
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub metrics: Arc<Metrics>,
    pub config: Config,
    pub ready: AtomicBool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            router: Router::default(),
            middleware: vec![],
            metrics: Arc::new(Metrics::default()),
            config,
            ready: AtomicBool::new(true),
        }
    }
    pub async fn run(self: Arc<Self>) -> Result<(), NanowebError> {
        let listener = TcpListener::bind(&self.config.listen).await?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown2.store(true, Ordering::SeqCst);
        });
        while !shutdown.load(Ordering::SeqCst) {
            let (stream, addr) = tokio::select! {
                r = listener.accept() => r?,
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => continue,
            };
            let app = self.clone();
            tokio::spawn(async move {
                let _ = serve_connection(app, stream, addr).await;
            });
        }
        Ok(())
    }
}

pub async fn serve_connection(
    app: Arc<App>,
    mut stream: TcpStream,
    addr: SocketAddr,
) -> Result<(), NanowebError> {
    let mut buf = vec![0; 8192];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let (mut req, _) = match parse_request(&buf[..n]) {
        Ok(v) => v,
        Err(_) => {
            stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                .await?;
            return Ok(());
        }
    };
    req.peer_addr = Some(addr);
    req.request_id = uuid::Uuid::new_v4().to_string();
    let mut short = None;
    for m in &app.middleware {
        if let Some(resp) = m.before(&mut req)? {
            short = Some(resp);
            break;
        }
    }
    let method_for_head = req.method.clone();
    let mut resp = if let Some(resp) = short {
        resp
    } else {
        dispatch(&app, req.clone()).await
    };
    for m in app.middleware.iter().rev() {
        m.after(&req, &mut resp)?;
    }
    resp.headers
        .insert("x-request-id".into(), req.request_id.clone());
    write_response(&mut stream, method_for_head == Method::Head, resp).await?;
    Ok(())
}

async fn dispatch(app: &Arc<App>, req: Request) -> Response {
    if req.target == app.config.metrics {
        return Response::html(200, app.metrics.render());
    }
    if req.target == "/healthz" {
        return Response::text(200, crate::health::healthz());
    }
    if req.target == "/readyz" {
        return Response::text(
            if app.ready.load(Ordering::SeqCst) {
                200
            } else {
                503
            },
            crate::health::readyz(app.ready.load(Ordering::SeqCst)),
        );
    }
    if let Some((h, params, allowed)) = app.router.resolve(
        &req.method,
        req.target.split('?').next().unwrap_or(&req.target),
    ) {
        let mut req2 = req;
        let method = req2.method.clone();
        req2.params = params;
        let mut resp = h(req2);
        if matches!(method, Method::Options) {
            resp = Response::new(204, Vec::new());
            resp.headers.insert("allow".into(), allowed.join(", "));
        }
        return resp;
    }
    Response::text(404, "not found")
}

async fn write_response(
    stream: &mut TcpStream,
    head_only: bool,
    mut resp: Response,
) -> Result<(), NanowebError> {
    let status_text = match resp.status {
        200 => "OK",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "OK",
    };
    if head_only {
        resp.body.clear();
    }
    resp.headers
        .entry("content-length".into())
        .or_insert_with(|| resp.body.len().to_string());
    let mut out = format!("HTTP/1.1 {} {}\r\n", resp.status, status_text);
    for (k, v) in resp.headers {
        out.push_str(&format!("{}: {}\r\n", canonical(&k), v));
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes()).await?;
    stream.write_all(&resp.body).await?;
    Ok(())
}
fn canonical(s: &str) -> String {
    s.split('-')
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonicalizes() {
        assert_eq!(canonical("x-request-id"), "X-Request-Id");
    }
}
