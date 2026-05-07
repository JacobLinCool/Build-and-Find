use crate::{
    config::{Config, RouteConfig},
    error::{NanowebError, Result},
    health,
    http::{Method, Request, Response, parser::parse_request},
    middleware::{BasicAuth, Cors, Csrf, Gzip, RateLimit, RequestLogger},
    router::App,
    static_files::StaticFiles,
};
use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    task::JoinSet,
};

const READ_BUF: usize = 8192;

pub fn app_from_config(config: &Config) -> Result<App> {
    let mut app = App::new();
    let metrics = app.metrics();
    let metrics_path = config.metrics.clone();
    app.add_route(Method::Get, &metrics_path, move |_req: Request| {
        let metrics = metrics.clone();
        async move {
            let mut response = Response::text(200, metrics.render());
            response.set_header("Content-Type", "text/plain; version=0.0.4");
            response
        }
    });
    app.add_route(Method::Get, "/healthz", |_req: Request| async move {
        health::healthz()
    });
    let ready = app.ready_flag();
    app.add_route(Method::Get, "/readyz", move |_req: Request| {
        let ready = ready.clone();
        async move { health::readyz(&ready) }
    });

    if let Some(dir) = &config.static_dir {
        let static_files = StaticFiles::new(dir.clone());
        app.add_route(Method::Get, "/static/*path", move |req: Request| {
            let static_files = static_files.clone();
            async move { static_files.serve(&req, "path").await }
        });
    }

    for route in &config.routes {
        add_configured_route(&mut app, route)?;
    }

    let mw = &config.middleware;
    if mw.logging {
        app.add_middleware(RequestLogger);
    }
    if let Some(origins) = &mw.cors_allowed_origins {
        app.add_middleware(Cors::new(origins.clone()));
    }
    if let (Some(user), Some(pass)) = (&mw.basic_auth_username, &mw.basic_auth_password) {
        app.add_middleware(BasicAuth::new(user, pass));
    }
    if let Some(capacity) = mw.rate_limit_capacity {
        app.add_middleware(RateLimit::new(
            capacity,
            mw.rate_limit_refill_per_sec.unwrap_or(1.0),
        ));
    }
    if mw.csrf {
        app.add_middleware(Csrf);
    }
    if mw.gzip {
        app.add_middleware(Gzip::new());
    }
    Ok(app)
}

fn add_configured_route(app: &mut App, route: &RouteConfig) -> Result<()> {
    let method = route
        .method
        .to_ascii_uppercase()
        .parse::<Method>()
        .map_err(|_| NanowebError::Config(format!("invalid method {}", route.method)))?;
    let body = route.body.clone();
    let content_type = route.content_type.clone();
    let status = route.status;
    app.add_route(method, &route.path, move |_req: Request| {
        let body = body.clone();
        let content_type = content_type.clone();
        async move {
            let mut response = Response::new(status);
            response.set_header("Content-Type", content_type);
            response.body = body.into_bytes();
            response
        }
    });
    Ok(())
}

pub async fn run(config: Config) -> Result<()> {
    init_logging(&config.log_level);
    let app = Arc::new(app_from_config(&config)?);
    let shutdown = shutdown_signal();
    let timeout = Duration::from_secs(config.graceful_timeout_s);

    match (&config.tls_cert, &config.tls_key, &config.tls_listen) {
        (Some(cert), Some(key), Some(tls_addr)) => {
            let plain = TcpListener::bind(&config.listen).await?;
            let tls = TcpListener::bind(tls_addr).await?;
            let acceptor = crate::tls::load_acceptor(cert, key)?;
            let (tx, rx) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                shutdown.await;
                let _ = tx.send(true);
            });
            let plain_task =
                serve_with_shutdown(plain, app.clone(), wait_for_shutdown(rx.clone()), timeout);
            let tls_task =
                serve_tls_with_shutdown(tls, acceptor, app, wait_for_shutdown(rx), timeout);
            tokio::try_join!(plain_task, tls_task)?;
        }
        (Some(cert), Some(key), _) => {
            let listener = TcpListener::bind(&config.listen).await?;
            let acceptor = crate::tls::load_acceptor(cert, key)?;
            serve_tls_with_shutdown(listener, acceptor, app, shutdown, timeout).await?;
        }
        (None, None, _) => {
            let listener = TcpListener::bind(&config.listen).await?;
            serve_with_shutdown(listener, app, shutdown, timeout).await?;
        }
        _ => {
            return Err(NanowebError::Config(
                "both tls_cert and tls_key must be provided".into(),
            ));
        }
    }
    Ok(())
}

