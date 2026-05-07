use crate::config::Config;
use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::parser::{into_request, read_request};
use crate::http::types::{Method, Response};
use crate::metrics::Metrics;
use crate::router::Router;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

pub struct Server {
    pub config: Arc<Config>,
    pub router: Arc<Router>,
    pub metrics: Arc<Metrics>,
    pub health: Arc<HealthState>,
}

impl Server {
    pub fn new(config: Config) -> Result<Self, NanowebError> {
        Ok(Server {
            config: Arc::new(config),
            router: Arc::new(Router::new()),
            metrics: Arc::new(Metrics::new()?),
            health: Arc::new(HealthState::new()),
        })
    }

    pub fn router_mut(&mut self) -> &mut Router {
        Arc::get_mut(&mut self.router).expect("Router has no other owners")
    }

    pub async fn run(self) -> Result<(), NanowebError> {
        let listener = TcpListener::bind(&self.config.listen).await.map_err(|e| {
            NanowebError::Config(format!("Cannot bind {}: {}", self.config.listen, e))
        })?;
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        self.run_on_listener(listener, shutdown_tx).await
    }

    pub async fn run_on_listener(
        self,
        listener: TcpListener,
        shutdown_signal: broadcast::Sender<()>,
    ) -> Result<(), NanowebError> {
        let local_addr = listener.local_addr().map_err(NanowebError::Io)?;
        info!("nanoweb listening on {}", local_addr);

        let config = self.config.clone();
        let router = self.router.clone();
        let metrics = self.metrics.clone();
        let health = self.health.clone();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let graceful_timeout = Duration::from_secs(config.graceful_timeout_s);

        health.set_ready(true);

        let shutdown_tx_for_signal = shutdown_signal.clone();
        tokio::spawn(async move {
            wait_for_signal().await;
            info!("Shutdown signal received");
            let _ = shutdown_tx_for_signal.send(());
        });

        let mut shutdown_rx = shutdown_signal.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let router = router.clone();
                            let metrics = metrics.clone();
                            let in_flight = in_flight.clone();
                            in_flight.fetch_add(1, Ordering::SeqCst);
                            tokio::spawn(async move {
                                handle_connection(stream, addr.to_string(), router, metrics).await;
                                in_flight.fetch_sub(1, Ordering::SeqCst);
                            });
                        }
                        Err(e) => warn!("Accept error: {}", e),
                    }
                }
                _ = shutdown_rx.recv() => break,
            }
        }

        health.set_ready(false);
        info!(
            "Draining in-flight requests (timeout: {}s)",
            config.graceful_timeout_s
        );
        let start = Instant::now();
        loop {
            if in_flight.load(Ordering::SeqCst) == 0 {
                break;
            }
            if start.elapsed() > graceful_timeout {
                warn!("Graceful shutdown timeout exceeded");
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        info!("Server shut down cleanly");
        Ok(())
    }
}

async fn handle_connection(
    stream: TcpStream,
    remote_addr: String,
    router: Arc<Router>,
    metrics: Arc<Metrics>,
) {
    let mut reader = BufReader::new(stream);

    loop {
        let request_id = Uuid::new_v4().to_string();
        let start = Instant::now();

        let raw = match read_request(&mut reader).await {
            Ok(r) => r,
            Err(NanowebError::ConnectionClosed) => break,
            Err(e) => {
                let resp = error_to_response(&e);
                let _ = reader.get_mut().write_all(&resp.serialize()).await;
                break;
            }
        };

        let is_head = raw.method == Method::Head;
        let keep_alive = check_keep_alive(&raw.headers, &raw.version);
        let method_str = raw.method.to_string();
        let path = raw.path.clone();

        let request = into_request(raw, remote_addr.clone(), request_id.clone());

        let mut response = match router.dispatch(request) {
            Ok((next, req)) => match next.run(req).await {
                Ok(r) => r,
                Err(e) => error_to_response(&e),
            },
            Err(e) => error_to_response(&e),
        };

        metrics.record_request(&method_str, response.status, &path, start.elapsed());

        if is_head {
            response.body.clear();
            // Keep Content-Length from original response
        }

        response.set_header("X-Request-Id", &request_id);
        if !keep_alive {
            response.set_header("Connection", "close");
        }

        if let Err(_) = reader.get_mut().write_all(&response.serialize()).await {
            break;
        }

        if !keep_alive {
            break;
        }
    }
}

fn check_keep_alive(
    headers: &[(String, String)],
    version: &crate::http::types::HttpVersion,
) -> bool {
    let conn = headers
        .iter()
        .find(|(k, _)| k == "connection")
        .map(|(_, v)| v.to_lowercase());

    match version {
        crate::http::types::HttpVersion::Http11 => conn.as_deref() != Some("close"),
        crate::http::types::HttpVersion::Http10 => conn.as_deref() == Some("keep-alive"),
    }
}

fn error_to_response(e: &NanowebError) -> Response {
    let status = e.status_code();
    let mut resp = Response::with_body(status, format!("{}", e).into_bytes());
    resp.set_header("Content-Type", "text/plain; charset=utf-8");
    if let NanowebError::MethodNotAllowed(methods) = e {
        resp.set_header("Allow", &methods.join(", "));
    }
    resp
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sigint.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.expect("ctrl-c handler");
    }
}

pub struct ServerBuilder {
    config: Config,
}

impl ServerBuilder {
    pub fn new() -> Self {
        ServerBuilder {
            config: Config::default(),
        }
    }

    pub fn with_config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn listen(mut self, addr: &str) -> Self {
        self.config.listen = addr.to_string();
        self
    }

    pub fn build(self) -> Result<Server, NanowebError> {
        Server::new(self.config)
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_to_response_not_found() {
        let resp = error_to_response(&NanowebError::NotFound);
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn test_error_to_response_method_not_allowed() {
        let resp = error_to_response(&NanowebError::MethodNotAllowed(vec!["GET".into()]));
        assert_eq!(resp.status, 405);
        assert!(resp.get_header("allow").is_some());
    }
}
