use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::error::NanowebError;
use crate::health::HealthState;
use crate::http::parser::parse_request;
use crate::http::{Request, Response};
use crate::router::Router;

const READ_TIMEOUT_SECS: u64 = 30;
const MAX_CONCURRENT: usize = 1024;
const READ_BUF_SIZE: usize = 8192;

pub struct Server {
    router: Arc<Router>,
    health: Arc<HealthState>,
    graceful_timeout: Duration,
}

impl Server {
    pub fn new(router: Router, health: Arc<HealthState>, graceful_timeout_s: u64) -> Self {
        Self {
            router: Arc::new(router),
            health,
            graceful_timeout: Duration::from_secs(graceful_timeout_s),
        }
    }

    /// Run the accept loop. `shutdown` is a future that resolves when the server should stop.
    pub async fn run(
        self: Arc<Self>,
        listener: TcpListener,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), NanowebError> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
        let mut active = tokio::task::JoinSet::new();

        self.health.set_ready(true);
        info!("Server ready, accepting connections");

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            crate::metrics::record_connection();
                            let permit = semaphore.clone().acquire_owned().await
                                .map_err(|e| NanowebError::Internal(e.to_string()))?;

                            let server = self.clone();
                            active.spawn(async move {
                                let _permit = permit; // Released when dropped
                                if let Err(e) = server.handle_connection(stream, peer_addr).await {
                                    debug!("Connection error from {}: {}", peer_addr, e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Shutdown signal received, draining connections...");
                        break;
                    }
                }
            }

            // Clean up finished tasks
            while active.try_join_next().is_some() {}
        }

        // Graceful shutdown: wait for in-flight requests
        self.health.set_ready(false);
        let grace = self.graceful_timeout;

        match timeout(grace, async { while active.join_next().await.is_some() {} }).await {
            Ok(_) => info!("All connections drained"),
            Err(_) => warn!("Graceful timeout expired, forcing shutdown"),
        }

        Ok(())
    }

    async fn handle_connection(
        &self,
        mut stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> Result<(), NanowebError> {
        let mut buf = Vec::with_capacity(READ_BUF_SIZE);
        let mut tmp = [0u8; READ_BUF_SIZE];

        loop {
            // Read until we have a complete request
            let parsed = loop {
                match parse_request(&buf) {
                    Ok(Some(result)) => break Some(result),
                    Ok(None) => {
                        // Need more data
                        let n = match timeout(
                            Duration::from_secs(READ_TIMEOUT_SECS),
                            stream.read(&mut tmp),
                        )
                        .await
                        {
                            Ok(Ok(0)) => return Ok(()), // Connection closed
                            Ok(Ok(n)) => n,
                            Ok(Err(e)) => return Err(e.into()),
                            Err(_) => {
                                // Read timeout
                                let resp = Response::new(408).with_text("Request Timeout");
                                let _ = stream.write_all(&resp.into_bytes(false)).await;
                                return Ok(());
                            }
                        };
                        buf.extend_from_slice(&tmp[..n]);

                        if buf.len() > 16 * 1024 * 1024 {
                            let resp = Response::bad_request("Request too large");
                            let _ = stream.write_all(&resp.into_bytes(false)).await;
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        let resp = Response::bad_request(&e.to_string());
                        let _ = stream.write_all(&resp.into_bytes(false)).await;
                        return Ok(());
                    }
                }
            };

            let (parsed, consumed) = match parsed {
                Some(r) => r,
                None => return Ok(()),
            };

            // Remove consumed bytes
            buf.drain(..consumed);

            let request_id = Uuid::new_v4().to_string();
            let request = Request::new(
                parsed.method,
                parsed.target,
                parsed.version,
                parsed.headers,
                parsed.body,
                Some(peer_addr),
                request_id.clone(),
            );

            let method_str = request.method.as_str().to_string();
            let path = request.path.clone();
            let start = std::time::Instant::now();

            let (response, keep_alive) = self.dispatch(request).await;

            let elapsed = start.elapsed().as_secs_f64();
            crate::metrics::record_request(&method_str, &path, response.status, elapsed);

            let suppress_body = method_str == "HEAD";
            let bytes = response.into_bytes(suppress_body);

            // Ensure Connection header
            if !keep_alive {
                // Already handled in dispatch
            }

            if let Err(e) = stream.write_all(&bytes).await {
                debug!("Write error: {}", e);
                return Ok(());
            }

            if !keep_alive {
                return Ok(());
            }

            // Keep-alive: continue loop to handle next request on same connection
        }
    }

    async fn dispatch(&self, req: Request) -> (Response, bool) {
        let connection = req
            .headers
            .get("connection")
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        let keep_alive = req.version == crate::http::Version::Http11 && connection != "close";

        // Inject request ID into response
        let request_id = req.request_id.clone();

        let result = self.router.handle(req).await;

        let mut response = match result {
            Ok(resp) => resp,
            Err(e) => error_to_response(&e),
        };

        // Add standard headers
        response.headers.insert("X-Request-Id", &request_id);
        if !keep_alive {
            response.headers.insert("Connection", "close");
        }

        (response, keep_alive)
    }
}

fn error_to_response(e: &NanowebError) -> Response {
    let status = e.status_code();
    let mut resp = Response::new(status);
    resp.headers.insert("Content-Type", "application/json");
    resp.body = e.to_response_body().into_bytes();
    resp.headers
        .insert("Content-Length", resp.body.len().to_string());
    resp
}

/// Start a plain HTTP server and return the join handle.
pub async fn start_http(
    addr: &str,
    server: Arc<Server>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<tokio::task::JoinHandle<()>, NanowebError> {
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    info!("HTTP server listening on {}", local_addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = server.run(listener, shutdown).await {
            error!("Server error: {}", e);
        }
    });

    Ok(handle)
}

/// Start a TLS server (when feature enabled).
#[cfg(feature = "tls")]
pub async fn start_https(
    addr: &str,
    server: Arc<Server>,
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<tokio::task::JoinHandle<()>, NanowebError> {
    use crate::tls::make_acceptor;

    let acceptor = make_acceptor(cert_path, key_path)?;
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    info!("HTTPS server listening on {}", local_addr);

    let handle = tokio::spawn(async move {
        if let Err(e) = run_tls_server(server, listener, acceptor, shutdown).await {
            error!("TLS server error: {}", e);
        }
    });

    Ok(handle)
}

#[cfg(feature = "tls")]
async fn run_tls_server(
    server: Arc<Server>,
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), NanowebError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let mut active = tokio::task::JoinSet::new();

    server.health.set_ready(true);

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, peer_addr)) => {
                        let permit = semaphore.clone().acquire_owned().await
                            .map_err(|e| NanowebError::Internal(e.to_string()))?;
                        let server = server.clone();
                        let acceptor = acceptor.clone();

                        active.spawn(async move {
                            let _permit = permit;
                            match acceptor.accept(stream).await {
                                Ok(tls_stream) => {
                                    if let Err(e) = handle_tls_connection(server, peer_addr, tls_stream).await {
                                        debug!("TLS connection error from {}: {}", peer_addr, e);
                                    }
                                }
                                Err(e) => {
                                    debug!("TLS handshake failed from {}: {}", peer_addr, e);
                                }
                            }
                        });
                    }
                    Err(e) => error!("TLS accept error: {}", e),
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }

    let grace = server.graceful_timeout;
    let _ = timeout(grace, async { while active.join_next().await.is_some() {} }).await;

    Ok(())
}

