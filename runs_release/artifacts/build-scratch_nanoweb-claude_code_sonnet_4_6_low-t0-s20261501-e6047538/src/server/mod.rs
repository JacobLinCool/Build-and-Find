use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::error::NanowebError;
use crate::http::parser::Parser as HttpParser;
use crate::http::types::{Method, Response, Status};
use crate::metrics::MetricsRegistry;
use crate::middleware::MiddlewareStack;
use crate::router::{Handler, Router};

pub struct Server {
    config: Config,
    router: Arc<Router>,
    middleware: Arc<MiddlewareStack>,
    metrics: Option<Arc<MetricsRegistry>>,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Server {
            config,
            router: Arc::new(Router::new()),
            middleware: Arc::new(MiddlewareStack::new()),
            metrics: None,
        }
    }

    pub fn with_router(mut self, router: Router) -> Self {
        self.router = Arc::new(router);
        self
    }

    pub fn with_middleware(mut self, middleware: MiddlewareStack) -> Self {
        self.middleware = Arc::new(middleware);
        self
    }

    pub fn with_metrics(mut self, metrics: MetricsRegistry) -> Self {
        self.metrics = Some(Arc::new(metrics));
        self
    }

    pub async fn run(self) -> Result<(), NanowebError> {
        let addr: SocketAddr = self
            .config
            .listen
            .parse()
            .map_err(|e| NanowebError::Config(format!("invalid listen address: {}", e)))?;

        let listener = TcpListener::bind(&addr).await?;
        info!("Server listening on {}", addr);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Setup signal handlers
        let shutdown_tx_clone = shutdown_tx.clone();
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    info!("Received SIGINT, shutting down...");
                    let _ = shutdown_tx_clone.send(true);
                }
                Err(e) => error!("Failed to listen for signal: {}", e),
            }
        });

        let router = self.router.clone();
        let middleware = self.middleware.clone();
        let metrics = self.metrics.clone();
        let graceful_timeout = Duration::from_secs(self.config.graceful_timeout_s);
        let metrics_path = self.config.metrics_path.clone();

        let active_connections = Arc::new(tokio::sync::Semaphore::new(10000));

        loop {
            let mut shutdown_rx_clone = shutdown_rx.clone();

            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            let router = router.clone();
                            let middleware = middleware.clone();
                            let metrics = metrics.clone();
                            let metrics_path = metrics_path.clone();
                            let permit = active_connections.clone().acquire_owned().await;

                            tokio::spawn(async move {
                                let _permit = permit;
                                handle_connection(stream, peer_addr, router, middleware, metrics, &metrics_path).await;
                            });
                        }
                        Err(e) => {
                            warn!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx_clone.changed() => {
                    if *shutdown_rx_clone.borrow() {
                        info!("Stopping accept loop...");
                        break;
                    }
                }
            }
        }

        // Wait for in-flight requests
        info!(
            "Waiting for in-flight requests (timeout: {}s)...",
            graceful_timeout.as_secs()
        );
        let _ = tokio::time::timeout(graceful_timeout, async {
            // Wait until all permits are returned
            let _ = active_connections.acquire_many(10000).await;
        })
        .await;

        info!("Shutdown complete");
        Ok(())
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    router: Arc<Router>,
    middleware: Arc<MiddlewareStack>,
    metrics: Option<Arc<MetricsRegistry>>,
    metrics_path: &str,
) {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];

    loop {
        // Read data
        match stream.read(&mut tmp).await {
            Ok(0) => break, // Connection closed
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(e) => {
                warn!("Read error from {}: {}", peer_addr, e);
                break;
            }
        }

        // Try to parse a request
        let parse_result = HttpParser::parse(&buf);

        match parse_result {
            Ok((mut req, consumed)) => {
                buf.drain(..consumed);

                // Check if this is the metrics path
                if let Some(ref metrics_reg) = metrics {
                    if req.path == metrics_path {
                        let metrics_reg = metrics_reg.clone();
                        let resp = metrics_reg.handle_metrics(req).await.unwrap_or_else(|_| {
                            Response::new(Status::InternalServerError).with_text("metrics error")
                        });
                        let resp_bytes = resp.to_bytes(true);
                        let _ = stream.write_all(&resp_bytes).await;
                        continue;
                    }
                }

                let start = std::time::Instant::now();
                let method_str = req.method.to_string();
                let path = req.path.clone();

                let route_match = router.lookup(&req.method, &req.path);

                let (resp, route_label) = if let Some(rm) = route_match {
                    req.params = rm.params;
                    let handler = rm.handler;
                    let route = path.clone();

                    let resp = middleware
                        .run(req, move |r| {
                            let h = handler.clone();
                            async move { h.call(r).await }
                        })
                        .await
                        .unwrap_or_else(|e| error_response(e));

                    (resp, route)
                } else {
                    // Check if method not allowed
                    let allowed = router.allowed_methods(&path);
                    let resp = if allowed.is_empty() {
                        Response::new(Status::NotFound).with_text("Not Found")
                    } else if req.method == Method::OPTIONS {
                        Response::new(Status::NoContent).set_header("allow", allowed.join(", "))
                    } else {
                        Response::new(Status::MethodNotAllowed)
                            .with_text("Method Not Allowed")
                            .set_header("allow", allowed.join(", "))
                    };
                    (resp, path.clone())
                };

                // Record metrics
                if let Some(ref metrics_reg) = metrics {
                    let duration = start.elapsed().as_secs_f64();
                    let status_class = resp.status.class().to_string();
                    metrics_reg.record_request(&method_str, &status_class, &route_label);
                    metrics_reg.record_duration(&method_str, &route_label, duration);
                }

                let include_body = method_str != "HEAD";
                let resp_bytes = resp.to_bytes(include_body);

                if let Err(e) = stream.write_all(&resp_bytes).await {
                    warn!("Write error to {}: {}", peer_addr, e);
                    break;
                }

                // Check for Connection: close
                // For simplicity, keep-alive is supported via loop
                // Break if connection: close
                // (In HTTP/1.1, persistent connections are the default)
                if buf.is_empty() {
                    // Wait for more data or connection close
                    // Continue the loop to read next request
                }
            }
            Err(crate::http::parser::ParseError::Incomplete) => {
                // Need more data, keep reading
                continue;
            }
            Err(e) => {
                warn!("Parse error from {}: {}", peer_addr, e);
                let resp =
                    Response::new(Status::BadRequest).with_text(format!("Bad Request: {}", e));
                let _ = stream.write_all(&resp.to_bytes(true)).await;
                break;
            }
        }
    }
}

