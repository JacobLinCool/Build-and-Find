use crate::config::Config;
use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::{Method, Request, Response, StatusCode};
use crate::metrics::{Metrics, MetricsTimer};
use crate::middleware::{run_middleware_after, run_middleware_before, BoxMiddleware};
use crate::router::Router;
use crate::static_files::StaticFileServer;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

/// The main server that ties everything together.
pub struct Server {
    config: Config,
    router: Arc<Router>,
    middleware: Arc<Vec<BoxMiddleware>>,
    metrics: Arc<Metrics>,
    health: Arc<HealthState>,
    static_server: Option<Arc<StaticFileServer>>,
    shutdown_notify: Arc<Notify>,
}

impl Server {
    pub fn new(
        config: Config,
        router: Router,
        middleware: Vec<BoxMiddleware>,
        metrics: Metrics,
    ) -> Self {
        let health = HealthState::new();
        let static_server = config
            .static_dir
            .as_ref()
            .map(|dir| Arc::new(StaticFileServer::new(dir)));

        Self {
            config,
            router: Arc::new(router),
            middleware: Arc::new(middleware),
            metrics: Arc::new(metrics),
            health: Arc::new(health),
            static_server,
            shutdown_notify: Arc::new(Notify::new()),
        }
    }

    /// Run the server, blocking until shutdown.
    pub async fn run(&self) -> Result<(), NanowebError> {
        let listener = TcpListener::bind(&self.config.listen).await?;
        tracing::info!("Listening on {}", self.config.listen);

        self.health.set_ready(true);

        // Graceful shutdown signal
        let health_clone = self.health.clone();

        tokio::select! {
            result = self.accept_loop(listener) => {
                if let Err(e) = result {
                    tracing::error!("Accept loop error: {}", e);
                }
            }
            _ = shutdown_signal() => {
                tracing::info!("Shutdown signal received, starting graceful shutdown");
                health_clone.set_ready(false);
            }
        }

        // Wait for in-flight requests with timeout
        let timeout = std::time::Duration::from_secs(self.config.graceful_timeout_s);
        let _ = tokio::time::timeout(timeout, async {
            // Give connections time to finish
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        })
        .await;

        tracing::info!("Server shut down cleanly");
        Ok(())
    }

    async fn accept_loop(&self, listener: TcpListener) -> Result<(), NanowebError> {
        loop {
            let (stream, addr) = listener.accept().await?;
            let router = self.router.clone();
            let middleware = self.middleware.clone();
            let metrics = self.metrics.clone();
            let health = self.health.clone();
            let static_server = self.static_server.clone();
            let config = self.config.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(
                    stream,
                    addr.to_string(),
                    router,
                    middleware,
                    metrics,
                    health,
                    static_server,
                    config,
                )
                .await
                {
                    tracing::debug!("Connection error: {}", e);
                }
            });
        }
    }

    /// Signal the server to shut down gracefully.
    pub fn shutdown(&self) {
        self.health.set_ready(false);
        self.shutdown_notify.notify_one();
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    _addr: String,
    router: Arc<Router>,
    middleware: Arc<Vec<BoxMiddleware>>,
    metrics: Arc<Metrics>,
    health: Arc<HealthState>,
    static_server: Option<Arc<StaticFileServer>>,
    config: Config,
) -> Result<(), NanowebError> {
    let mut buf = vec![0u8; 8192];
    let mut accumulated = Vec::new();

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        accumulated.extend_from_slice(&buf[..n]);

        // Try to parse the request
        match crate::http::parser::parse_request(&accumulated) {
            Ok(mut req) => {
                let request_id = uuid::Uuid::new_v4().to_string();
                req.request_id = request_id;

                let timer = MetricsTimer::start();
                let resp = dispatch_request(
                    &req,
                    &router,
                    &middleware,
                    &health,
                    &static_server,
                    &metrics,
                    &config,
                )
                .await;

                let duration = timer.elapsed_secs();
                let route_label = format!("{}:{}", req.method, req.path);
                metrics.record_request(req.method.as_str(), resp.status.0, &route_label, duration);

                let mut resp = resp;
                resp.headers.insert("x-request-id", &req.request_id);
                resp.headers.insert("connection", "keep-alive");

                let resp_bytes = resp.to_bytes();
                stream.write_all(&resp_bytes).await?;
                stream.flush().await?;

                // Reset for next request on keep-alive
                accumulated.clear();
            }
            Err(_) => {
                // If we haven't received the full headers yet, keep reading
                if accumulated.windows(4).any(|w| w == b"\r\n\r\n") {
                    // We have full headers but parsing failed - send 400
                    let resp = Response::text(StatusCode::BAD_REQUEST, "Bad Request");
                    let resp_bytes = resp.to_bytes();
                    stream.write_all(&resp_bytes).await?;
                    return Ok(());
                }
                // Otherwise keep reading
                if accumulated.len() > 16384 {
                    // Too much data without a valid request
                    let resp = Response::text(StatusCode::BAD_REQUEST, "Request too large");
                    let resp_bytes = resp.to_bytes();
                    stream.write_all(&resp_bytes).await?;
                    return Ok(());
                }
            }
        }
    }
}

