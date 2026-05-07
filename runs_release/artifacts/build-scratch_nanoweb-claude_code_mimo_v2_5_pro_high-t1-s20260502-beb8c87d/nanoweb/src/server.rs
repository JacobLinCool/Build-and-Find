use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::config::Config;
use crate::health::HealthState;
use crate::http::types::{Method, Request, Response};
use crate::metrics::Metrics;
use crate::middleware::compression::CompressionMiddleware;
use crate::middleware::cors::CorsMiddleware;
use crate::middleware::csrf::CsrfMiddleware;
use crate::middleware::logging::LoggingMiddleware;
use crate::middleware::rate_limit::RateLimitMiddleware;
use crate::middleware::Middleware;
use crate::router::Router;
use crate::session::{InMemorySessionStore, SessionManager};

pub struct Server {
    config: Config,
    router: Arc<Router>,
    metrics: Arc<Metrics>,
    pub health: Arc<HealthState>,
    #[allow(dead_code)]
    session: Arc<SessionManager>,
    global_middleware: Vec<Box<dyn Middleware>>,
    shutdown: Arc<Notify>,
}

impl Server {
    pub fn new(config: Config, router: Router) -> Self {
        let metrics = Arc::new(Metrics::new());
        let health = Arc::new(HealthState::new());
        let store = Box::new(InMemorySessionStore::new());
        let session = Arc::new(SessionManager::new(&config.session_secret, store));

        // Build global middleware stack
        let mut global_middleware: Vec<Box<dyn Middleware>> = Vec::new();
        global_middleware.push(Box::new(LoggingMiddleware::new()));
        global_middleware.push(Box::new(CompressionMiddleware::new()));
        global_middleware.push(Box::new(RateLimitMiddleware::new(config.rate_limit)));

        if !config.allowed_origins.is_empty() {
            global_middleware.push(Box::new(CorsMiddleware::new(
                config.allowed_origins.clone(),
            )));
        }

        global_middleware.push(Box::new(CsrfMiddleware::new()));

        if let (Some(user), Some(pass)) = (&config.basic_auth_user, &config.basic_auth_pass) {
            global_middleware.push(Box::new(
                crate::middleware::basic_auth::BasicAuthMiddleware::new(user.clone(), pass.clone()),
            ));
        }

        Self {
            config,
            router: Arc::new(router),
            metrics,
            health,
            session,
            global_middleware,
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub fn with_middleware(
        config: Config,
        router: Router,
        middleware: Vec<Box<dyn Middleware>>,
    ) -> Self {
        let metrics = Arc::new(Metrics::new());
        let health = Arc::new(HealthState::new());
        let store = Box::new(InMemorySessionStore::new());
        let session = Arc::new(SessionManager::new(&config.session_secret, store));

        Self {
            config,
            router: Arc::new(router),
            metrics,
            health,
            session,
            global_middleware: middleware,
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub fn shutdown_signal(&self) -> Arc<Notify> {
        self.shutdown.clone()
    }

    pub async fn run(self: Arc<Self>) -> Result<(), crate::error::NanowebError> {
        let listener = TcpListener::bind(&self.config.listen).await?;
        tracing::info!("Listening on {}", self.config.listen);
        self.health.set_ready(true);

        // Spawn signal handler
        let shutdown = self.shutdown.clone();
        let health = self.health.clone();
        let timeout = self.config.graceful_timeout_s;
        tokio::spawn(async move {
            wait_for_signal().await;
            tracing::info!("Shutdown signal received, starting graceful shutdown");
            health.set_ready(false);
            shutdown.notify_waiters();
            tokio::time::sleep(std::time::Duration::from_secs(timeout)).await;
            tracing::info!("Graceful timeout reached, forcing shutdown");
            std::process::exit(0);
        });

        // Accept loop
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let server = self.clone();
                            let addr_str = addr.to_string();
                            tokio::spawn(async move {
                                if let Err(e) = server.handle_connection(stream, addr_str).await {
                                    tracing::debug!("Connection error: {e}");
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {e}");
                        }
                    }
                }
                _ = self.shutdown.notified() => {
                    tracing::info!("Stopping accept loop");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_connection(
        &self,
        mut stream: tokio::net::TcpStream,
        addr: String,
    ) -> Result<(), crate::error::NanowebError> {
        let mut buf = Vec::with_capacity(8192);
        let mut temp = [0u8; 4096];

        loop {
            let n = stream.read(&mut temp).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&temp[..n]);

            match crate::http::parse_request(
                &buf,
                uuid::Uuid::new_v4().to_string(),
                Some(addr.clone()),
            ) {
                Ok((mut req, consumed)) => {
                    buf.drain(..consumed);
                    let response = self.dispatch_request(&mut req).await;
                    let resp_bytes = response.to_bytes();
                    stream.write_all(&resp_bytes).await?;
                    let _ = stream.shutdown().await;
                    return Ok(());
                }
                Err(crate::error::NanowebError::Http(msg)) if msg == "Incomplete request" => {
                    // Need more data
                    if buf.len() > 16384 {
                        let resp = Response::text(400, "Bad Request");
                        stream.write_all(&resp.to_bytes()).await?;
                        let _ = stream.shutdown().await;
                        return Ok(());
                    }
                    continue;
                }
                Err(crate::error::NanowebError::Http(msg)) => {
                    tracing::warn!("Bad request from {addr}: {msg}");
                    let resp = Response::text(400, "Bad Request");
                    stream.write_all(&resp.to_bytes()).await?;
                    let _ = stream.shutdown().await;
                    return Ok(());
                }
                Err(e) => {
                    tracing::error!("Parse error: {e}");
                    let resp = Response::text(500, "Internal Server Error");
                    stream.write_all(&resp.to_bytes()).await?;
                    let _ = stream.shutdown().await;
                    return Ok(());
                }
            }
        }
    }

    async fn dispatch_request(&self, req: &mut Request) -> Response {
        let timer = Instant::now();

        // Built-in endpoints
        if req.path == "/healthz" {
            return self.health.healthz();
        }
        if req.path == "/readyz" {
            return self.health.readyz();
        }
        if req.path == self.config.metrics {
            let body = self.metrics.render();
            return Response::text(200, &body);
        }

        // OPTIONS auto-listing
        if req.method == Method::Options {
            let allowed = self.router.allowed_methods(&req.path);
            if !allowed.is_empty() {
                let methods_str: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
                let mut resp = Response::new(204);
                resp.set_header("allow", &methods_str.join(", "));
                return resp;
            }
        }

        // Route matching
        let route_result = self.router.route(req.method, &req.path);
        match route_result {
            Some((entry, params)) => {
                req.params = params;

                // Run middleware chain
                let mut all_middleware: Vec<&dyn Middleware> = Vec::new();
                for mw in &self.global_middleware {
                    all_middleware.push(mw.as_ref());
                }
                for mw in &entry.middleware {
                    all_middleware.push(mw.as_ref());
                }

                // Run before hooks
                for mw in &all_middleware {
                    if let Some(resp) = mw.before(req) {
                        self.record_metrics(req, &resp, &timer);
                        return resp;
                    }
                }

                // Run handler
                let handler = &entry.handler;
                let mut resp = handler(req.clone()).await;

                // HEAD: suppress body
                if req.method == Method::Head {
                    resp.body.clear();
                }

                // Run after hooks in reverse
                for mw in all_middleware.iter().rev() {
                    mw.after(req, &mut resp);
                }

                self.record_metrics(req, &resp, &timer);
                resp
            }
            None => {
                let resp = Response::text(404, "Not Found");
                self.record_metrics(req, &resp, &timer);
                resp
            }
        }
    }

    fn record_metrics(&self, req: &Request, resp: &Response, timer: &Instant) {
        let duration = timer.elapsed().as_secs_f64();
        self.metrics
            .record_request(req.method.as_str(), resp.status_class(), &req.path);
        self.metrics.record_duration(duration);
    }
}

async fn wait_for_signal() {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).unwrap();
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
    use crate::router::Router;

    #[tokio::test]
    async fn test_server_dispatch_healthz() {
        let config = Config::default();
        let router = Router::new();
        let server = Arc::new(Server::new(config, router));

        let mut req = Request {
            method: Method::Get,
            target: "/healthz".to_string(),
            path: "/healthz".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };

        let resp = server.dispatch_request(&mut req).await;
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_server_dispatch_readyz() {
        let config = Config::default();
        let router = Router::new();
        let server = Arc::new(Server::new(config, router));

        server.health.set_ready(true);

        let mut req = Request {
            method: Method::Get,
            target: "/readyz".to_string(),
            path: "/readyz".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };

        let resp = server.dispatch_request(&mut req).await;
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_server_dispatch_metrics() {
        let config = Config::default();
        let router = Router::new();
        let server = Arc::new(Server::new(config, router));

        let mut req = Request {
            method: Method::Get,
            target: "/metrics".to_string(),
            path: "/metrics".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };

        let resp = server.dispatch_request(&mut req).await;
        assert_eq!(resp.status, 200);
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("nanoweb_http_requests_total"));
    }

    #[tokio::test]
    async fn test_server_dispatch_404() {
        let config = Config::default();
        let router = Router::new();
        let server = Arc::new(Server::new(config, router));

        let mut req = Request {
            method: Method::Get,
            target: "/nonexistent".to_string(),
            path: "/nonexistent".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers: Default::default(),
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };

        let resp = server.dispatch_request(&mut req).await;
        assert_eq!(resp.status, 404);
    }
}
