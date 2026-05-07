//! The tokio-based accept loop and per-connection HTTP/1.1 handler.
//!
//! Responsibilities:
//! * Bind a listener and accept connections.
//! * Drive each connection through the parser, chain, router, and
//!   metrics pipeline.
//! * Observe SIGINT/SIGTERM for graceful shutdown, bounded by
//!   `graceful_timeout_s` from `NanowebConfig::server`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::time;

use crate::config::NanowebConfig;
use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::parser::{parse_request, ParseOutcome, MAX_HEADER_BYTES};
use crate::http::types::{Method, Request, Response, StatusCode, Version};
use crate::metrics::Metrics;
use crate::middleware::Chain;
use crate::router::{Handler, NoMatch, Router};

/// Everything a live server needs.
pub struct Server {
    pub router: Arc<Router>,
    pub chain: Arc<Chain>,
    pub health: HealthState,
    pub metrics: Arc<Metrics>,
    pub config: Arc<NanowebConfig>,
}

impl Server {
    pub fn new(
        router: Router,
        chain: Chain,
        health: HealthState,
        metrics: Metrics,
        config: NanowebConfig,
    ) -> Self {
        Self {
            router: Arc::new(router),
            chain: Arc::new(chain),
            health,
            metrics: Arc::new(metrics),
            config: Arc::new(config),
        }
    }

    /// Bind, serve, and block until shutdown. The runtime must already
    /// be running.
    pub async fn run(self) -> Result<(), NanowebError> {
        let addr = self.config.listen_addr()?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| NanowebError::Io(e))?;

        let shutdown = Arc::new(Notify::new());
        spawn_signal_watcher(shutdown.clone(), self.health.clone());

        self.health.mark_live();
        self.health.mark_ready();

        eprintln!("nanoweb listening on http://{}", addr);

        let graceful = Duration::from_secs(self.config.server.graceful_timeout_s);
        let accept_shutdown = shutdown.clone();

        loop {
            tokio::select! {
                _ = accept_shutdown.notified() => {
                    break;
                }
                accept = listener.accept() => {
                    let (stream, peer) = match accept {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("accept error: {}", e);
                            continue;
                        }
                    };
                    let ctx = ConnectionCtx {
                        router: self.router.clone(),
                        chain: self.chain.clone(),
                        health: self.health.clone(),
                        metrics: self.metrics.clone(),
                    };
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, peer.to_string(), ctx).await {
                            eprintln!("connection error: {}", e);
                        }
                    });
                }
            }
        }

        self.health.mark_shutdown();
        // Give in-flight requests a chance to complete.
        let _ = time::timeout(graceful, async {
            time::sleep(Duration::from_millis(50)).await;
        })
        .await;
        Ok(())
    }
}

#[derive(Clone)]
struct ConnectionCtx {
    router: Arc<Router>,
    chain: Arc<Chain>,
    health: HealthState,
    metrics: Arc<Metrics>,
}

fn spawn_signal_watcher(shutdown: Arc<Notify>, health: HealthState) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigint = match signal(SignalKind::interrupt()) {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => return,
            };
            tokio::select! {
                _ = sigint.recv() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        health.mark_shutdown();
        shutdown.notify_waiters();
    });
}

async fn handle_connection(
    mut stream: TcpStream,
    peer: String,
    ctx: ConnectionCtx,
) -> Result<(), NanowebError> {
    let mut buf = Vec::with_capacity(4096);
    let mut scratch = [0u8; 4096];

    loop {
        // Try to parse what we already have before reading more.
        let outcome = parse_request(&buf);
        match outcome {
            Ok(ParseOutcome::Complete { mut request, consumed }) => {
                request.remote_addr = Some(peer.clone());
                let response = dispatch(&ctx, request).await?;
                let out = response.to_bytes(Version::Http11);
                if let Err(e) = stream.write_all(&out).await {
                    return Err(NanowebError::Io(e));
                }
                buf.drain(..consumed);

                let keep_alive = response
                    .headers
                    .get("connection")
                    .map(|v| !v.eq_ignore_ascii_case("close"))
                    .unwrap_or(true);
                if !keep_alive {
                    break;
                }
                if buf.is_empty() {
                    // Wait for the next request (pipelining / keep-alive).
                    match stream.read(&mut scratch).await {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&scratch[..n]),
                        Err(e) => return Err(NanowebError::Io(e)),
                    }
                }
            }
            Ok(ParseOutcome::Incomplete) => {
                if buf.len() > MAX_HEADER_BYTES + (1024 * 1024) {
                    return Err(NanowebError::Parse("request too large".into()));
                }
                match stream.read(&mut scratch).await {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&scratch[..n]),
                    Err(e) => return Err(NanowebError::Io(e)),
                }
            }
            Err(e) => {
                let resp = error_response(&e);
                let out = resp.to_bytes(Version::Http11);
                let _ = stream.write_all(&out).await;
                return Err(e);
            }
        }
    }
    Ok(())
}

fn error_response(err: &NanowebError) -> Response {
    let mut resp = Response::new(StatusCode::BAD_REQUEST);
    let body = format!("{}", err).into_bytes();
    resp.headers.set("content-type", "text/plain");
    resp.headers.set("content-length", body.len().to_string());
    resp.body = body;
    resp
}

async fn dispatch(ctx: &ConnectionCtx, req: Request) -> Result<Response, NanowebError> {
    let start = Instant::now();
    let path = req.path.clone();
    let method = req.method;

    // Intercept framework-owned endpoints before user routes.
    if method == Method::Options {
        let resp = ctx.router.options_response(&path);
        if resp.status != StatusCode::NOT_FOUND {
            return Ok(resp);
        }
    }

    if path == "/healthz" {
        return Ok(ctx.health.healthz());
    }
    if path == "/readyz" {
        return Ok(ctx.health.readyz());
    }
    if path == "/metrics" {
        let mut resp = Response::new(StatusCode::OK);
        resp.headers
            .set("content-type", "text/plain; version=0.0.4; charset=utf-8");
        let body = ctx.metrics.render().into_bytes();
        resp.headers.set("content-length", body.len().to_string());
        resp.body = body;
        return Ok(resp);
    }

    // Route lookup up-front so middleware (e.g. logger) can know the
    // pattern. The router also fills in path params.
    let (handler, params, pattern) = match ctx.router.find(method, &path) {
        Ok(m) => (Some(m.handler), m.params, m.pattern),
        Err(NoMatch::NotFound) => (None, HashMap::new(), path.clone()),
        Err(NoMatch::MethodNotAllowed(methods)) => {
            let mut resp = Response::status(StatusCode::METHOD_NOT_ALLOWED);
            let allow = methods
                .iter()
                .map(|m| m.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            resp.headers.set("allow", allow);
            return Ok(resp);
        }
    };

    let req = {
        let mut r = req;
        r.path_params = params;
        r.route_pattern = Some(pattern.clone());
        r
    };

    let response = ctx
        .chain
        .run(req, move |r| async move {
            match handler {
                Some(h) => run_handler(h, r).await,
                None => Ok(Response::status(StatusCode::NOT_FOUND)),
            }
        })
        .await?;

    let elapsed = start.elapsed().as_secs_f64();
    ctx.metrics
        .observe(method.as_str(), response.status.0, &pattern, elapsed);
    Ok(response)
}

async fn run_handler(h: Handler, req: Request) -> Result<Response, NanowebError> {
    (h)(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_response_has_body() {
        let resp = error_response(&NanowebError::Parse("bad".into()));
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        assert!(!resp.body.is_empty());
    }
}