async fn dispatch_request(
    req: &Request,
    router: &Router,
    middleware: &[BoxMiddleware],
    health: &HealthState,
    static_server: &Option<Arc<StaticFileServer>>,
    metrics: &Metrics,
    config: &Config,
) -> Response {
    // Health endpoints
    if req.path == "/healthz" {
        return health.healthz();
    }
    if req.path == "/readyz" {
        return health.readyz();
    }

    // Metrics endpoint
    if req.path == config.metrics {
        return Response {
            status: StatusCode::OK,
            headers: {
                let mut h = crate::http::HeaderMap::new();
                h.insert("content-type", "text/plain; version=0.0.4");
                h
            },
            body: metrics.render().into_bytes(),
        };
    }

    // Run middleware before
    let req = match run_middleware_before(middleware, req.clone()).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    // Try router first
    let method = req.method;
    let path = req.path.clone();

    // For HEAD, try GET handler; for OPTIONS, auto-list methods
    let result = if method == Method::Head {
        router
            .find(Method::Get, &path)
            .map(|(h, p)| (h, p, Method::Get))
    } else {
        router.find(method, &path).map(|(h, p)| (h, p, method))
    };

    let mut resp = if let Some((handler, params, _matched_method)) = result {
        let mut req = req.clone();
        req.params = params;
        let mut resp = handler(req).await;
        // Suppress body for HEAD requests
        if method == Method::Head {
            resp.body = Vec::new();
        }
        resp
    } else if method == Method::Options {
        // Auto-respond with allowed methods
        let allowed = router.allowed_methods(&path);
        if !allowed.is_empty() {
            let methods_str: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
            let mut resp = Response::new(StatusCode::NO_CONTENT);
            resp.headers.insert("allow", methods_str.join(", "));
            resp
        } else {
            // Try static files
            if let Some(ref static_server) = static_server {
                if let Some(resp) = static_server.serve(&req).await {
                    resp
                } else {
                    Response::text(StatusCode::NOT_FOUND, "Not Found")
                }
            } else {
                Response::text(StatusCode::NOT_FOUND, "Not Found")
            }
        }
    } else {
        // Check allowed methods for 405
        let allowed = router.allowed_methods(&path);
        if !allowed.is_empty() {
            let methods_str: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
            let mut resp = Response::text(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed");
            resp.headers.insert("allow", methods_str.join(", "));
            resp
        } else {
            // Try static files
            if let Some(ref static_server) = static_server {
                if let Some(resp) = static_server.serve(&req).await {
                    resp
                } else {
                    Response::text(StatusCode::NOT_FOUND, "Not Found")
                }
            } else {
                Response::text(StatusCode::NOT_FOUND, "Not Found")
            }
        }
    };

    // Run middleware after
    resp = run_middleware_after(middleware, &req, resp).await;
    resp
}

/// Wait for SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();

        tokio::select! {
            _ = ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::metrics::Metrics;
    use crate::router::Router;

    #[tokio::test]
    async fn test_dispatch_healthz() {
        let health = HealthState::new();
        let metrics = Metrics::new();
        let config = Config::default();

        let req = Request {
            method: Method::Get,
            target: "/healthz".into(),
            path: "/healthz".into(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: "test".into(),
            session_data: Default::default(),
        };

        let resp =
            dispatch_request(&req, &Router::new(), &[], &health, &None, &metrics, &config).await;

        assert_eq!(resp.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dispatch_readyz_not_ready() {
        let health = HealthState::new();
        let metrics = Metrics::new();
        let config = Config::default();

        let req = Request {
            method: Method::Get,
            target: "/readyz".into(),
            path: "/readyz".into(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: "test".into(),
            session_data: Default::default(),
        };

        let resp =
            dispatch_request(&req, &Router::new(), &[], &health, &None, &metrics, &config).await;

        assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_dispatch_not_found() {
        let health = HealthState::new();
        let metrics = Metrics::new();
        let config = Config::default();

        let req = Request {
            method: Method::Get,
            target: "/nope".into(),
            path: "/nope".into(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: "test".into(),
            session_data: Default::default(),
        };

        let resp =
            dispatch_request(&req, &Router::new(), &[], &health, &None, &metrics, &config).await;

        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dispatch_metrics() {
        let health = HealthState::new();
        let metrics = Metrics::new();
        let config = Config::default();

        let req = Request {
            method: Method::Get,
            target: "/metrics".into(),
            path: "/metrics".into(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: "test".into(),
            session_data: Default::default(),
        };

        let resp =
            dispatch_request(&req, &Router::new(), &[], &health, &None, &metrics, &config).await;

        assert_eq!(resp.status, StatusCode::OK);
        assert!(String::from_utf8_lossy(&resp.body).contains("nanoweb"));
    }
}
