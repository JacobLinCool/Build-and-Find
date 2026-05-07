use nanoweb::config::Config;
use nanoweb::http::{Method, Response, StatusCode};
use nanoweb::metrics::Metrics;
use nanoweb::router::Router;
use nanoweb::server::Server;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn ephemeral_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

/// Send a raw HTTP request and read the response.
async fn http_request(port: u16, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();

    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                match result {
                    Ok(0) | Err(_) => break,
                    Ok(n) => response.extend_from_slice(&buf[..n]),
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => break,
        }
    }
    String::from_utf8_lossy(&response).to_string()
}

#[tokio::test]
async fn test_healthz_endpoint() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();
    let router = Router::new();
    let server = Server::new(config, router, vec![], metrics);

    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("OK"));
}

#[tokio::test]
async fn test_readyz_endpoint() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();
    let router = Router::new();
    let server = Server::new(config, router, vec![], metrics);

    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /readyz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();
    let router = Router::new();
    let server = Server::new(config, router, vec![], metrics);

    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("nanoweb"));
}

#[tokio::test]
async fn test_not_found() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();
    let router = Router::new();
    let server = Server::new(config, router, vec![], metrics);

    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("404"));
}

#[tokio::test]
async fn test_custom_route() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();

    let mut router = Router::new();
    router.route(Method::Get, "/hello", |_req| {
        Box::pin(async { Response::text(StatusCode::OK, "Hello, World!") })
    });

    let server = Server::new(config, router, vec![], metrics);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("Hello, World!"));
}

#[tokio::test]
async fn test_request_id_header() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();

    let mut router = Router::new();
    router.route(Method::Get, "/test", |_req| {
        Box::pin(async { Response::text(StatusCode::OK, "ok") })
    });

    let server = Server::new(config, router, vec![], metrics);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /test HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("x-request-id"));
}

#[tokio::test]
async fn test_path_parameters() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();

    let mut router = Router::new();
    router.route(Method::Get, "/users/:id", |req| {
        Box::pin(async move {
            let id = req.params.get("id").cloned().unwrap_or_default();
            Response::text(StatusCode::OK, format!("User: {}", id))
        })
    });

    let server = Server::new(config, router, vec![], metrics);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "GET /users/42 HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("User: 42"));
}

#[tokio::test]
async fn test_method_not_allowed() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();

    let mut router = Router::new();
    router.route(Method::Get, "/items", |_req| {
        Box::pin(async { Response::text(StatusCode::OK, "ok") })
    });

    let server = Server::new(config, router, vec![], metrics);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(
        port,
        "POST /items HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(resp.contains("405"));
    assert!(resp.contains("allow: GET"));
}

#[tokio::test]
async fn test_options_auto_list() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();

    let mut router = Router::new();
    router.route(Method::Get, "/items", |_req| {
        Box::pin(async { Response::text(StatusCode::OK, "ok") })
    });
    router.route(Method::Post, "/items", |_req| {
        Box::pin(async { Response::text(StatusCode::OK, "ok") })
    });

    let server = Server::new(config, router, vec![], metrics);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "OPTIONS /items HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("204") || resp.contains("allow"));
}

#[tokio::test]
async fn test_head_suppresses_body() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();

    let mut router = Router::new();
    router.route(Method::Get, "/page", |_req| {
        Box::pin(async { Response::text(StatusCode::OK, "Hello World") })
    });

    let server = Server::new(config, router, vec![], metrics);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "HEAD /page HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    // HEAD should not have body
    assert!(!resp.contains("Hello World"));
}

#[tokio::test]
async fn test_bad_request() {
    let port = ephemeral_port().await;
    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);
    let metrics = Metrics::new();
    let router = Router::new();
    let server = Server::new(config, router, vec![], metrics);

    tokio::spawn(async move {
        let _ = server.run().await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = http_request(port, "INVALID / HTTP/1.1\r\n\r\n").await;
    assert!(resp.contains("400"));
}
