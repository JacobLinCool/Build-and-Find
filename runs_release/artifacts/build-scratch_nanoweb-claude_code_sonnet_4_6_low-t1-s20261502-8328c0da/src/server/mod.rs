use crate::config::Config;
use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::parser::Parser as HttpParser;
use crate::http::types::{Headers, Method, Request, Response, StatusCode};
use crate::metrics::Metrics;
use crate::middleware::MiddlewareStack;
use crate::router::{RouteMatch, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, RwLock};
use tracing::{error, info};
use uuid::Uuid;

pub struct ServerState {
    pub router: Arc<RwLock<Router>>,
    pub middleware: Arc<MiddlewareStack>,
    pub config: Config,
    pub metrics: Arc<Metrics>,
    pub health: HealthState,
}

pub struct Server {
    state: Arc<ServerState>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl Server {
    pub fn new(config: Config) -> Result<Self, NanowebError> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let metrics = Metrics::new()?;

        Ok(Server {
            state: Arc::new(ServerState {
                router: Arc::new(RwLock::new(Router::new())),
                middleware: Arc::new(MiddlewareStack::new()),
                config,
                metrics,
                health: HealthState::new(),
            }),
            shutdown_tx,
            shutdown_rx,
        })
    }

    pub fn with_state(
        config: Config,
        router: Router,
        middleware: MiddlewareStack,
    ) -> Result<Self, NanowebError> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let metrics = Metrics::new()?;

        Ok(Server {
            state: Arc::new(ServerState {
                router: Arc::new(RwLock::new(router)),
                middleware: Arc::new(middleware),
                config,
                metrics,
                health: HealthState::new(),
            }),
            shutdown_tx,
            shutdown_rx,
        })
    }

    pub fn state(&self) -> Arc<ServerState> {
        self.state.clone()
    }

    pub async fn run(&self) -> Result<(), NanowebError> {
        let addr: SocketAddr = self
            .state
            .config
            .listen
            .parse()
            .map_err(|e| NanowebError::Config(format!("Invalid listen address: {}", e)))?;

        let listener = TcpListener::bind(addr).await?;
        info!("Listening on {}", addr);

        self.state.health.set_ready(true);

        self.accept_loop(listener).await
    }

    pub async fn run_on_listener(&self, listener: TcpListener) -> Result<(), NanowebError> {
        self.state.health.set_ready(true);
        self.accept_loop(listener).await
    }

    async fn accept_loop(&self, listener: TcpListener) -> Result<(), NanowebError> {
        let mut shutdown_rx = self.shutdown_rx.clone();
        let graceful_timeout = std::time::Duration::from_secs(self.state.config.graceful_timeout_s);
        let in_flight = Arc::new(tokio::sync::Semaphore::new(10000));

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, remote_addr)) => {
                            let state = self.state.clone();
                            let permit = in_flight.clone().acquire_owned().await.unwrap();
                            tokio::spawn(async move {
                                handle_connection(stream, remote_addr, state).await;
                                drop(permit);
                            });
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Shutdown signal received, stopping accept loop");
                        break;
                    }
                }
            }
        }

        // Wait for in-flight requests or timeout
        info!("Waiting for in-flight requests to complete...");
        let wait_start = Instant::now();
        loop {
            let available = in_flight.available_permits();
            if available == 10000 {
                break;
            }
            if wait_start.elapsed() >= graceful_timeout {
                info!("Graceful timeout reached, forcing shutdown");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        info!("Server shutdown complete");
        Ok(())
    }

    pub fn shutdown(&self) {
        self.state.health.set_shutting_down(true);
        let _ = self.shutdown_tx.send(true);
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    state: Arc<ServerState>,
) {
    let mut buf = vec![0u8; 8192];
    let mut read_buf = Vec::new();

    loop {
        // Read data
        let n = match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                error!("Read error: {}", e);
                break;
            }
        };

        read_buf.extend_from_slice(&buf[..n]);

        // Try to parse headers
        match HttpParser::parse_headers(&read_buf) {
            Ok(Some((mut parsed, body_start))) => {
                let remaining_body = read_buf[body_start..].to_vec();
                read_buf.clear();

                // Read body
                if let Err(e) = HttpParser::read_body(&mut stream, &mut parsed, &remaining_body).await {
                    let resp = Response::new(StatusCode::BAD_REQUEST)
                        .with_body(e.to_string().into_bytes());
                    let _ = stream.write_all(&resp.into_bytes()).await;
                    break;
                }

                // Build request
                let method = Method::from_str(&parsed.method);
                let mut headers = Headers::new();
                for (k, v) in parsed.headers {
                    headers.set(k, v);
                }

                let request_id = Uuid::new_v4().to_string();
                let mut req = Request::new(
                    method,
                    parsed.path,
                    parsed.query,
                    headers,
                    parsed.body,
                    remote_addr,
                );
                req.request_id = request_id.clone();

                // Process request
                let start = Instant::now();
                let resp = dispatch_request(req, &state).await;
                let duration = start.elapsed().as_secs_f64();

                let mut resp = match resp {
                    Ok(r) => r,
                    Err(e) => error_response(e),
                };

                // Add X-Request-Id header
                resp.headers.set("x-request-id", &request_id);

                // Record metrics
                let route = "unknown"; // simplified
                state
                    .metrics
                    .record_request("unknown", resp.status.0, route, duration);

                // Check keep-alive
                let keep_alive = resp.headers.get("connection")
                    .map(|v| v.to_lowercase() != "close")
                    .unwrap_or(false);

                // Write response
                if let Err(e) = stream.write_all(&resp.into_bytes()).await {
                    error!("Write error: {}", e);
                    break;
                }

                if !keep_alive {
                    break;
                }
            }
            Ok(None) => {
                // Need more data
                if read_buf.len() > 16 * 1024 {
                    let resp = Response::new(StatusCode::BAD_REQUEST)
                        .with_body(b"Request headers too large".to_vec());
                    let _ = stream.write_all(&resp.into_bytes()).await;
                    break;
                }
            }
            Err(e) => {
                let resp = Response::new(StatusCode::BAD_REQUEST)
                    .with_body(e.to_string().into_bytes());
                let _ = stream.write_all(&resp.into_bytes()).await;
                break;
            }
        }
    }
}

