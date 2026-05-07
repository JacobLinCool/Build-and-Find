use std::net::SocketAddr;

use base64::Engine;
use nanoweb::config::{
    BasicAuthConfig, Config, CorsConfig, MiddlewareConfig, RateLimitConfig, RouteConfig,
};
use nanoweb::server::{App, accept_loop};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

async fn spawn(config: Config) -> (SocketAddr, watch::Sender<bool>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = App::from_config(&config).unwrap();
    app.set_ready(true);
    let (tx, rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        accept_loop(listener, app, rx, &mut tasks).await.unwrap();
        while tasks.join_next().await.is_some() {}
    });
    (addr, tx, handle)
}

async fn request(addr: SocketAddr, raw: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

fn config_with(middleware: MiddlewareConfig) -> Config {
    Config {
        listen: "127.0.0.1:0".into(),
        routes: vec![RouteConfig {
            method: "GET".into(),
            path: "/".into(),
            response: "hello middleware".into(),
            status: 200,
            content_type: "text/plain".into(),
        }],
        middleware,
        ..Config::default()
    }
}

#[tokio::test]
async fn logging_and_gzip_are_exercised_end_to_end() {
    let (addr, shutdown, handle) = spawn(config_with(MiddlewareConfig {
        logging: true,
        gzip: true,
        ..MiddlewareConfig::default()
    }))
    .await;

    let out = request(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(out.contains("200 OK"));
    assert!(out.contains("Content-Encoding: gzip"));
    assert!(out.contains("X-Request-Id:"));
    shutdown.send(true).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn cors_preflight_is_exercised_end_to_end() {
    let (addr, shutdown, handle) = spawn(config_with(MiddlewareConfig {
        cors: Some(CorsConfig {
            allowed_origins: vec!["https://example.com".into()],
        }),
        ..MiddlewareConfig::default()
    }))
    .await;

    let out = request(
        addr,
        "OPTIONS / HTTP/1.1\r\nHost: localhost\r\nOrigin: https://example.com\r\nAccess-Control-Request-Method: GET\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(out.contains("204 No Content"));
    assert!(out.contains("Access-Control-Allow-Origin: https://example.com"));
    shutdown.send(true).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn rate_limit_is_exercised_end_to_end() {
    let (addr, shutdown, handle) = spawn(config_with(MiddlewareConfig {
        rate_limit: Some(RateLimitConfig {
            capacity: 1,
            refill_per_second: 0.1,
        }),
        ..MiddlewareConfig::default()
    }))
    .await;

    let first = request(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    let second = request(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(first.contains("200 OK"));
    assert!(second.contains("429 Too Many Requests"));
    shutdown.send(true).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn csrf_and_basic_auth_are_exercised_end_to_end() {
    let (addr, shutdown, handle) = spawn(config_with(MiddlewareConfig {
        csrf: true,
        basic_auth: Some(BasicAuthConfig {
            username: "u".into(),
            password: "p".into(),
        }),
        ..MiddlewareConfig::default()
    }))
    .await;

    let unauthorized = request(
        addr,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(unauthorized.contains("401 Unauthorized"));

    let auth = base64::engine::general_purpose::STANDARD.encode("u:p");
    let rejected = request(
        addr,
        &format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Basic {auth}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    )
    .await;
    assert!(rejected.contains("403 Forbidden"));
    shutdown.send(true).unwrap();
    handle.await.unwrap();
}
