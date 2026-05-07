use base64::{Engine, engine::general_purpose};
use nanoweb::http::{Method, Request, Response, StatusCode};
use nanoweb::metrics::Metrics;
use nanoweb::middleware::{
    Middleware, auth::BasicAuth, cors::Cors, csrf::Csrf, gzip::Gzip, logging::Logging,
    rate_limit::RateLimit,
};
use nanoweb::router::{App, Router};
use nanoweb::server::handle_connection;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

async fn roundtrip(middleware: Vec<Arc<dyn Middleware>>, request: &str) -> String {
    let mut router = Router::new();
    router.add(Method::Get, "/", |_req: Request| async {
        Response::text(
            StatusCode::Ok,
            "hello world hello world hello world hello world",
        )
    });
    router.add(Method::Post, "/", |_req: Request| async {
        Response::text(StatusCode::Ok, "posted")
    });
    let app = App::new(router).with_middleware(middleware);
    let metrics = Arc::new(Metrics::default());
    let (mut client, server) = duplex(8192);
    let peer: SocketAddr = "127.0.0.1:5555".parse().unwrap();
    let task =
        tokio::spawn(async move { handle_connection(server, peer, app, metrics).await.unwrap() });
    client.write_all(request.as_bytes()).await.unwrap();
    client.shutdown().await.unwrap();
    let mut out = Vec::new();
    client.read_to_end(&mut out).await.unwrap();
    task.await.unwrap();
    String::from_utf8_lossy(&out).to_string()
}

#[tokio::test]
async fn logging_middleware_end_to_end() {
    let res = roundtrip(vec![Arc::new(Logging)], "GET / HTTP/1.1\r\nHost: x\r\n\r\n").await;
    assert!(res.contains("200 OK"));
    assert!(res.contains("X-Request-Id:"));
}

#[tokio::test]
async fn gzip_middleware_end_to_end() {
    let res = roundtrip(
        vec![Arc::new(Gzip)],
        "GET / HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip\r\n\r\n",
    )
    .await;
    assert!(res.contains("Content-Encoding: gzip"));
}

#[tokio::test]
async fn cors_middleware_end_to_end() {
    let res = roundtrip(
        vec![Arc::new(Cors::new(vec!["https://example.test".into()]))],
        "GET / HTTP/1.1\r\nHost: x\r\nOrigin: https://example.test\r\n\r\n",
    )
    .await;
    assert!(res.contains("Access-Control-Allow-Origin: https://example.test"));
}

#[tokio::test]
async fn csrf_middleware_end_to_end() {
    let res = roundtrip(
        vec![Arc::new(Csrf)],
        "POST / HTTP/1.1\r\nHost: x\r\nCookie: csrf_token=a\r\nX-CSRF-Token: a\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(res.contains("200 OK"));
}

#[tokio::test]
async fn basic_auth_middleware_end_to_end() {
    let auth = general_purpose::STANDARD.encode("u:p");
    let res = roundtrip(
        vec![Arc::new(BasicAuth::new("u".into(), "p".into()))],
        &format!("GET / HTTP/1.1\r\nHost: x\r\nAuthorization: Basic {auth}\r\n\r\n"),
    )
    .await;
    assert!(res.contains("200 OK"));
}

#[tokio::test]
async fn rate_limit_middleware_end_to_end() {
    let res = roundtrip(
        vec![Arc::new(RateLimit::new(1, 1.0))],
        "GET / HTTP/1.1\r\nHost: x\r\n\r\n",
    )
    .await;
    assert!(res.contains("200 OK"));
}