async fn dispatch_request(
    mut req: Request,
    state: &Arc<ServerState>,
) -> Result<Response, NanowebError> {
    let path = req.path.clone();
    let method = req.method.clone();

    // Handle built-in routes first
    if path == "/healthz" {
        return Ok(state.health.healthz_response());
    }
    if path == "/readyz" {
        return Ok(state.health.readyz_response());
    }
    if path == state.config.metrics {
        let body = state.metrics.render()?;
        return Ok(Response::text(body).with_header("content-type", "text/plain; version=0.0.4"));
    }

    // Route lookup
    let router = state.router.read().await;
    let route_match = router.lookup(&path, &method);

    match route_match {
        RouteMatch::Found { handler, params } => {
            req.params = params;

            // Handle HEAD: run GET handler but suppress body
            let is_head = method == Method::Head;
            let actual_req = if is_head {
                let mut r = req.clone();
                r.method = Method::Get;
                r
            } else {
                req
            };

            let handler_arc = Arc::clone(&handler);
            let middleware = state.middleware.clone();

            let resp = middleware
                .run(
                    actual_req,
                    Arc::new(move |req| handler_arc(req)),
                )
                .await?;

            if is_head {
                let mut head_resp = resp;
                head_resp.body = Vec::new();
                Ok(head_resp)
            } else {
                Ok(resp)
            }
        }
        RouteMatch::MethodNotAllowed(allowed) => {
            // For OPTIONS: auto-list allowed methods
            if method == Method::Options {
                let allow = allowed.join(", ");
                return Ok(Response::new(StatusCode::OK)
                    .with_header("allow", allow)
                    .with_body(b"".to_vec()));
            }

            let allow = allowed.join(", ");
            Ok(Response::new(StatusCode::METHOD_NOT_ALLOWED)
                .with_header("allow", allow)
                .with_body(b"Method Not Allowed".to_vec()))
        }
        RouteMatch::NotFound => {
            // Try static file serving
            if let Some(static_dir) = &state.config.static_dir {
                let file_server = crate::static_files::StaticFileServer::new(static_dir);
                match file_server.serve(&req, &path).await {
                    Ok(resp) => return Ok(resp),
                    Err(NanowebError::NotFound(_)) | Err(NanowebError::Forbidden(_)) => {}
                    Err(e) => return Err(e),
                }
            }

            Ok(Response::new(StatusCode::NOT_FOUND)
                .with_body(b"Not Found".to_vec()))
        }
    }
}

fn error_response(err: NanowebError) -> Response {
    match err {
        NanowebError::NotFound(msg) => Response::new(StatusCode::NOT_FOUND)
            .with_body(msg.into_bytes()),
        NanowebError::MethodNotAllowed(methods) => Response::new(StatusCode::METHOD_NOT_ALLOWED)
            .with_header("allow", methods.join(", "))
            .with_body(b"Method Not Allowed".to_vec()),
        NanowebError::Unauthorized(msg) => Response::new(StatusCode::UNAUTHORIZED)
            .with_body(msg.into_bytes()),
        NanowebError::Forbidden(msg) => Response::new(StatusCode::FORBIDDEN)
            .with_body(msg.into_bytes()),
        NanowebError::BadRequest(msg) => Response::new(StatusCode::BAD_REQUEST)
            .with_body(msg.into_bytes()),
        _ => Response::new(StatusCode::INTERNAL_SERVER_ERROR)
            .with_body(err.to_string().into_bytes()),
    }
}

/// Setup graceful shutdown on SIGINT/SIGTERM
pub async fn setup_shutdown_signal(server: Arc<Server>) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigint = signal(SignalKind::interrupt()).expect("Failed to setup SIGINT handler");
            let mut sigterm = signal(SignalKind::terminate()).expect("Failed to setup SIGTERM handler");

            tokio::select! {
                _ = sigint.recv() => {
                    info!("Received SIGINT");
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM");
                }
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.expect("Failed to setup ctrl-c handler");
            info!("Received ctrl-c");
        }

        server.shutdown();
    });
}
