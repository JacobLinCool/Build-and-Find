use nanoweb::config::Config;
use nanoweb::http::{Method, Response, StatusCode};
use nanoweb::middleware::basic_auth::BasicAuth;
use nanoweb::middleware::compression::GzipCompression;
use nanoweb::middleware::cors::Cors;
use nanoweb::middleware::csrf::Csrf;
use nanoweb::middleware::logging::RequestLogger;
use nanoweb::middleware::rate_limit::RateLimit;
use nanoweb::server::App;
use nanoweb::NanowebError;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn spawn_app(build: impl FnOnce(&mut App)) -> (SocketAddr, tokio::sync::oneshot::Sender<()>) {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    build(&mut app);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    (addr, tx)
}

async fn raw_request(addr: SocketAddr, req: &[u8]) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(req).await.unwrap();
    s.shutdown().await.ok();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let body = buf[header_end + 4..].to_vec();
    (head, body)
}

fn handler_ok() -> nanoweb::router::ArcHandler {
    Arc::new(|_req: nanoweb::http::Request| async { Ok::<_, NanowebError>(Response::text("ok")) })
}

#[tokio::test]
async fn basic_routing_and_params() {
    let (addr, _shutdown) = spawn_app(|app| {
        app.route(
            Method::Get,
            "/users/:id",
            Arc::new(|req: nanoweb::http::Request| async move {
                Ok::<_, NanowebError>(Response::text(format!(
                    "id={}",
                    req.param("id").unwrap_or("?")
                )))
            }),
        );
    })
    .await;
    let (head, body) = raw_request(
        addr,
        b"GET /users/42 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"));
    assert_eq!(body, b"id=42");
}

#[tokio::test]
async fn head_uses_get_handler_with_no_body() {
    let (addr, _shutdown) = spawn_app(|app| {
        app.route(Method::Get, "/x", handler_ok());
    })
    .await;
    let (head, body) = raw_request(
        addr,
        b"HEAD /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn options_lists_allowed_methods() {
    let (addr, _shutdown) = spawn_app(|app| {
        app.route(Method::Get, "/x", handler_ok());
        app.route(Method::Post, "/x", handler_ok());
    })
    .await;
    let (head, _) = raw_request(
        addr,
        b"OPTIONS /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.contains("204"));
    assert!(head.to_ascii_lowercase().contains("allow:"));
    assert!(head.contains("GET"));
    assert!(head.contains("POST"));
}

#[tokio::test]
async fn method_not_allowed() {
    let (addr, _shutdown) = spawn_app(|app| {
        app.route(Method::Get, "/x", handler_ok());
    })
    .await;
    let (head, _) = raw_request(
        addr,
        b"DELETE /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 405"));
}

#[tokio::test]
async fn invalid_request_returns_400() {
    let (addr, _shutdown) = spawn_app(|_| {}).await;
    let (head, _) = raw_request(addr, b"NOT-A-REQUEST\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 400"));
}

