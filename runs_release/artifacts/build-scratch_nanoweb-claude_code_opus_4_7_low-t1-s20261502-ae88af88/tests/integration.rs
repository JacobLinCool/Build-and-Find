use nanoweb::http::Response;
use nanoweb::middleware::{BasicAuth, Cors, Csrf, Gzip, Logging, RateLimit};
use nanoweb::router::handler;
use nanoweb::server::{App, Server};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn spawn_server(app: App) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let server = Server::new(app);
    tokio::spawn(async move {
        let _ = server.run(addr).await;
    });
    // Wait for server to come up
    for _ in 0..50 {
        if TcpStream::connect(addr).await.is_ok() {
            return addr;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    addr
}

async fn http_request(addr: SocketAddr, raw: &str) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(raw.as_bytes()).await.unwrap();
    s.shutdown().await.ok();
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), s.read_to_end(&mut buf)).await;
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let body = if split + 4 < buf.len() {
        buf[split + 4..].to_vec()
    } else {
        Vec::new()
    };
    (head, body)
}

#[tokio::test]
async fn basic_get() {
    let mut app = App::new();
    app.middleware.push(Arc::new(Logging));
    app.router.get(
        "/hello/:name",
        handler(
            |req| async move { Response::text(format!("hi {}", req.param("name").unwrap_or("?"))) },
        ),
    );
    let addr = spawn_server(app).await;
    let (head, body) = http_request(
        addr,
        "GET /hello/world HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"));
    assert_eq!(body, b"hi world");
    assert!(head.contains("X-Request-Id"));
}

#[tokio::test]
async fn head_falls_back_to_get() {
    let mut app = App::new();
    app.router
        .get("/x", handler(|_r| async move { Response::text("hello") }));
    let addr = spawn_server(app).await;
    let (head, body) = http_request(
        addr,
        "HEAD /x HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn options_lists_methods() {
    let mut app = App::new();
    app.router.get("/y", handler(|_r| async { Response::ok() }));
    app.router
        .post("/y", handler(|_r| async { Response::ok() }));
    let addr = spawn_server(app).await;
    let (head, _) = http_request(
        addr,
        "OPTIONS /y HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.contains("Allow:"));
}

#[tokio::test]
async fn metrics_endpoint() {
    let mut app = App::new();
    app.router
        .get("/x", handler(|_r| async { Response::text("ok") }));
    let addr = spawn_server(app).await;
    let _ = http_request(
        addr,
        "GET /x HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    let (head, body) = http_request(
        addr,
        "GET /metrics HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"));
    let s = String::from_utf8_lossy(&body);
    assert!(s.contains("nanoweb_http_requests_total"));
}

#[tokio::test]
async fn health_endpoints() {
    let app = App::new();
    let addr = spawn_server(app).await;
    let (h1, _) = http_request(
        addr,
        "GET /healthz HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h1.starts_with("HTTP/1.1 200"));
    let (h2, _) = http_request(
        addr,
        "GET /readyz HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h2.starts_with("HTTP/1.1 200"));
}

#[tokio::test]
async fn gzip_middleware() {
    let mut app = App::new();
    app.middleware.push(Arc::new(Gzip { min_size: 1 }));
    app.router.get(
        "/big",
        handler(|_r| async { Response::text("a".repeat(2000)) }),
    );
    let addr = spawn_server(app).await;
    let (head, body) = http_request(
        addr,
        "GET /big HTTP/1.1\r\nHost: y\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.contains("Content-Encoding: gzip"));
    assert!(body.starts_with(&[0x1f, 0x8b]));
}

#[tokio::test]
async fn rate_limit_middleware() {
    let mut app = App::new();
    app.middleware.push(Arc::new(RateLimit::new(1, 0.0)));
    app.router
        .get("/x", handler(|_r| async { Response::text("ok") }));
    let addr = spawn_server(app).await;
    let (h1, _) = http_request(
        addr,
        "GET /x HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h1.starts_with("HTTP/1.1 200"));
    let (h2, _) = http_request(
        addr,
        "GET /x HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h2.starts_with("HTTP/1.1 429"));
}

#[tokio::test]
async fn cors_middleware() {
    let mut app = App::new();
    app.middleware.push(Arc::new(Cors::new()));
    app.router
        .get("/x", handler(|_r| async { Response::text("ok") }));
    let addr = spawn_server(app).await;
    let (head, _) = http_request(
        addr,
        "GET /x HTTP/1.1\r\nHost: y\r\nOrigin: https://a.com\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.contains("Access-Control-Allow-Origin"));
}

#[tokio::test]
async fn csrf_middleware_blocks_post() {
    let mut app = App::new();
    app.middleware.push(Arc::new(Csrf::new()));
    app.router
        .post("/x", handler(|_r| async { Response::text("ok") }));
    let addr = spawn_server(app).await;
    let (head, _) = http_request(
        addr,
        "POST /x HTTP/1.1\r\nHost: y\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 403"));
}

#[tokio::test]
async fn basic_auth_middleware() {
    let mut app = App::new();
    app.middleware.push(Arc::new(BasicAuth::new("u", "p")));
    app.router
        .get("/x", handler(|_r| async { Response::text("ok") }));
    let addr = spawn_server(app).await;
    let (h1, _) = http_request(
        addr,
        "GET /x HTTP/1.1\r\nHost: y\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h1.starts_with("HTTP/1.1 401"));
    // base64 of "u:p" = dTpw
    let (h2, _) = http_request(
        addr,
        "GET /x HTTP/1.1\r\nHost: y\r\nAuthorization: Basic dTpw\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(h2.starts_with("HTTP/1.1 200"));
}

#[tokio::test]
async fn malformed_request_400() {
    let app = App::new();
    let addr = spawn_server(app).await;
    let (head, _) = http_request(addr, "BREW /coffee HTCPCP\r\n\r\n").await;
    assert!(head.starts_with("HTTP/1.1 400"));
}

#[tokio::test]
async fn json_body() {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Q {
        name: String,
    }
    let mut app = App::new();
    app.router.post(
        "/echo",
        handler(|req| async move {
            let v: Q = req.body_as().unwrap();
            Response::text(v.name)
        }),
    );
    let addr = spawn_server(app).await;
    let body = b"{\"name\":\"alice\"}";
    let raw = format!(
        "POST /echo HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        std::str::from_utf8(body).unwrap()
    );
    let (head, body) = http_request(addr, &raw).await;
    assert!(head.starts_with("HTTP/1.1 200"));
    assert_eq!(body, b"alice");
}
