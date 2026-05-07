use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::{error, info};

use crate::config::Config;
use crate::error::{NanowebError, Result};
use crate::health::Health;
use crate::http::parser::read_request;
use crate::http::{Method, Request, Response, StatusCode};
use crate::metrics::{Metrics, MetricsMiddleware};
use crate::middleware::MiddlewareStack;
use crate::middleware::basic_auth::BasicAuth;
use crate::middleware::cors::Cors;
use crate::middleware::csrf::CsrfProtection;
use crate::middleware::gzip::GzipCompression;
use crate::middleware::logging::RequestLogging;
use crate::middleware::rate_limit::RateLimit;
use crate::router::Router;
use crate::static_files::StaticFiles;

#[derive(Clone)]
pub struct App {
    router: Arc<Router>,
    middleware: MiddlewareStack,
    health: Health,
}

impl App {
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut router = Router::new();
        let metrics = Metrics::new();
        let health = Health::new();

        router.add(Method::Get, &config.metrics, metrics.handler());
        router.add(Method::Get, "/healthz", health.healthz());
        router.add(Method::Get, "/readyz", health.readyz());

        if let Some(root) = &config.static_dir {
            let files = StaticFiles::new(root.clone());
            router.add(Method::Get, "/static/*path", move |req: Request| {
                let files = files.clone();
                async move { files.serve(req).await }
            });
        }

        for route in &config.routes {
            let method = Method::from_str(&route.method).map_err(NanowebError::Parse)?;
            let body = route.response.clone();
            let status = StatusCode(route.status);
            let content_type = route.content_type.clone();
            router.add(method, &route.path, move |_req: Request| {
                let body = body.clone();
                let content_type = content_type.clone();
                async move {
                    let mut response = Response::text(status, body);
                    response.set_header("Content-Type", content_type);
                    Ok(response)
                }
            });
        }

        let mut middleware = MiddlewareStack::new();
        if config.middleware.logging {
            middleware.push(RequestLogging);
        }
        middleware.push(MetricsMiddleware::new(metrics));
        if let Some(auth) = &config.middleware.basic_auth {
            middleware.push(BasicAuth::new(auth.username.clone(), auth.password.clone()));
        }
        if let Some(cors) = &config.middleware.cors {
            middleware.push(Cors::new(cors.allowed_origins.clone()));
        }
        if let Some(limit) = &config.middleware.rate_limit {
            middleware.push(RateLimit::new(limit.capacity, limit.refill_per_second));
        }
        if config.middleware.csrf {
            middleware.push(CsrfProtection);
        }
        if config.middleware.gzip {
            middleware.push(GzipCompression);
        }

        Ok(Self {
            router: Arc::new(router),
            middleware,
            health,
        })
    }

    pub async fn handle(&self, request: Request) -> Result<Response> {
        self.middleware.handle(&self.router, request).await
    }

    pub fn set_ready(&self, ready: bool) {
        self.health.set_ready(ready);
    }
}

pub async fn run(config: Config) -> Result<()> {
    init_logging(&config.log_level);
    let app = App::from_config(&config)?;
    let listener = TcpListener::bind(&config.listen).await?;
    #[cfg(feature = "tls")]
    let tls_acceptor = if config.tls_cert.is_some() || config.tls_key.is_some() {
        let cert = config
            .tls_cert
            .as_deref()
            .ok_or_else(|| NanowebError::Tls("missing TLS certificate path".into()))?;
        let key = config
            .tls_key
            .as_deref()
            .ok_or_else(|| NanowebError::Tls("missing TLS key path".into()))?;
        Some(crate::tls::load_acceptor(cert, key)?)
    } else {
        None
    };
    #[cfg(not(feature = "tls"))]
    if config.tls_cert.is_some() || config.tls_key.is_some() {
        return Err(NanowebError::Tls(
            "TLS support is disabled; rebuild with the tls feature".into(),
        ));
    }
    info!(listen = %config.listen, "nanoweb_listening");
    app.set_ready(true);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let signal_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = signal_tx.send(true);
    });

    let mut tasks = JoinSet::new();
    #[cfg(feature = "tls")]
    if let Some(acceptor) = tls_acceptor {
        accept_loop_tls(
            listener,
            acceptor,
            app.clone(),
            shutdown_rx.clone(),
            &mut tasks,
        )
        .await?;
    } else {
        accept_loop(listener, app.clone(), shutdown_rx.clone(), &mut tasks).await?;
    }
    #[cfg(not(feature = "tls"))]
    accept_loop(listener, app.clone(), shutdown_rx.clone(), &mut tasks).await?;
    app.set_ready(false);

    let deadline = tokio::time::sleep(Duration::from_secs(config.graceful_timeout_s));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(err)) = result {
                    error!(error = %err, "connection_task_failed");
                }
            }
            _ = &mut deadline => {
                tasks.abort_all();
                break;
            }
            else => break,
        }
    }
    Ok(())
}