#[tokio::test]
async fn metrics_endpoint() {
    let (addr, _shutdown) = spawn_app(|app| {
        app.route(Method::Get, "/x", handler_ok());
    })
    .await;
    let _ = raw_request(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (_h, body) = raw_request(
        addr,
        b"GET /metrics HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    let s = String::from_utf8_lossy(&body);
    assert!(s.contains("nanoweb_http_requests_total"));
}

#[tokio::test]
async fn health_endpoints() {
    let (addr, _shutdown) = spawn_app(|_| {}).await;
    let (h, _) = raw_request(
        addr,
        b"GET /healthz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h.starts_with("HTTP/1.1 200"));
    let (h2, _) = raw_request(
        addr,
        b"GET /readyz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h2.starts_with("HTTP/1.1 200"));
}

#[tokio::test]
async fn request_id_header_present() {
    let (addr, _shutdown) = spawn_app(|app| {
        app.route(Method::Get, "/x", handler_ok());
    })
    .await;
    let (head, _) = raw_request(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.to_ascii_lowercase().contains("x-request-id:"));
}

#[tokio::test]
async fn middleware_logging_runs() {
    let (addr, _s) = spawn_app(|app| {
        app.route(Method::Get, "/x", handler_ok());
        // Workaround mutability: use take/replace pattern via Arc.
    })
    .await;
    // Build a separate app that includes logging middleware via with_middleware.
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(Method::Get, "/x", handler_ok());
    let app = app.with_middleware(Arc::new(RequestLogger));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (addr2, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let (head, _) = raw_request(
        addr2,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"));
    drop(tx);
    let _ = addr;
}

#[tokio::test]
async fn middleware_gzip_compresses_large_body() {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(
        Method::Get,
        "/big",
        Arc::new(|_| async { Ok::<_, NanowebError>(Response::text("a".repeat(2048))) }),
    );
    let app = app.with_middleware(Arc::new(GzipCompression::default()));
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let (head, body) = raw_request(
        addr,
        b"GET /big HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.to_ascii_lowercase().contains("content-encoding: gzip"));
    assert!(body.len() < 2048);
    assert_eq!(&body[0..2], &[0x1f, 0x8b]);
}

#[tokio::test]
async fn middleware_rate_limit_returns_429() {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(Method::Get, "/x", handler_ok());
    let app = app.with_middleware(Arc::new(RateLimit::new(1, 0.0)));
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let (h1, _) = raw_request(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h1.starts_with("HTTP/1.1 200"));
    let (h2, _) = raw_request(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h2.starts_with("HTTP/1.1 429"));
}

#[tokio::test]
async fn middleware_cors_preflight() {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(Method::Get, "/x", handler_ok());
    let app = app.with_middleware(Arc::new(Cors::new(vec!["*".into()])));
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let (head, _) = raw_request(
        addr,
        b"OPTIONS /x HTTP/1.1\r\nHost: x\r\nOrigin: https://e.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 204"));
    assert!(head
        .to_ascii_lowercase()
        .contains("access-control-allow-origin"));
}

#[tokio::test]
async fn middleware_csrf_blocks_post_without_token() {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(Method::Post, "/x", handler_ok());
    let app = app.with_middleware(Arc::new(Csrf::default()));
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let (head, _) = raw_request(
        addr,
        b"POST /x HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 403"));
}

#[tokio::test]
async fn middleware_basic_auth_blocks_then_allows() {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(Method::Get, "/x", handler_ok());
    let mut users = HashMap::new();
    users.insert("alice".to_string(), "secret".to_string());
    let app = app.with_middleware(Arc::new(BasicAuth::new("realm", users)));
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let (head, _) = raw_request(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 401"));
    // alice:secret = YWxpY2U6c2VjcmV0
    let (head2, _) = raw_request(
        addr,
        b"GET /x HTTP/1.1\r\nHost: x\r\nAuthorization: Basic YWxpY2U6c2VjcmV0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head2.starts_with("HTTP/1.1 200"));
    let _ = StatusCode::OK;
}

#[tokio::test]
async fn json_body_parsing() {
    let mut cfg = Config::default();
    cfg.listen = "127.0.0.1:0".into();
    let mut app = App::new(cfg);
    app.route(
        Method::Post,
        "/echo",
        Arc::new(|req: nanoweb::http::Request| async move {
            #[derive(serde::Deserialize, serde::Serialize)]
            struct Msg {
                msg: String,
            }
            let m: Msg = req.body_as()?;
            Ok::<_, NanowebError>(Response::json(&m)?)
        }),
    );
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let (addr, _h) = app.serve("127.0.0.1:0".parse().unwrap(), rx).await.unwrap();
    let body = b"{\"msg\":\"hi\"}";
    let req = format!(
        "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut full = req.into_bytes();
    full.extend_from_slice(body);
    let (head, body) = raw_request(addr, &full).await;
    assert!(head.starts_with("HTTP/1.1 200"));
    assert_eq!(&body[..], b"{\"msg\":\"hi\"}");
}
