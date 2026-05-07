use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tokio::io::{AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

use crate::config::{Config, MiddlewareConfig, RouteHandlerConfig};
use crate::error::Result;
use crate::health::HealthState;
use crate::http::{Method, Request, Response, StatusCode};
use crate::metrics;
use crate::middleware;
use crate::middleware::basic_auth::BasicAuth;
use crate::middleware::cors::{Cors, CorsConfig};
use crate::middleware::csrf::Csrf;
use crate::middleware::gzip::GzipCompression;
use crate::middleware::logging::RequestLogging;
use crate::middleware::rate_limit::{RateLimit, RateLimitConfig};
use crate::router::{Handler, RouteMatch, Router, allowed_methods};
use crate::session::{MemorySessionStore, Session, SessionStore};
use crate::static_files::StaticFiles;
use crate::template::Renderer;

#[derive(Clone)]
struct App {
    router: Arc<Router>,
    middlewares: Arc<Vec<Arc<dyn middleware::Middleware>>>,
    health: HealthState,
    session_store: Arc<dyn SessionStore>,
    session_secret: Arc<Vec<u8>>,
    session_cookie_name: Arc<String>,
    metrics_path: Arc<String>,
}

impl App {
    fn build(config: &Config) -> Result<Self> {
        let mut router = Router::new();
        let health = HealthState::new();
        let renderer = Renderer::default();
        let session_store: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::new());
        let session_secret = Arc::new(config.session_secret.as_bytes().to_vec());
        let session_cookie_name = Arc::new("nanoweb_session".to_string());

        let middlewares = build_middlewares(&config.middleware)?;

        let static_files = config.static_dir.as_ref().map(StaticFiles::new);
        if let Some(static_files) = static_files.clone() {
            let mount = config.static_mount.trim_end_matches('/').to_string();
            let route_pattern = format!("{mount}/*path");
            let handler = Arc::new(move |request: Request| {
                let static_files = static_files.clone();
                async move {
                    let rel = request.params.get("path").cloned().unwrap_or_default();
                    static_files.serve(&request, &rel).await
                }
            });
            router.add_route(Method::Get, route_pattern, handler)?;
        }

        for route in &config.routes {
            let handler = compile_route_handler(route.handler.clone(), renderer.clone())?;
            router.add_route(route.method.clone(), route.path.clone(), handler)?;
        }

        // Built-in health and metrics routes are always present and bypass the configured middleware stack.
        let health_clone = health.clone();
        router.add_route(
            Method::Get,
            "/healthz",
            Arc::new(move |_request: Request| {
                let health_clone = health_clone.clone();
                async move { Ok(health_clone.healthz()) }
            }),
        )?;
        let health_clone = health.clone();
        router.add_route(
            Method::Get,
            "/readyz",
            Arc::new(move |_request: Request| {
                let health_clone = health_clone.clone();
                async move { Ok(health_clone.readyz()) }
            }),
        )?;
        let metrics_path = config.metrics.clone();
        let metrics_path_for_route = metrics_path.clone();
        router.add_route(
            Method::Get,
            metrics_path_for_route,
            Arc::new(move |_request: Request| async move {
                let body = metrics::gather()?;
                Ok(Response::text(StatusCode::Ok, body)
                    .with_header("content-type", "text/plain; version=0.0.4; charset=utf-8"))
            }),
        )?;

        Ok(Self {
            router: Arc::new(router),
            middlewares: Arc::new(middlewares),
            health,
            session_store,
            session_secret,
            session_cookie_name,
            metrics_path: Arc::new(config.metrics.clone()),
        })
    }
}

