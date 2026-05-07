use std::net::TcpListener;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use nanoweb::config::Config;
use nanoweb::error::NanowebError;
use nanoweb::http::types::{Method, Request, Response};
use nanoweb::middleware::MiddlewareStack;
use nanoweb::router::Router;
use nanoweb::server::Server;

fn get_ephemeral_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn send_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut resp = Vec::new();
    let mut buf = [0u8; 4096];
    // Read with timeout
    loop {
        match tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => resp.extend_from_slice(&buf[..n]),
            Ok(Err(_)) => break,
        }
    }
    String::from_utf8_lossy(&resp).to_string()
}

async fn start_test_server(port: u16) -> tokio::task::JoinHandle<()> {
    let mut router = Router::new();
    router.add_route(Method::GET, "/hello", |_req: Request| async {
        Ok::<Response, NanowebError>(Response::ok().with_text("hello world"))
    });
    router.add_route(Method::GET, "/echo/:msg", |req: Request| async move {
        let msg = req.params.get("msg").cloned().unwrap_or_default();
        Ok::<Response, NanowebError>(Response::ok().with_text(msg))
    });
    router.add_route(Method::POST, "/data", |req: Request| async move {
        let body = String::from_utf8_lossy(&req.body).to_string();
        Ok::<Response, NanowebError>(Response::ok().with_text(body))
    });

    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);

    let server = Server::new(config).with_router(router);

    tokio::spawn(async move {
        let _ = server.run().await;
    })
}

#[tokio::test]
async fn test_simple_get() {
    let port = get_ephemeral_port();
    let _handle = start_test_server(port).await;

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = send_request(
        port,
        "GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("200 OK"), "Response: {}", resp);
    assert!(resp.contains("hello world"), "Response: {}", resp);
}

#[tokio::test]
async fn test_path_param() {
    let port = get_ephemeral_port();
    let _handle = start_test_server(port).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = send_request(
        port,
        "GET /echo/rustlang HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("200 OK"), "Response: {}", resp);
    assert!(resp.contains("rustlang"), "Response: {}", resp);
}

#[tokio::test]
async fn test_not_found() {
    let port = get_ephemeral_port();
    let _handle = start_test_server(port).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = send_request(
        port,
        "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("404"), "Response: {}", resp);
}

#[tokio::test]
async fn test_post_with_body() {
    let port = get_ephemeral_port();
    let _handle = start_test_server(port).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body = "test body data";
    let request = format!(
        "POST /data HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let resp = send_request(port, &request).await;
    assert!(resp.contains("200 OK"), "Response: {}", resp);
    assert!(resp.contains("test body data"), "Response: {}", resp);
}

#[tokio::test]
async fn test_method_not_allowed() {
    let port = get_ephemeral_port();
    let _handle = start_test_server(port).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // GET /data doesn't exist, only POST
    let resp = send_request(
        port,
        "DELETE /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.contains("405"), "Response: {}", resp);
}
