use crate::config::Config;
use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::parser::parse_request;
use crate::http::request::Request;
use crate::http::response::Response;
use crate::metrics::Metrics;
use crate::middleware::MiddlewareChain;
use crate::router::Router;
use crate::session::SessionManager;
use crate::static_files::StaticFileServer;
use crate::template::Renderer;
use bytes::{Buf, BytesMut};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::watch;
use tokio::time::Duration;

pub struct Server {
    config: Config,
    router: Router,
    middleware: MiddlewareChain,
    metrics: Metrics,
    health: HealthState,
    session_manager: Option<SessionManager>,
    static_server: Option<StaticFileServer>,
}

impl Server {
    pub fn new(config: Config) -> Self {
        let metrics = Metrics::new();
        let health = HealthState::new();
        let mut router = Router::new();

        for route in &config.routes {
            let handler = create_handler_from_config(route);
            router
                .add_route(&route.method, &route.path, Arc::new(handler))
                .unwrap();
        }

        let health_state = health.clone();
        router
            .add_route(
                "GET",
                "/healthz",
                Arc::new(move |_req| health_state.healthz()),
            )
            .unwrap();

        let health_state = health.clone();
        router
            .add_route(
                "GET",
                "/readyz",
                Arc::new(move |_req| health_state.readyz()),
            )
            .unwrap();

        let metrics_for_route = metrics.clone();
        router
            .add_route(
                "GET",
                &config.metrics_path,
                Arc::new(move |_req| crate::metrics::metrics_handler(&metrics_for_route)),
            )
            .unwrap();

        let mut mws: Vec<Box<dyn crate::middleware::Middleware>> = Vec::new();
        mws.push(Box::new(
            crate::middleware::logging::LoggingMiddleware::new(),
        ));
        mws.push(Box::new(crate::middleware::cors::CorsMiddleware::new(
            config.cors.allowed_origins.clone(),
        )));
        mws.push(Box::new(
            crate::middleware::rate_limit::RateLimitMiddleware::new(
                config.rate_limit.requests_per_second,
                config.rate_limit.burst,
            ),
        ));
        let middleware = MiddlewareChain::from_vec(mws);

        let session_manager = config.session_secret.as_ref().map(|secret| {
            SessionManager::new(
                Arc::new(crate::session::InMemorySessionStore::new()),
                secret,
            )
        });

        let static_server = config
            .static_dir
            .as_ref()
            .and_then(|dir| StaticFileServer::new(dir).ok());

        Server {
            config,
            router,
            middleware,
            metrics,
            health,
            session_manager,
            static_server,
        }
    }