fn error_response(e: NanowebError) -> Response {
    match e {
        NanowebError::NotFound => Response::new(Status::NotFound).with_text("Not Found"),
        NanowebError::MethodNotAllowed => {
            Response::new(Status::MethodNotAllowed).with_text("Method Not Allowed")
        }
        NanowebError::BadRequest(msg) => Response::new(Status::BadRequest).with_text(msg),
        NanowebError::Unauthorized => Response::new(Status::Unauthorized).with_text("Unauthorized"),
        NanowebError::Forbidden => Response::new(Status::Forbidden).with_text("Forbidden"),
        NanowebError::TooManyRequests => {
            Response::new(Status::TooManyRequests).with_text("Too Many Requests")
        }
        _ => {
            error!("Internal error: {}", e);
            Response::new(Status::InternalServerError).with_text("Internal Server Error")
        }
    }
}

/// A builder for creating a nanoweb application
pub struct App {
    router: Router,
    middleware: MiddlewareStack,
    metrics: Option<MetricsRegistry>,
}

impl App {
    pub fn new() -> Self {
        App {
            router: Router::new(),
            middleware: MiddlewareStack::new(),
            metrics: None,
        }
    }

    pub fn get(mut self, path: &str, handler: impl Handler + 'static) -> Self {
        self.router.add_route(Method::GET, path, handler);
        self
    }

    pub fn post(mut self, path: &str, handler: impl Handler + 'static) -> Self {
        self.router.add_route(Method::POST, path, handler);
        self
    }

    pub fn put(mut self, path: &str, handler: impl Handler + 'static) -> Self {
        self.router.add_route(Method::PUT, path, handler);
        self
    }

    pub fn delete(mut self, path: &str, handler: impl Handler + 'static) -> Self {
        self.router.add_route(Method::DELETE, path, handler);
        self
    }

    pub fn patch(mut self, path: &str, handler: impl Handler + 'static) -> Self {
        self.router.add_route(Method::PATCH, path, handler);
        self
    }

    pub fn route(mut self, method: Method, path: &str, handler: impl Handler + 'static) -> Self {
        self.router.add_route(method, path, handler);
        self
    }

    pub fn middleware(mut self, m: impl crate::middleware::Middleware + 'static) -> Self {
        self.middleware.add(m);
        self
    }

    pub fn with_metrics(mut self) -> Self {
        self.metrics = Some(MetricsRegistry::new());
        self
    }

    pub fn into_server(self, config: Config) -> Server {
        let mut server = Server::new(config)
            .with_router(self.router)
            .with_middleware(self.middleware);

        if let Some(m) = self.metrics {
            server = server.with_metrics(m);
        }

        server
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}
