use crate::{
    app::App,
    config::Config,
    error::{NanowebError, Result},
    http::{
        parser::{parse_request, ParseError, ParsedRequest},
        types::{Method, Request, Response, StatusCode},
    },
    tls,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    task::JoinSet,
    time::timeout,
};

pub async fn run(config: Config) -> Result<()> {
    let app = Arc::new(App::from_config(config.clone())?);
    app.state.ready.store(true, Ordering::SeqCst);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_notify = Arc::new(Notify::new());
    let active_connections = Arc::new(AtomicUsize::new(0));
    let drained_notify = Arc::new(Notify::new());
    let mut tasks = JoinSet::new();

    let listener = TcpListener::bind(config.listen).await?;
    tasks.spawn({
        let app = app.clone();
        let shutdown = shutdown.clone();
        let shutdown_notify = shutdown_notify.clone();
        let active_connections = active_connections.clone();
        let drained_notify = drained_notify.clone();
        async move {
            accept_loop(
                listener,
                app,
                shutdown,
                shutdown_notify,
                active_connections,
                drained_notify,
            )
            .await
        }
    });

    #[cfg(feature = "tls")]
    if let (Some(cert), Some(key), Some(tls_listen)) = (
        config.tls_cert.clone(),
        config.tls_key.clone(),
        config.tls_listen,
    ) {
        let acceptor = tls::load_acceptor(&cert, &key)?;
        let listener = TcpListener::bind(tls_listen).await?;
        tasks.spawn({
            let app = app.clone();
            let shutdown = shutdown.clone();
            let shutdown_notify = shutdown_notify.clone();
            let active_connections = active_connections.clone();
            let drained_notify = drained_notify.clone();
            async move {
                accept_loop_tls(
                    listener,
                    acceptor,
                    app,
                    shutdown,
                    shutdown_notify,
                    active_connections,
                    drained_notify,
                )
                .await
            }
        });
    }

    #[cfg(not(feature = "tls"))]
    if config.tls_cert.is_some() || config.tls_key.is_some() {
        return Err(NanowebError::Tls(
            "tls support is disabled at compile time; rebuild with the tls feature enabled"
                .to_string(),
        ));
    }

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|err| {
                NanowebError::Other(format!("failed to install SIGTERM handler: {err}"))
            })?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.map_err(|err| {
            NanowebError::Other(format!("failed to install ctrl-c handler: {err}"))
        })?;
    }

    shutdown.store(true, Ordering::SeqCst);
    shutdown_notify.notify_waiters();
    app.state.ready.store(false, Ordering::SeqCst);

    let grace = tokio::time::Duration::from_secs(config.graceful_timeout_s);
    let wait_for_shutdown = async {
        while tasks.join_next().await.is_some() {}
        loop {
            if active_connections.load(Ordering::SeqCst) == 0 {
                break;
            }
            drained_notify.notified().await;
        }
    };
    let _ = timeout(grace, wait_for_shutdown).await;
    Ok(())
}

async fn accept_loop(
    listener: TcpListener,
    app: Arc<App>,
    shutdown: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    active_connections: Arc<AtomicUsize>,
    drained_notify: Arc<Notify>,
) -> Result<()> {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = shutdown_notify.notified() => {
                break;
            }
            result = listener.accept() => {
                let (stream, addr) = match result {
                    Ok(pair) => pair,
                    Err(err) => {
                        if shutdown.load(Ordering::SeqCst) {
                            break;
                        }
                        return Err(err.into());
                    }
                };
                active_connections.fetch_add(1, Ordering::SeqCst);
                let app = app.clone();
                let active_connections = active_connections.clone();
                let drained_notify = drained_notify.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, addr, app).await;
                    if active_connections.fetch_sub(1, Ordering::SeqCst) == 1 {
                        drained_notify.notify_waiters();
                    }
                });
            }
        }
    }
    Ok(())
}

#[cfg(feature = "tls")]
async fn accept_loop_tls(
    listener: TcpListener,
    acceptor: tls::TlsAcceptor,
    app: Arc<App>,
    shutdown: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    active_connections: Arc<AtomicUsize>,
    drained_notify: Arc<Notify>,
) -> Result<()> {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = shutdown_notify.notified() => {
                break;
            }
            result = listener.accept() => {
                let (stream, addr) = match result {
                    Ok(pair) => pair,
                    Err(err) => {
                        if shutdown.load(Ordering::SeqCst) {
                            break;
                        }
                        return Err(err.into());
                    }
                };
                let acceptor = acceptor.clone();
                active_connections.fetch_add(1, Ordering::SeqCst);
                let app = app.clone();
                let active_connections = active_connections.clone();
                let drained_notify = drained_notify.clone();
                tokio::spawn(async move {
                    if let Ok(stream) = acceptor.accept(stream).await {
                        let _ = handle_connection(stream, addr, app).await;
                    }
                    if active_connections.fetch_sub(1, Ordering::SeqCst) == 1 {
                        drained_notify.notify_waiters();
                    }
                });
            }
        }
    }
    Ok(())
}

async fn handle_connection<S>(mut stream: S, addr: SocketAddr, app: Arc<App>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = Vec::new();
    let mut temp = [0u8; 4096];
    loop {
        match parse_request(&buffer) {
            Ok((parsed, consumed)) => {
                buffer.drain(..consumed);
                let request_id = generate_request_id();
                let request = build_request(parsed, addr, request_id.clone());
                let started = Instant::now();
                let mut response = match app.handle(request.clone()).await {
                    Ok(response) => response,
                    Err(err) => Response::text(StatusCode::InternalServerError, err.to_string()),
                };
                response.set_header("x-request-id", request_id.clone());
                app.state.metrics.observe_request(
                    request.method,
                    response.status,
                    request.route_pattern.as_deref().unwrap_or("unmatched"),
                    started.elapsed(),
                );
                let bytes =
                    response.into_http_bytes(Some(&request_id), request.method == Method::HEAD);
                stream.write_all(&bytes).await?;
                if request.connection_close {
                    break;
                }
            }
            Err(ParseError::Incomplete) => {
                let n = stream.read(&mut temp).await?;
                if n == 0 {
                    break;
                }
                buffer.extend_from_slice(&temp[..n]);
                continue;
            }
            Err(ParseError::Protocol(err)) => {
                let response = Response::text(StatusCode::BadRequest, err);
                let bytes = response.into_http_bytes(None, false);
                let _ = stream.write_all(&bytes).await;
                break;
            }
        }
    }
    Ok(())
}

fn build_request(parsed: ParsedRequest, remote_addr: SocketAddr, request_id: String) -> Request {
    Request {
        method: parsed.method,
        target: parsed.target,
        path: parsed.path,
        query: parsed.query,
        version: parsed.version,
        headers: parsed.headers,
        body: parsed.body,
        params: HashMap::new(),
        request_id,
        remote_addr: Some(remote_addr),
        route_pattern: None,
        connection_close: parsed.connection_close,
    }
}

fn generate_request_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}