fn build_middlewares(configs: &[MiddlewareConfig]) -> Result<Vec<Arc<dyn middleware::Middleware>>> {
    let mut middlewares: Vec<Arc<dyn middleware::Middleware>> = Vec::new();
    for config in configs {
        let middleware: Arc<dyn middleware::Middleware> = match config {
            MiddlewareConfig::Logging => Arc::new(RequestLogging),
            MiddlewareConfig::Gzip => Arc::new(GzipCompression),
            MiddlewareConfig::RateLimit {
                permits_per_second,
                burst,
            } => Arc::new(RateLimit::new(RateLimitConfig {
                permits_per_second: *permits_per_second,
                burst: *burst,
            })),
            MiddlewareConfig::Cors { allowed_origins } => Arc::new(Cors::new(CorsConfig {
                allowed_origins: allowed_origins.clone(),
            })),
            MiddlewareConfig::Csrf {
                cookie_name,
                header_name,
            } => Arc::new(Csrf::new(cookie_name.clone(), header_name.clone())),
            MiddlewareConfig::BasicAuth { username, password } => {
                Arc::new(BasicAuth::new(username.clone(), password.clone()))
            }
        };
        middlewares.push(middleware);
    }
    Ok(middlewares)
}

fn compile_route_handler(
    handler: RouteHandlerConfig,
    renderer: Renderer,
) -> Result<Arc<dyn Handler>> {
    Ok(match handler {
        RouteHandlerConfig::Text {
            body,
            status,
            content_type,
        } => Arc::new(move |_request: Request| {
            let body = body.clone();
            let content_type = content_type.clone();
            async move {
                let status = status_from_u16(status);
                let mut response = Response::text(status, body);
                response.headers.insert("content-type", content_type);
                Ok(response)
            }
        }),
        RouteHandlerConfig::Json { body, status } => Arc::new(move |_request: Request| {
            let body = body.clone();
            async move {
                let status = status_from_u16(status);
                let response = Response::json(status, &body)?;
                Ok(response)
            }
        }),
        RouteHandlerConfig::Template {
            template,
            context,
            status,
        } => Arc::new(move |request: Request| {
            let renderer = renderer.clone();
            let template = template.clone();
            let context = context.clone();
            async move {
                let mut data = merge_context(context, &request.params);
                if let Some(session) = request.session.as_ref() {
                    if let Some(value) = session.get::<serde_json::Value>("context") {
                        if let Some(obj) = data.as_object_mut() {
                            if let Some(session_obj) = value.as_object() {
                                for (key, value) in session_obj {
                                    obj.insert(key.clone(), value.clone());
                                }
                            }
                        }
                    }
                }
                let rendered = renderer.render(&template, &data)?;
                let mut response = Response::html(status_from_u16(status), rendered);
                response
                    .headers
                    .insert("content-type", "text/html; charset=utf-8");
                Ok(response)
            }
        }),
        RouteHandlerConfig::EchoBody { status } => Arc::new(move |request: Request| async move {
            let mut response = Response::new(status_from_u16(status));
            response.headers.insert(
                "content-type",
                request
                    .header("content-type")
                    .unwrap_or("application/octet-stream"),
            );
            response.body = request.body.clone();
            Ok(response)
        }),
        RouteHandlerConfig::Empty { status } => Arc::new(move |_request: Request| async move {
            Ok(Response::empty(status_from_u16(status)))
        }),
    })
}

fn merge_context(
    context: serde_json::Value,
    params: &HashMap<String, String>,
) -> serde_json::Value {
    let mut merged = match context {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            map
        }
    };
    for (key, value) in params {
        merged.insert(key.clone(), serde_json::Value::String(value.clone()));
    }
    serde_json::Value::Object(merged)
}

fn status_from_u16(code: u16) -> StatusCode {
    match code {
        200 => StatusCode::Ok,
        201 => StatusCode::Created,
        204 => StatusCode::NoContent,
        301 => StatusCode::MovedPermanently,
        303 => StatusCode::SeeOther,
        304 => StatusCode::NotModified,
        400 => StatusCode::BadRequest,
        401 => StatusCode::Unauthorized,
        403 => StatusCode::Forbidden,
        404 => StatusCode::NotFound,
        405 => StatusCode::MethodNotAllowed,
        408 => StatusCode::RequestTimeout,
        409 => StatusCode::Conflict,
        413 => StatusCode::PayloadTooLarge,
        415 => StatusCode::UnsupportedMediaType,
        429 => StatusCode::TooManyRequests,
        500 => StatusCode::InternalServerError,
        503 => StatusCode::ServiceUnavailable,
        _ => StatusCode::Ok,
    }
}

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_request_id() -> String {
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{counter:x}-{nanos:x}")
}