#[cfg(feature = "tls")]
async fn handle_tls_connection(
    server: Arc<Server>,
    peer_addr: SocketAddr,
    mut stream: tokio_rustls::server::TlsStream<TcpStream>,
) -> Result<(), NanowebError> {
    let mut buf = Vec::with_capacity(READ_BUF_SIZE);
    let mut tmp = [0u8; READ_BUF_SIZE];

    loop {
        let parsed = loop {
            match parse_request(&buf) {
                Ok(Some(result)) => break Some(result),
                Ok(None) => {
                    let n = match timeout(
                        Duration::from_secs(READ_TIMEOUT_SECS),
                        stream.read(&mut tmp),
                    )
                    .await
                    {
                        Ok(Ok(0)) => return Ok(()),
                        Ok(Ok(n)) => n,
                        Ok(Err(e)) => return Err(e.into()),
                        Err(_) => {
                            let resp = Response::new(408).with_text("Request Timeout");
                            let _ = stream.write_all(&resp.into_bytes(false)).await;
                            return Ok(());
                        }
                    };
                    buf.extend_from_slice(&tmp[..n]);
                }
                Err(e) => {
                    let resp = Response::bad_request(&e.to_string());
                    let _ = stream.write_all(&resp.into_bytes(false)).await;
                    return Ok(());
                }
            }
        };

        let (parsed, consumed) = match parsed {
            Some(r) => r,
            None => return Ok(()),
        };

        buf.drain(..consumed);

        let request_id = Uuid::new_v4().to_string();
        let request = Request::new(
            parsed.method,
            parsed.target,
            parsed.version,
            parsed.headers,
            parsed.body,
            Some(peer_addr),
            request_id,
        );

        let method_str = request.method.as_str().to_string();
        let path = request.path.clone();
        let start = std::time::Instant::now();

        let (response, keep_alive) = server.dispatch(request).await;

        let elapsed = start.elapsed().as_secs_f64();
        crate::metrics::record_request(&method_str, &path, response.status, elapsed);

        let suppress_body = method_str == "HEAD";
        if let Err(e) = stream.write_all(&response.into_bytes(suppress_body)).await {
            debug!("TLS write error: {}", e);
            return Ok(());
        }

        if !keep_alive {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthState;
    use crate::router::Router;

    fn make_server() -> Arc<Server> {
        let router = Router::new();
        let health = Arc::new(HealthState::new());
        Arc::new(Server::new(router, health, 5))
    }

    #[tokio::test]
    async fn test_error_to_response_not_found() {
        let e = NanowebError::NotFound;
        let resp = error_to_response(&e);
        assert_eq!(resp.status, 404);
    }

    #[tokio::test]
    async fn test_error_to_response_bad_request() {
        let e = NanowebError::BadRequest("test".into());
        let resp = error_to_response(&e);
        assert_eq!(resp.status, 400);
    }
}
