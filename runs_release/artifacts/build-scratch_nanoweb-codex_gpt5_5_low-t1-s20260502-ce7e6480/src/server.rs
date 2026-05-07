use crate::config::Config;
use crate::error::Result;
use crate::health::Health;
use crate::http::{Method, Request, Response, StatusCode, parser};
use crate::metrics::Metrics;
use crate::middleware::{
    Stack, auth::BasicAuth, cors::Cors, csrf::Csrf, gzip::Gzip, logging::Logging,
    rate_limit::RateLimit,
};
use crate::router::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinSet;

#[derive(Clone)]
pub struct Server {
    config: Config,
    app: Arc<App>,
}

pub(crate) struct App {
    router: Router,
    stack: Stack,
    metrics: Metrics,
    health: Health,
    metrics_path: String,
}

impl Server {
    pub fn from_config(config: Config) -> Result<Self> {
        let mut router = Router::new();
        for route in &config.routes {
            let body = route.response.clone();
            router.add(
                Method::from(route.method.as_str()),
                &route.path,
                move |_| {
                    let body = body.clone();
                    async move { Ok(Response::text(body)) }
                },
            );
        }
        if let Some(dir) = config.static_dir.clone() {
            router.add(Method::Get, "/static/*path", move |req| {
                let dir = dir.clone();
                async move {
                    crate::static_files::serve(dir, &req, "path")
                        .await
                        .or_else(|_| Ok(Response::new(StatusCode::NOT_FOUND, "not found")))
                }
            });
        }
        let mut stack = Stack::new();
        let mw = &config.middleware;
        if mw.logging {
            stack.push(Logging);
        }
        if mw.gzip {
            stack.push(Gzip);
        }
        if !mw.cors_origins.is_empty() {
            stack.push(Cors::new(mw.cors_origins.clone()));
        }
        if mw.csrf {
            stack.push(Csrf);
        }
        if let (Some(u), Some(p)) = (mw.basic_auth_user.clone(), mw.basic_auth_password.clone()) {
            stack.push(BasicAuth::new(u, p));
        }
        if let Some(cap) = mw.rate_limit_capacity {
            stack.push(RateLimit::new(
                cap,
                mw.rate_limit_refill_per_s.unwrap_or(1.0),
            ));
        }
        Ok(Self {
            config: config.clone(),
            app: Arc::new(App {
                router,
                stack,
                metrics: Metrics::new()?,
                health: Health::default(),
                metrics_path: config.metrics.clone(),
            }),
        })
    }

    pub async fn run(self) -> Result<()> {
        let shutdown = Arc::new(Notify::new());
        install_shutdown_signal(shutdown.clone());
        self.run_until(shutdown).await
    }

    pub async fn run_until(self, shutdown: Arc<Notify>) -> Result<()> {
        let listener = TcpListener::bind(&self.config.listen).await?;
        self.app.health.set_ready(true);
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                res = listener.accept() => {
                    let (stream, addr) = res?;
                    let app = self.app.clone();
                    tasks.spawn(async move {
                        let _ = handle_connection(stream, app, Some(addr)).await;
                    });
                }
                _ = shutdown.notified() => {
                    self.app.health.set_ready(false);
                    break;
                }
            }
        }
        let timeout = std::time::Duration::from_secs(self.config.graceful_timeout_s);
        let _ = tokio::time::timeout(timeout, async {
            while tasks.join_next().await.is_some() {}
        })
        .await;
        Ok(())
    }
}

pub(crate) async fn handle_connection<S>(
    mut stream: S,
    app: Arc<App>,
    addr: Option<SocketAddr>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let parsed = parser::read_request(&mut stream).await;
    let (response, suppress) = match parsed {
        Ok(mut req) => {
            if let Some(addr) = addr {
                req.client_ip = Some(addr.ip().to_string());
            }
            let suppress = matches!(req.method, Method::Head);
            let started = Instant::now();
            let method = req.method.clone();
            let route = req.path().to_string();
            let request_id = req.request_id.clone();
            let res = dispatch(app.clone(), req).await.unwrap_or_else(|e| {
                Response::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            });
            app.metrics.observe(&method, &route, res.status, started);
            let mut res = res;
            res.headers.insert("x-request-id".into(), request_id);
            (res, suppress)
        }
        Err(_) => (Response::new(StatusCode::BAD_REQUEST, "bad request"), false),
    };
    stream.write_all(&response.serialize(suppress)).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn dispatch(app: Arc<App>, req: Request) -> Result<Response> {
    if req.path() == "/healthz" {
        return Ok(app.health.healthz());
    }
    if req.path() == "/readyz" {
        return Ok(app.health.readyz());
    }
    if req.path() == app.metrics_path {
        return Ok(app.metrics.response());
    }
    let app_for_handler = app.clone();
    app.stack
        .run(
            req,
            |r| async move { app_for_handler.router.route(r).await },
        )
        .await
}

fn install_shutdown_signal(shutdown: Arc<Notify>) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install sigterm");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        shutdown.notify_waiters();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_constructs() {
        let cfg = Config {
            listen: "127.0.0.1:0".into(),
            tls_cert: None,
            tls_key: None,
            metrics: "/metrics".into(),
            static_dir: None,
            log_level: "info".into(),
            graceful_timeout_s: 1,
            session_secret: "s".into(),
            routes: vec![],
            middleware: Default::default(),
        };
        assert!(Server::from_config(cfg).is_ok());
    }
}
