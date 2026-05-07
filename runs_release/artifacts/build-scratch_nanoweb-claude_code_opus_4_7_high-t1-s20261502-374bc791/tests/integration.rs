//! End-to-end integration tests: start the server on an ephemeral port and
//! exercise handlers, including one per standard middleware.

use std::sync::Arc;
use std::time::Duration;

use nanoweb::config::{
    BasicAuthConfig, Config, CorsConfig, MiddlewareConfig, RateLimitConfig, RouteConfig,
    RouteResponse,
};
use nanoweb::http::Method;
use nanoweb::middleware::{BoxFuture, Middleware, Next};
use nanoweb::router::Handler;
use nanoweb::server::{serve, App};
use nanoweb::{Request, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

fn base_config() -> Config {
    Config {
        listen: "127.0.0.1:0".into(),
        tls_cert: None,
        tls_key: None,
        metrics_path: "/metrics".into(),
        static_dir: None,
        log_level: "warn".into(),
        graceful_timeout_s: 5,
        session_secret: "test-secret".into(),
        middleware: MiddlewareConfig::default(),
        routes: Vec::new(),
    }
}

async fn spawn_server(
    app: Arc<App>,
) -> (
    std::net::SocketAddr,
    Arc<Notify>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(Notify::new());
    let s = shutdown.clone();
    let handle = tokio::spawn(async move {
        let _ = serve(listener, app, s).await;
    });
    (addr, shutdown, handle)
}

/// Send a raw HTTP request and read the response back into a (head, body) pair.
async fn send_raw(addr: std::net::SocketAddr, raw: &[u8]) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw).await.unwrap();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let body = buf[split + 4..].to_vec();
    (head, body)
}

fn parse_status(head: &str) -> u16 {
    let line = head.lines().next().unwrap();
    let parts: Vec<&str> = line.split_whitespace().collect();
    parts[1].parse().unwrap()
}