pub async fn accept_loop(
    listener: TcpListener,
    app: App,
    mut shutdown: watch::Receiver<bool>,
    tasks: &mut JoinSet<()>,
) -> Result<()> {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => {
                break;
            }
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let app = app.clone();
                tasks.spawn(async move {
                    if let Err(err) = handle_connection(stream, app, peer).await {
                        error!(error = %err, "connection_failed");
                    }
                });
            }
        }
    }
    Ok(())
}

#[cfg(feature = "tls")]
pub async fn accept_loop_tls(
    listener: TcpListener,
    acceptor: crate::tls::TlsAcceptor,
    app: App,
    mut shutdown: watch::Receiver<bool>,
    tasks: &mut JoinSet<()>,
) -> Result<()> {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let app = app.clone();
                let acceptor = acceptor.clone();
                tasks.spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(stream) => {
                            if let Err(err) = handle_connection(stream, app, peer).await {
                                error!(error = %err, "connection_failed");
                            }
                        }
                        Err(err) => error!(error = %err, "tls_accept_failed"),
                    }
                });
            }
        }
    }
    Ok(())
}

pub async fn handle_connection<S>(mut stream: S, app: App, peer: SocketAddr) -> Result<()>
where
    S: tokio::io::AsyncRead + AsyncWrite + Unpin,
{
    loop {
        match read_request(&mut stream).await {
            Ok(Some(mut request)) => {
                request.peer_addr = Some(peer);
                let keep_alive = !request
                    .header("connection")
                    .is_some_and(|v| v.eq_ignore_ascii_case("close"));
                let bytes = match app.handle(request).await {
                    Ok(response) => response.to_http_bytes(keep_alive),
                    Err(err) => {
                        error!(error = %err, "handler_error");
                        Response::text(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                            .to_http_bytes(false)
                    }
                };
                stream.write_all(&bytes).await?;
                stream.flush().await?;
                if !keep_alive {
                    break;
                }
            }
            Ok(None) => break,
            Err(err) => {
                let response = Response::text(StatusCode::BAD_REQUEST, "bad request");
                stream.write_all(&response.to_http_bytes(false)).await?;
                stream.flush().await?;
                return Err(err);
            }
        }
    }
    Ok(())
}

#[cfg(feature = "tls")]
pub async fn run_tls_listener(config: &Config, app: App) -> Result<()> {
    let cert = config
        .tls_cert
        .as_deref()
        .ok_or_else(|| NanowebError::Tls("missing TLS certificate path".into()))?;
    let key = config
        .tls_key
        .as_deref()
        .ok_or_else(|| NanowebError::Tls("missing TLS key path".into()))?;
    let acceptor = crate::tls::load_acceptor(cert, key)?;
    let listener = TcpListener::bind(&config.listen).await?;
    loop {
        let (stream, peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls) => {
                    let _ = handle_connection(tls, app, peer).await;
                }
                Err(err) => error!(error = %err, "tls_accept_failed"),
            }
        });
    }
}

#[cfg(not(feature = "tls"))]
pub async fn run_tls_listener(_config: &Config, _app: App) -> Result<()> {
    Err(NanowebError::Tls(
        "TLS support is disabled; rebuild with the tls feature".into(),
    ))
}

fn init_logging(level: &str) {
    let filter = tracing_subscriber::EnvFilter::try_new(level).unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .try_init();
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, duplex};

    #[tokio::test]
    async fn app_serves_configured_route() {
        let config = Config::default();
        let app = App::from_config(&config).unwrap();
        let res = app.handle(Request::new(Method::Get, "/")).await.unwrap();
        assert_eq!(res.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn connection_handles_one_request() {
        let config = Config::default();
        let app = App::from_config(&config).unwrap();
        let (mut client, server) = duplex(4096);
        let peer = "127.0.0.1:12345".parse().unwrap();
        tokio::spawn(async move {
            handle_connection(server, app, peer).await.unwrap();
        });
        client
            .write_all(b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut out = String::new();
        client.read_to_string(&mut out).await.unwrap();
        assert!(out.contains("200 OK"));
    }
}
