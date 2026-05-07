use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use nanoweb::config::Config;
use nanoweb::http::{Method, Response};
use nanoweb::middleware::logging::LoggingMiddleware;
use nanoweb::middleware::Middleware;
use nanoweb::router::{RouteEntry, Router};
use nanoweb::server::Server;

async fn start_server(router: Router) -> (u16, Arc<Server>) {
    let port = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };

    let config = Config {
        listen: format!("127.0.0.1:{port}"),
        ..Default::default()
    };

    // Use minimal middleware for tests (no CSRF, no rate limit)
    let middleware: Vec<Box<dyn Middleware>> = vec![Box::new(LoggingMiddleware::new())];
    let server = Arc::new(Server::with_middleware(config, router, middleware));

    let server_run = server.clone();
    tokio::spawn(async move {
        let _ = server_run.run().await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    (port, server)
}

fn make_router() -> Router {
    let mut router = Router::new();

    router.add_route(
        Method::Get,
        "/",
        RouteEntry {
            handler: Box::new(|_req| Box::pin(async { Response::html(200, "<h1>Home</h1>") })),
            middleware: vec![],
        },
    );

    router.add_route(
        Method::Get,
        "/hello",
        RouteEntry {
            handler: Box::new(|req| {
                Box::pin(async move {
                    let name = req.query_param("name").unwrap_or("World");
                    Response::text(200, &format!("Hello, {name}!"))
                })
            }),
            middleware: vec![],
        },
    );

    router.add_route(
        Method::Post,
        "/echo",
        RouteEntry {
            handler: Box::new(|req| {
                Box::pin(async move { Response::text(200, &String::from_utf8_lossy(&req.body)) })
            }),
            middleware: vec![],
        },
    );

    router.add_route(
        Method::Get,
        "/users/:id",
        RouteEntry {
            handler: Box::new(|req| {
                Box::pin(async move {
                    let id = req.param("id").unwrap_or("unknown");
                    Response::text(200, &format!("User: {id}"))
                })
            }),
            middleware: vec![],
        },
    );

    router.add_route(
        Method::Get,
        "/static/*path",
        RouteEntry {
            handler: Box::new(|req| {
                Box::pin(async move {
                    let path = req.param("path").unwrap_or("");
                    Response::text(200, &format!("Static: {path}"))
                })
            }),
            middleware: vec![],
        },
    );

    router
}

async fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).to_string()
}

async fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).to_string()
}

#[tokio::test]
async fn test_home_endpoint() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("<h1>Home</h1>"));
}

#[tokio::test]
async fn test_hello_endpoint() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/hello?name=Test").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("Hello, Test!"));
}

#[tokio::test]
async fn test_echo_endpoint() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_post(port, "/echo", "test body").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("test body"));
}

#[tokio::test]
async fn test_path_parameters() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/users/42").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("User: 42"));
}

#[tokio::test]
async fn test_wildcard_route() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/static/css/style.css").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("Static: css/style.css"));
}

#[tokio::test]
async fn test_404_not_found() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/nonexistent").await;
    assert!(resp.contains("404 Not Found"));
}

#[tokio::test]
async fn test_healthz_endpoint() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/healthz").await;
    assert!(resp.contains("200 OK"));
}

#[tokio::test]
async fn test_readyz_endpoint() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/readyz").await;
    assert!(resp.contains("200 OK") || resp.contains("503"));
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/metrics").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("nanoweb_http_requests_total"));
}

#[tokio::test]
async fn test_head_fallback() {
    let router = make_router();
    let (port, _server) = start_server(router).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let request = "HEAD / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response_str = String::from_utf8_lossy(&response).to_string();

    assert!(response_str.contains("200 OK"));
    assert!(response_str.contains("content-length:"));
}

#[tokio::test]
async fn test_bad_request() {
    let router = make_router();
    let (port, _server) = start_server(router).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream
        .write_all(b"INVALID / HTTP/1.1\r\n\r\n")
        .await
        .unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response_str = String::from_utf8_lossy(&response).to_string();

    assert!(response_str.contains("400 Bad Request"));
}

#[tokio::test]
async fn test_request_id_header() {
    let router = make_router();
    let (port, _server) = start_server(router).await;
    let resp = http_get(port, "/").await;
    assert!(resp.contains("x-request-id:"));
}