pub async fn serve_with_shutdown<S>(
    listener: TcpListener,
    app: Arc<App>,
    shutdown: S,
    graceful_timeout: Duration,
) -> Result<()>
where
    S: Future<Output = ()> + Send,
{
    let mut shutdown = Box::pin(shutdown);
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                app.ready_flag().store(false, std::sync::atomic::Ordering::SeqCst);
                break;
            }
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let app = app.clone();
                tasks.spawn(async move {
                    let _ = handle_connection(stream, app, peer).await;
                });
            }
        }
    }
    drain_tasks(tasks, graceful_timeout).await;
    Ok(())
}

#[cfg(feature = "tls")]
pub async fn serve_tls_with_shutdown<S>(
    listener: TcpListener,
    acceptor: crate::tls::TlsAcceptor,
    app: Arc<App>,
    shutdown: S,
    graceful_timeout: Duration,
) -> Result<()>
where
    S: Future<Output = ()> + Send,
{
    let mut shutdown = Box::pin(shutdown);
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                app.ready_flag().store(false, std::sync::atomic::Ordering::SeqCst);
                break;
            }
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let acceptor = acceptor.clone();
                let app = app.clone();
                tasks.spawn(async move {
                    if let Ok(stream) = acceptor.accept(stream).await {
                        let _ = handle_connection(stream, app, peer).await;
                    }
                });
            }
        }
    }
    drain_tasks(tasks, graceful_timeout).await;
    Ok(())
}

#[cfg(not(feature = "tls"))]
pub async fn serve_tls_with_shutdown<S>(
    _listener: TcpListener,
    _acceptor: crate::tls::TlsAcceptor,
    _app: Arc<App>,
    _shutdown: S,
    _graceful_timeout: Duration,
) -> Result<()>
where
    S: Future<Output = ()> + Send,
{
    Err(NanowebError::Tls(
        "TLS support is disabled at compile time".into(),
    ))
}

async fn drain_tasks(mut tasks: JoinSet<()>, graceful_timeout: Duration) {
    let deadline = tokio::time::Instant::now() + graceful_timeout;
    while !tasks.is_empty() {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            tasks.abort_all();
            break;
        }
        if tokio::time::timeout_at(deadline, tasks.join_next())
            .await
            .is_err()
        {
            tasks.abort_all();
            break;
        }
    }
}

pub async fn handle_connection<S>(mut stream: S, app: Arc<App>, peer: SocketAddr) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut bytes = Vec::new();
    let mut tmp = [0_u8; READ_BUF];
    let request = loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&tmp[..n]);
        match parse_request(&bytes) {
            Ok(Some((mut request, _used))) => {
                request.remote_addr = Some(peer);
                break request;
            }
            Ok(None) => continue,
            Err(_) => {
                let response = Response::text(400, "bad request");
                stream.write_all(&response.to_http_bytes(false)).await?;
                stream.shutdown().await?;
                return Ok(());
            }
        }
    };
    let suppress = request.method == Method::Head;
    let response = app.handle(request).await;
    stream.write_all(&response.to_http_bytes(suppress)).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sigterm) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            let _ = sigterm.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

async fn wait_for_shutdown(mut rx: tokio::sync::watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            break;
        }
    }
}

fn init_logging(level: &str) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(level)
        .json()
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn configured_route_works() {
        let mut cfg = Config::default();
        cfg.routes.push(RouteConfig {
            method: "GET".into(),
            path: "/hello".into(),
            status: 200,
            body: "world".into(),
            content_type: "text/plain".into(),
        });
        let app = app_from_config(&cfg).unwrap();
        let res = app.handle(Request::new(Method::Get, "/hello")).await;
        assert_eq!(res.body, b"world");
    }
}