    pub async fn run(&self) -> Result<(), NanowebError> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            let _ = signal::ctrl_c().await;
            eprintln!("[nanoweb] received shutdown signal");
            let _ = tx.send(true);
        });

        let addr: SocketAddr = self
            .config
            .listen
            .parse()
            .map_err(|e| NanowebError::Config(format!("invalid listen address: {e}")))?;

        let listener = TcpListener::bind(addr).await?;
        eprintln!("[nanoweb] listening on {}", self.config.listen);

        self.health.mark_started();

        #[cfg(feature = "tls")]
        {
            let tls_acceptor = if let (Some(ref cert), Some(ref key)) =
                (&self.config.tls_cert, &self.config.tls_key)
            {
                Some(crate::tls::create_tls_acceptor(cert, key)?)
            } else {
                None
            };
            self.run_with_tls(listener, shutdown_rx, tls_acceptor).await
        }

        #[cfg(not(feature = "tls"))]
        {
            if self.config.tls_cert.is_some() {
                return Err(NanowebError::Tls(
                    "TLS cert/key specified but TLS feature is disabled".into(),
                ));
            }
            self.run_plain(listener, shutdown_rx).await
        }
    }

    #[cfg(not(feature = "tls"))]
    async fn run_plain(
        &self,
        listener: TcpListener,
        mut shutdown_rx: watch::Receiver<bool>,
    ) -> Result<(), NanowebError> {
        let router = Arc::new(self.router.clone());
        let middleware = Arc::new(self.middleware.clone());
        let metrics = Arc::new(self.metrics.clone());
        let health = Arc::new(self.health.clone());
        let session_manager = self.session_manager.clone();
        let static_server = self.static_server.clone();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let router = router.clone();
                            let middleware = middleware.clone();
                            let metrics = metrics.clone();
                            let health = health.clone();
                            let session_manager = session_manager.clone();
                            let static_server = static_server.clone();

                            tokio::spawn(async move {
                                handle_connection(
                                    stream,
                                    addr,
                                    router,
                                    middleware,
                                    metrics,
                                    health,
                                    static_server,
                                    &session_manager,
                                ).await;
                            });
                        }
                        Err(e) => {
                            eprintln!("[nanoweb] accept error: {e}");
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        eprintln!("[nanoweb] shutting down gracefully...");
                        self.health.mark_shutting_down();
                        tokio::time::sleep(Duration::from_secs(self.config.graceful_timeout_s)).await;
                        eprintln!("[nanoweb] shutdown complete");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "tls")]
    async fn run_with_tls(
        &self,
        listener: TcpListener,
        mut shutdown_rx: watch::Receiver<bool>,
        tls_acceptor: Option<Arc<tokio_rustls::TlsAcceptor>>,
    ) -> Result<(), NanowebError> {
        let router = Arc::new(self.router.clone());
        let middleware = Arc::new(self.middleware.clone());
        let metrics = Arc::new(self.metrics.clone());
        let health = Arc::new(self.health.clone());
        let session_manager = self.session_manager.clone();
        let static_server = self.static_server.clone();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let router = router.clone();
                            let middleware = middleware.clone();
                            let metrics = metrics.clone();
                            let health = health.clone();
                            let session_manager = session_manager.clone();
                            let static_server = static_server.clone();
                            let tls_acceptor = tls_acceptor.clone();

                            tokio::spawn(async move {
                                if let Some(ref acceptor) = tls_acceptor {
                                    match acceptor.accept(stream).await {
                                        Ok(tls_stream) => {
                                            handle_connection(
                                                tls_stream,
                                                addr,
                                                router,
                                                middleware,
                                                metrics,
                                                health,
                                                static_server,
                                                &session_manager,
                                            ).await;
                                        }
                                        Err(e) => {
                                            eprintln!("[nanoweb] TLS error: {e}");
                                        }
                                    }
                                } else {
                                    handle_connection(
                                        stream,
                                        addr,
                                        router,
                                        middleware,
                                        metrics,
                                        health,
                                        static_server,
                                        &session_manager,
                                    ).await;
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("[nanoweb] accept error: {e}");
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        eprintln!("[nanoweb] shutting down gracefully...");
                        self.health.mark_shutting_down();
                        tokio::time::sleep(Duration::from_secs(self.config.graceful_timeout_s)).await;
                        eprintln!("[nanoweb] shutdown complete");
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}

async fn handle_connection<S>(
    mut stream: S,
    addr: SocketAddr,
    router: Arc<Router>,
    middleware: Arc<MiddlewareChain>,
    metrics: Arc<Metrics>,
    _health: Arc<HealthState>,
    static_server: Option<StaticFileServer>,
    _session_manager: &Option<SessionManager>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut buf = BytesMut::with_capacity(4096);

    loop {
        let _n = match stream.read_buf(&mut buf).await {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => return,
        };

        match parse_request(&buf) {
            Ok(Some(raw)) => {
                let header_end = find_header_end_in_buf(&buf).unwrap_or(buf.len());
                let body_start = header_end + 4;
                let total_consumed = body_start + raw.body.len();
                buf.advance(total_consumed);

                let remote = Some(addr.to_string());
                let mut request = Request::from_raw(raw, remote);
                request.request_id = format!(
                    "{}-{}",
                    chrono::Utc::now().timestamp_millis(),
                    &request.request_id[..8.min(request.request_id.len())]
                );

                let method_str = request.method.as_str().to_string();
                let path = request.path.clone();

                let start = std::time::Instant::now();

                let request_id = request.request_id.clone();

                let response = if let Some(result) = router.lookup(&method_str, &path) {
                    request.params = result.params;
                    let handler = result.handler;
                    let mw = middleware.clone();
                    let static_server = static_server.clone();

                    mw.execute(
                        request,
                        Box::new(move |req| {
                            if let Some(ref ss) = static_server {
                                let resp = ss.serve(&req);
                                if resp.status != 404 {
                                    return resp;
                                }
                            }
                            handler(req)
                        }),
                    )
                } else if let Some(ref ss) = static_server {
                    let resp = ss.serve(&request);
                    if resp.status != 404 {
                        let ss = ss.clone();
                        middleware.execute(request, Box::new(move |req| ss.serve(&req)))
                    } else {
                        let mut resp = Response::not_found();
                        resp.set_header("x-request-id", &request_id);
                        resp
                    }
                } else {
                    let mut resp = Response::not_found();
                    resp.set_header("x-request-id", &request_id);
                    resp
                };

                let duration = start.elapsed().as_secs_f64();
                metrics.record_request(&method_str, &path, response.status, duration);

                let mut resp = response;
                resp.set_header("x-request-id", &request_id);

                if method_str == "HEAD" {
                    let mut head_resp = Response::new(resp.status);
                    for (k, v) in &resp.headers {
                        head_resp.headers.insert(k.clone(), v.clone());
                    }
                    head_resp.body.clear();
                    resp = head_resp;
                }

                if method_str == "OPTIONS" && resp.status == 404 {
                    let methods = router.allowed_methods(&path);
                    if !methods.is_empty() {
                        resp = Response::new(204);
                        resp.headers.insert("allow".into(), methods.join(", "));
                        resp.headers.insert("content-length".into(), "0".into());
                    }
                }

                let response_bytes = resp.to_http_bytes();
                let _ = stream.write_all(&response_bytes).await;
                let _ = stream.flush().await;
                return;
            }
            Ok(None) => {
                if buf.len() > 8192 {
                    let resp = Response::bad_request("Request too large");
                    let _ = stream.write_all(&resp.to_http_bytes()).await;
                    return;
                }
            }
            Err(e) => {
                let resp = Response::bad_request(&e.to_string());
                let _ = stream.write_all(&resp.to_http_bytes()).await;
                return;
            }
        }
    }
}

fn find_header_end_in_buf(buf: &BytesMut) -> Option<usize> {
    let data = buf.as_ref();
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some(i);
        }
    }
    None
}

fn create_handler_from_config(
    route: &crate::config::RouteConfig,
) -> Box<dyn Fn(Request) -> Response + Send + Sync> {
    let handler_name = route.handler.clone();
    let template = route.template.clone();

    Box::new(move |req: Request| match handler_name.as_str() {
        "echo" => {
            let body = req.body_string().unwrap_or_default();
            Response::text(&body)
        }
        "hello" => {
            let name = req.query.get("name").map(|s| s.as_str()).unwrap_or("World");
            Response::text(&format!("Hello, {name}!"))
        }
        "json_echo" => Response::json(&serde_json::json!({
            "method": req.method.as_str(),
            "path": req.path,
            "headers": req.headers,
        })),
        _ => {
            if let Some(ref tmpl) = template {
                let renderer = Renderer::new();
                let context = serde_json::json!({
                    "method": req.method.as_str(),
                    "path": req.path,
                });
                match renderer.render(tmpl, &context) {
                    Ok(html) => Response::html(&html),
                    Err(e) => Response::text(&format!("Template error: {e}")),
                }
            } else {
                Response::text("OK")
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = Config::default_config();
        let server = Server::new(config);
        assert_eq!(server.config.listen, "127.0.0.1:8080");
    }
}
