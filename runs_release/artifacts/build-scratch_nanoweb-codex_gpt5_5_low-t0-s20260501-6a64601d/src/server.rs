use crate::config::{Config, RouteConfig};
use crate::error::{NanowebError, Result};
use crate::health::Health;
use crate::http::parser::parse_request;
use crate::http::{Method, Request, Response, StatusCode};
use crate::metrics::Metrics;
use crate::middleware::{
    Middleware, auth::BasicAuth, cors::Cors, csrf::Csrf, gzip::Gzip, logging::Logging,
    rate_limit::RateLimit,
};
use crate::router::{App, Router};
use crate::static_files::StaticFiles;
use crate::template::Renderer;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast};

#[derive(Clone)]
pub struct Server {
    pub config: Config,
    app: App,
    metrics: Arc<Metrics>,
    health: Arc<Health>,
}

impl Server {
    pub fn from_config(config: Config) -> Result<Self> {
        let metrics = Arc::new(Metrics::default());
        let health = Arc::new(Health::default());
        let mut router = Router::new();

        add_builtin_routes(&mut router, &config, metrics.clone(), health.clone());
        if let Some(dir) = &config.static_dir {
            let files = StaticFiles::new(dir);
            router.add(Method::Get, "/static/*path", move |req: Request| {
                let files = files.clone();
                async move { files.serve(req, "/static").await }
            });
        }
        for route in &config.routes {
            add_config_route(&mut router, route, config.static_dir.clone())?;
        }

        let mut middleware: Vec<Arc<dyn Middleware>> = Vec::new();
        let mw = &config.middleware;
        if mw.logging {
            middleware.push(Arc::new(Logging));
        }
        if !mw.cors_origins.is_empty() {
            middleware.push(Arc::new(Cors::new(mw.cors_origins.clone())));
        }
        if let Some(auth) = &mw.basic_auth {
            middleware.push(Arc::new(BasicAuth::new(
                auth.username.clone(),
                auth.password.clone(),
            )));
        }
        if let Some(rate) = &mw.rate_limit {
            middleware.push(Arc::new(RateLimit::new(rate.capacity, rate.refill_per_s)));
        }
        if mw.csrf {
            middleware.push(Arc::new(Csrf));
        }
        if mw.gzip {
            middleware.push(Arc::new(Gzip));
        }

        Ok(Self {
            config,
            app: App::new(router).with_middleware(middleware),
            metrics,
            health,
        })
    }

    pub async fn run(self) -> Result<()> {
        let addr: SocketAddr = self
            .config
            .listen
            .parse()
            .map_err(|e| NanowebError::Config(format!("invalid listen address: {e}")))?;
        let listener = TcpListener::bind(addr).await?;
        self.run_with_listener(listener).await
    }