pub async fn run(config: Config) -> Result<()> {
    let app = Arc::new(App::build(&config)?);
    let ready = app.health.clone();

    let mut plain_listener = None;
    let mut tls_listener = None;
    let mut tls_acceptor = None;

    #[cfg(feature = "tls")]
    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        let acceptor = crate::tls::load_tls_acceptor(cert, key)?;
        let listen = config
            .tls_listen
            .clone()
            .unwrap_or_else(|| config.listen.clone());
        tls_listener = Some(TcpListener::bind(listen).await?);
        tls_acceptor = Some(acceptor);
        if config.tls_listen.is_some() {
            plain_listener = Some(TcpListener::bind(&config.listen).await?);
        }
    } else {
        plain_listener = Some(TcpListener::bind(&config.listen).await?);
    }

    #[cfg(not(feature = "tls"))]
    if config.tls_cert.is_some() || config.tls_key.is_some() {
        return Err(crate::error::NanowebError::Tls(
            "tls support is disabled at compile time".into(),
        ));
    } else {
        plain_listener = Some(TcpListener::bind(&config.listen).await?);
    }

    ready.set_ready(true);

    let mut shutdown_rx = shutdown_signal();
    let mut joinset = JoinSet::new();
    let has_plain = plain_listener.is_some();
    let has_tls = tls_listener.is_some();
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() {
                    break;
                }
                break;
            }
            accepted = async {
                if let Some(listener) = plain_listener.as_mut() {
                    Some(listener.accept().await)
                } else {
                    None
                }
            }, if has_plain => {
                if let Some(accepted) = accepted {
                    let (stream, addr) = accepted?;
                    let app = app.clone();
                    let shutdown = shutdown_rx.clone();
                    joinset.spawn(async move {
                        let _ = serve_stream(stream, addr, app, shutdown).await;
                    });
                }
            }
            accepted = async {
                if let Some(listener) = tls_listener.as_mut() {
                    Some(listener.accept().await)
                } else {
                    None
                }
            }, if has_tls => {
                if let Some(accepted) = accepted {
                    let (stream, addr) = accepted?;
                    let app = app.clone();
                    let shutdown = shutdown_rx.clone();
                    let acceptor = tls_acceptor.clone().expect("tls acceptor");
                    joinset.spawn(async move {
                        match acceptor.accept(stream).await {
                            Ok(stream) => {
                                let _ = serve_stream(stream, addr, app, shutdown).await;
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "tls handshake failed");
                            }
                        }
                    });
                }
            }
        }
    }

    ready.set_ready(false);

    let graceful_timeout = std::time::Duration::from_secs(config.graceful_timeout_s);
    let _ = tokio::time::timeout(graceful_timeout, async {
        while let Some(_result) = joinset.join_next().await {}
    })
    .await;

    Ok(())
}

fn shutdown_signal() -> tokio::sync::watch::Receiver<bool> {
    let (tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("sigterm");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        let _ = tx.send(true);
    });
    rx
}

async fn serve_stream<S>(
    stream: S,
    addr: SocketAddr,
    app: Arc<App>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<()>
where
    S: tokio::io::AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(stream);
    loop {
        if *shutdown.borrow() {
            break;
        }
        let parsed = crate::http::parser::read_request(&mut reader).await;
        let request = match parsed {
            Ok(Some(mut request)) => {
                request.remote_addr = Some(addr);
                request.request_id = next_request_id();
                let cookies = request.cookies();
                let session = Session::from_cookies(
                    &cookies,
                    app.session_cookie_name.as_str(),
                    app.session_store.clone(),
                    app.session_secret.as_ref().clone(),
                )
                .await?;
                request.session = Some(session);
                request
            }
            Ok(None) => break,
            Err(err) => {
                let response = Response::text(StatusCode::BadRequest, err.to_string());
                write_response(
                    &mut reader.get_mut(),
                    &Request {
                        method: Method::Get,
                        target: "/".into(),
                        path: "/".into(),
                        query: None,
                        version: crate::http::Version::Http11,
                        headers: Default::default(),
                        body: vec![],
                        params: Default::default(),
                        remote_addr: Some(addr),
                        request_id: next_request_id(),
                        session: None,
                    },
                    &response,
                    true,
                )
                .await?;
                break;
            }
        };

        let start = Instant::now();
        let route = app.router.lookup(&request.path);
        let route_pattern = route
            .as_ref()
            .map(|m| m.pattern.to_string())
            .unwrap_or_else(|| request.path.clone());
        let session = request.session.clone();
        let response = if matches!(request.path.as_str(), "/healthz" | "/readyz")
            || request.path == app.metrics_path.as_str()
        {
            handle_builtin(&app, &request).await?
        } else {
            handle_route(&app, request.clone(), route).await?
        };

        let mut response = response;
        if let Some(session) = session {
            if let Some(cookie) = session.commit().await? {
                response.headers.insert("set-cookie", cookie);
            }
        }
        response
            .headers
            .insert("x-request-id", request.request_id.clone());

        let status = response.status;
        metrics::record_request(
            &request.method,
            status.as_u16(),
            &route_pattern,
            start.elapsed(),
        );
        let should_close = request
            .header("connection")
            .map(|value| value.eq_ignore_ascii_case("close"))
            .unwrap_or(false);
        write_response(&mut reader.get_mut(), &request, &response, should_close).await?;
        if should_close {
            break;
        }
    }
    Ok(())
}