fn header(head: &str, name: &str) -> Option<String> {
    for line in head.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn add_simple_route(app: &App, method: Method, pattern: &str, body: &'static str) {
    let body = body.to_string();
    let handler: Handler = Arc::new(move |_req: &mut Request| {
        let body = body.clone();
        Box::pin(async move { Ok(Response::ok().text(body)) })
    });
    app.router.route(method, pattern, handler);
}

#[tokio::test]
async fn echoes_path_param_and_query() {
    let cfg = base_config();
    let app = Arc::new(App::from_config(&cfg));
    let handler: Handler = Arc::new(|req: &mut Request| {
        let id = req.params.get("id").cloned().unwrap_or_default();
        let q = req.query.clone();
        Box::pin(async move { Ok(Response::ok().text(format!("id={} q={}", id, q))) })
    });
    app.router.route(Method::Get, "/users/:id", handler);

    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, body) = send_raw(
        addr,
        b"GET /users/42?x=1 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert_eq!(body, b"id=42 q=x=1");
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn returns_404_for_unknown_route() {
    let app = Arc::new(App::from_config(&base_config()));
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"GET /missing HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 404);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn head_returns_no_body_but_content_length() {
    let cfg = base_config();
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "hello");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, body) = send_raw(
        addr,
        b"HEAD /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert!(body.is_empty());
    assert_eq!(header(&head, "content-length").as_deref(), Some("5"));
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn options_lists_allowed_methods() {
    let cfg = base_config();
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "g");
    add_simple_route(&app, Method::Post, "/x", "p");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"OPTIONS /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 204);
    let allow = header(&head, "allow").unwrap();
    assert!(allow.contains("GET"));
    assert!(allow.contains("POST"));
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn chunked_request_body_is_decoded() {
    let cfg = base_config();
    let app = Arc::new(App::from_config(&cfg));
    let handler: Handler = Arc::new(|req: &mut Request| {
        let body = req.body.clone();
        Box::pin(async move { Ok(Response::ok().text(String::from_utf8_lossy(&body).to_string())) })
    });
    app.router.route(Method::Post, "/echo", handler);
    let (addr, shutdown, handle) = spawn_server(app).await;
    let raw = b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    let (head, body) = send_raw(addr, raw).await;
    assert_eq!(parse_status(&head), 200);
    assert_eq!(body, b"hello world");
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn rejects_malformed_request() {
    let app = Arc::new(App::from_config(&base_config()));
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(addr, b"NOT-AN-HTTP REQUEST\r\n\r\n").await;
    assert_eq!(parse_status(&head), 400);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn metrics_endpoint_exposes_counters() {
    let cfg = base_config();
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "ok");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let _ = send_raw(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (head, body) = send_raw(
        addr,
        b"GET /metrics HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    let s = String::from_utf8_lossy(&body);
    assert!(s.contains("nanoweb_http_requests_total"));
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn health_endpoints_respond() {
    let app = Arc::new(App::from_config(&base_config()));
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"GET /healthz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    let (head, _) = send_raw(
        addr,
        b"GET /readyz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn json_body_round_trip() {
    let cfg = base_config();
    let app = Arc::new(App::from_config(&cfg));
    let handler: Handler = Arc::new(|req: &mut Request| {
        let parsed: serde_json::Value = req.body_as().unwrap_or(serde_json::Value::Null);
        Box::pin(async move {
            let mut resp = Response::ok();
            resp.headers.insert("content-type", "application/json");
            resp.body = serde_json::to_vec(&parsed).unwrap();
            Ok(resp)
        })
    });
    app.router.route(Method::Post, "/json", handler);
    let (addr, shutdown, handle) = spawn_server(app).await;
    let raw = b"POST /json HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"a\":1,\"b\":2}";
    let (head, body) = send_raw(addr, raw).await;
    assert_eq!(parse_status(&head), 200);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["a"], 1);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

// === One handler-end-to-end per standard middleware === //

#[tokio::test]
async fn logging_middleware_attaches_request_id() {
    let mut cfg = base_config();
    cfg.middleware.logging = true;
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "ok");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert!(header(&head, "x-request-id").is_some());
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn gzip_middleware_compresses_response() {
    let mut cfg = base_config();
    cfg.middleware.logging = false;
    cfg.middleware.gzip = true;
    let app = Arc::new(App::from_config(&cfg));
    let big = "abcdefgh".repeat(200);
    let big_clone = big.clone();
    let handler: Handler = Arc::new(move |_req: &mut Request| {
        let big = big_clone.clone();
        Box::pin(async move { Ok(Response::ok().text(big)) })
    });
    app.router.route(Method::Get, "/big", handler);
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, body) = send_raw(
        addr,
        b"GET /big HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert_eq!(header(&head, "content-encoding").as_deref(), Some("gzip"));
    assert!(body.len() < big.len());
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn rate_limit_middleware_blocks_after_capacity() {
    let mut cfg = base_config();
    cfg.middleware.logging = false;
    cfg.middleware.rate_limit = Some(RateLimitConfig {
        capacity: 2,
        refill_per_sec: 0.0,
    });
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "ok");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let mut last_status = 0u16;
    for _ in 0..3 {
        let (head, _) = send_raw(
            addr,
            b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;
        last_status = parse_status(&head);
    }
    assert_eq!(last_status, 429);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn cors_middleware_allows_configured_origin() {
    let mut cfg = base_config();
    cfg.middleware.logging = false;
    cfg.middleware.cors = Some(CorsConfig {
        allowed_origins: vec!["https://example.com".into()],
    });
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "ok");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nOrigin: https://example.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert_eq!(
        header(&head, "access-control-allow-origin").as_deref(),
        Some("https://example.com")
    );
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn csrf_middleware_blocks_unsafe_without_token() {
    let mut cfg = base_config();
    cfg.middleware.logging = false;
    cfg.middleware.csrf = true;
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Post, "/x", "ok");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"POST /x HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 403);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn basic_auth_middleware_blocks_then_admits() {
    let mut cfg = base_config();
    cfg.middleware.logging = false;
    cfg.middleware.basic_auth = Some(BasicAuthConfig {
        realm: "test".into(),
        credentials: vec![("alice".into(), "secret".into())],
    });
    let app = Arc::new(App::from_config(&cfg));
    add_simple_route(&app, Method::Get, "/x", "ok");
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 401);
    use base64::Engine;
    let token = base64::engine::general_purpose::STANDARD.encode("alice:secret");
    let req = format!(
        "GET /x HTTP/1.1\r\nHost: x\r\nAuthorization: Basic {}\r\nConnection: close\r\n\r\n",
        token
    );
    let (head, _) = send_raw(addr, req.as_bytes()).await;
    assert_eq!(parse_status(&head), 200);
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

#[tokio::test]
async fn templated_route_response() {
    let mut cfg = base_config();
    cfg.middleware.logging = false;
    cfg.routes.push(RouteConfig {
        method: "GET".into(),
        path: "/hi".into(),
        response: RouteResponse::Template {
            template: "Hello, {{name}}!".into(),
            context: serde_json::json!({"name": "Ada"}),
        },
    });
    let app = Arc::new(App::from_config(&cfg));
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, body) = send_raw(
        addr,
        b"GET /hi HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert_eq!(body, b"Hello, Ada!");
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}

// Custom middleware demonstrates the trait surface.
struct Tagger;
impl Middleware for Tagger {
    fn handle<'a>(
        &'a self,
        req: &'a mut nanoweb::Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, nanoweb::Result<nanoweb::Response>> {
        Box::pin(async move {
            let mut resp = next.run(req).await?;
            resp.headers.insert("x-tagger", "1");
            Ok(resp)
        })
    }
}

#[tokio::test]
async fn custom_middleware_can_be_inserted() {
    let cfg = base_config();
    let app = nanoweb::server::App::from_config(&cfg);
    add_simple_route(&app, Method::Get, "/x", "ok");
    // Mutate the app's middleware list before serving.
    let mut app_inner = app;
    app_inner.middleware.push(Arc::new(Tagger));
    let app = Arc::new(app_inner);
    let (addr, shutdown, handle) = spawn_server(app).await;
    let (head, _) = send_raw(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(parse_status(&head), 200);
    assert_eq!(header(&head, "x-tagger").as_deref(), Some("1"));
    shutdown.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}