    #[cfg(feature = "tls")]
    pub async fn run_tls(self, tls_config: rustls::ServerConfig) -> Result<()> {
        let addr: SocketAddr = self
            .config
            .listen
            .parse()
            .map_err(|e| NanowebError::Config(format!("invalid listen address: {e}")))?;
        let listener = TcpListener::bind(addr).await?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_config));
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let sem = Arc::new(Semaphore::new(10_000));
        self.health.set_ready(true);

        let shutdown_task_tx = shutdown_tx.clone();
        tokio::spawn(async move {
            wait_shutdown_signal().await;
            let _ = shutdown_task_tx.send(());
        });

        let mut shutdown_rx = shutdown_tx.subscribe();
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    self.health.set_ready(false);
                    break;
                }
                accepted = listener.accept() => {
                    let (stream, peer) = accepted?;
                    let permit = sem.clone().acquire_owned().await.map_err(|e| NanowebError::Server(e.to_string()))?;
                    let app = self.app.clone();
                    let metrics = self.metrics.clone();
                    let acceptor = acceptor.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        match acceptor.accept(stream).await {
                            Ok(tls_stream) => {
                                let _ = handle_connection(tls_stream, peer, app, metrics).await;
                            }
                            Err(err) => tracing::warn!(error = %err, "tls handshake failed"),
                        }
                    });
                }
            }
        }
        let timeout = Duration::from_secs(self.config.graceful_timeout_s);
        let _ = tokio::time::timeout(timeout, async {
            loop {
                if sem.available_permits() == 10_000 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        Ok(())
    }

    #[cfg(not(feature = "tls"))]
    pub async fn run_tls(self, _tls_config: ()) -> Result<()> {
        Err(NanowebError::Tls(
            "TLS support is disabled; rebuild with the `tls` feature".into(),
        ))
    }

    pub async fn run_with_listener(self, listener: TcpListener) -> Result<()> {
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let sem = Arc::new(Semaphore::new(10_000));
        self.health.set_ready(true);

        let shutdown_task_tx = shutdown_tx.clone();
        tokio::spawn(async move {
            wait_shutdown_signal().await;
            let _ = shutdown_task_tx.send(());
        });

        let mut shutdown_rx = shutdown_tx.subscribe();
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    self.health.set_ready(false);
                    break;
                }
                accepted = listener.accept() => {
                    let (stream, peer) = accepted?;
                    let permit = sem.clone().acquire_owned().await.map_err(|e| NanowebError::Server(e.to_string()))?;
                    let app = self.app.clone();
                    let metrics = self.metrics.clone();
                    let mut conn_shutdown = shutdown_tx.subscribe();
                    tokio::spawn(async move {
                        let _permit = permit;
                        tokio::select! {
                            _ = handle_connection(stream, peer, app, metrics) => {}
                            _ = conn_shutdown.recv() => {}
                        }
                    });
                }
            }
        }

        let timeout = Duration::from_secs(self.config.graceful_timeout_s);
        let _ = tokio::time::timeout(timeout, async {
            loop {
                if sem.available_permits() == 10_000 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await;
        Ok(())
    }
}

fn add_builtin_routes(
    router: &mut Router,
    config: &Config,
    metrics: Arc<Metrics>,
    health: Arc<Health>,
) {
    router.add(Method::Get, &config.metrics, move |_req: Request| {
        let metrics = metrics.clone();
        async move {
            let mut res = Response::text(StatusCode::Ok, metrics.render());
            res.header("Content-Type", "text/plain; version=0.0.4");
            res
        }
    });
    router.add(Method::Get, "/healthz", |_req: Request| async {
        Response::text(StatusCode::Ok, "ok")
    });
    router.add(Method::Get, "/readyz", move |_req: Request| {
        let health = health.clone();
        async move {
            if health.ready() {
                Response::text(StatusCode::Ok, "ready")
            } else {
                Response::text(StatusCode::InternalServerError, "not ready")
            }
        }
    });
}

fn add_config_route(
    router: &mut Router,
    route: &RouteConfig,
    static_dir: Option<String>,
) -> Result<()> {
    let method = Method::from(route.method.to_ascii_uppercase().as_str());
    let path = route.path.clone();
    if let Some(prefix) = route.static_prefix.clone() {
        let dir = static_dir
            .ok_or_else(|| NanowebError::Config("static route requires static_dir".into()))?;
        let files = StaticFiles::new(dir);
        router.add(method, &path, move |req: Request| {
            let files = files.clone();
            let prefix = prefix.clone();
            async move { files.serve(req, &prefix).await }
        });
        return Ok(());
    }
    if let Some(json) = route.json.clone() {
        router.add(method, &path, move |_req: Request| {
            let json = json.clone();
            async move { Response::json(&json) }
        });
    } else if let Some(tpl) = route.template.clone() {
        let ctx = route.context.clone().unwrap_or(serde_json::Value::Null);
        router.add(method, &path, move |_req: Request| {
            let tpl = tpl.clone();
            let ctx = ctx.clone();
            async move {
                match Renderer.render(&tpl, &ctx) {
                    Ok(html) => Response::html(StatusCode::Ok, html),
                    Err(e) => Response::text(StatusCode::InternalServerError, e.to_string()),
                }
            }
        });
    } else {
        let text = route.text.clone().unwrap_or_default();
        router.add(method, &path, move |_req: Request| {
            let text = text.clone();
            async move { Response::text(StatusCode::Ok, text) }
        });
    }
    Ok(())
}

pub async fn handle_connection<S>(
    stream: S,
    peer: SocketAddr,
    app: App,
    metrics: Arc<Metrics>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(stream);
    let parsed = parse_request(&mut reader).await;
    let stream = reader.get_mut();
    match parsed {
        Ok(mut req) => {
            req.peer_addr = Some(peer);
            let start = Instant::now();
            let method = req.method.clone();
            let request_id = req.request_id.clone();
            let (mut res, route, suppress_body) = app.handle(req).await;
            res.header("X-Request-Id", request_id);
            metrics.record(&method, res.status, &route, start.elapsed());
            stream.write_all(&res.to_bytes(suppress_body)).await?;
        }
        Err(err) => {
            let res = Response::text(StatusCode::BadRequest, format!("bad request: {err}"));
            stream.write_all(&res.to_bytes(false)).await?;
        }
    }
    stream.shutdown().await?;
    Ok(())
}

async fn wait_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async { if let Some(t) = &mut term { t.recv().await; } } => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_default_server() {
        let _ = super::Server::from_config(crate::config::Config::default()).unwrap();
    }
}