async fn handle_builtin(app: &App, request: &Request) -> Result<Response> {
    match request.path.as_str() {
        "/healthz" => Ok(app.health.healthz()),
        "/readyz" => Ok(app.health.readyz()),
        path if path == app.metrics_path.as_str() => {
            let body = metrics::gather()?;
            Ok(Response::text(StatusCode::Ok, body)
                .with_header("content-type", "text/plain; version=0.0.4; charset=utf-8"))
        }
        _ => Ok(Response::text(StatusCode::NotFound, "not found")),
    }
}

async fn handle_route(
    app: &App,
    request: Request,
    route: Option<RouteMatch<'_>>,
) -> Result<Response> {
    let Some(route) = route else {
        return Ok(Response::text(StatusCode::NotFound, "not found"));
    };
    dispatch_route(app, request, &route).await
}

async fn dispatch_route(
    app: &App,
    mut request: Request,
    route: &RouteMatch<'_>,
) -> Result<Response> {
    request.params = route.params.clone();
    if matches!(request.method, Method::Options) {
        let allow = allowed_methods(route.handlers)
            .into_iter()
            .map(|method| method.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(Response::empty(StatusCode::Ok).with_header("allow", allow));
    }

    let handler = match route.handlers.get(&request.method) {
        Some(handler) => handler.clone(),
        None if matches!(request.method, Method::Head) => match route.handlers.get(&Method::Get) {
            Some(handler) => handler.clone(),
            None => {
                let allow = allowed_methods(route.handlers)
                    .into_iter()
                    .map(|method| method.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Ok(
                    Response::text(StatusCode::MethodNotAllowed, "method not allowed")
                        .with_header("allow", allow),
                );
            }
        },
        None => {
            let allow = allowed_methods(route.handlers)
                .into_iter()
                .map(|method| method.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Ok(
                Response::text(StatusCode::MethodNotAllowed, "method not allowed")
                    .with_header("allow", allow),
            );
        }
    };

    let response = middleware::execute(app.middlewares.as_ref(), handler, request).await?;
    Ok(response)
}

async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &Request,
    response: &Response,
    close: bool,
) -> Result<()> {
    let status_line = format!("HTTP/1.1 {}\r\n", response.status);
    writer.write_all(status_line.as_bytes()).await?;

    let mut headers = response.headers.clone();
    if close {
        headers.insert("connection", "close");
    } else {
        headers.insert("connection", "keep-alive");
    }

    let body_len = if matches!(
        response.status,
        StatusCode::NoContent | StatusCode::NotModified
    ) {
        0
    } else {
        response.body.len()
    };
    headers.insert("content-length", body_len.to_string());

    for (key, value) in headers.iter() {
        let header = format!("{key}: {value}\r\n");
        writer.write_all(header.as_bytes()).await?;
    }
    writer.write_all(b"\r\n").await?;
    if !matches!(request.method, Method::Head) && body_len > 0 {
        writer.write_all(&response.body).await?;
    }
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_mapping_works() {
        assert_eq!(status_from_u16(404), StatusCode::NotFound);
    }
}
