use base64::Engine;
use nanoweb::config::{Config, MiddlewareConfig, RouteConfig};
use nanoweb::server::Server;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use tokio::sync::Notify;

fn config(middleware: MiddlewareConfig) -> Config {
    Config {
        listen: "127.0.0.1:0".into(),
        tls_cert: None,
        tls_key: None,
        metrics: "/metrics".into(),
        static_dir: None,
        log_level: "info".into(),
        graceful_timeout_s: 1,
        session_secret: "secret".into(),
        routes: vec![RouteConfig {
            method: "GET".into(),
            path: "/hello/:name".into(),
            response: "hello".into(),
        }],
        middleware,
    }
}

async fn one_request(mut cfg: Config, raw: &str) -> String {
    let raw = raw.to_string();
    let std_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = std_listener.local_addr().unwrap();
    drop(std_listener);
    cfg.listen = addr.to_string();
    let server = Server::from_config(cfg).unwrap();
    let shutdown = Arc::new(Notify::new());
    let stop = shutdown.clone();
    let task = tokio::spawn(async move { server.run_until(stop).await.unwrap() });
    let response = tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut out = Vec::new();
        stream.read_to_end(&mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    })
    .await
    .unwrap();
    shutdown.notify_waiters();
    task.await.unwrap();
    response
}

#[tokio::test]
async fn logging_adds_request_id() {
    let mw = MiddlewareConfig {
        logging: true,
        ..Default::default()
    };
    let res = one_request(config(mw), "GET /hello/bob HTTP/1.1\r\nHost: x\r\n\r\n").await;
    assert!(res.contains("x-request-id:"));
}

#[tokio::test]
async fn gzip_compresses() {
    let mw = MiddlewareConfig {
        gzip: true,
        ..Default::default()
    };
    let res = one_request(
        config(mw),
        "GET /hello/bob HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip\r\n\r\n",
    )
    .await;
    assert!(res.contains("content-encoding: gzip"));
}

#[tokio::test]
async fn cors_sets_origin() {
    let mw = MiddlewareConfig {
        cors_origins: vec!["https://example.test".into()],
        ..Default::default()
    };
    let res = one_request(
        config(mw),
        "GET /hello/bob HTTP/1.1\r\nHost: x\r\nOrigin: https://example.test\r\n\r\n",
    )
    .await;
    assert!(res.contains("access-control-allow-origin: https://example.test"));
}

#[tokio::test]
async fn basic_auth_blocks_and_allows() {
    let mw = MiddlewareConfig {
        basic_auth_user: Some("u".into()),
        basic_auth_password: Some("p".into()),
        ..Default::default()
    };
    let blocked = one_request(
        config(mw.clone()),
        "GET /hello/bob HTTP/1.1\r\nHost: x\r\n\r\n",
    )
    .await;
    assert!(blocked.contains("401 Unauthorized"));
    let token = base64::engine::general_purpose::STANDARD.encode("u:p");
    let allowed = one_request(
        config(mw),
        &format!("GET /hello/bob HTTP/1.1\r\nHost: x\r\nAuthorization: Basic {token}\r\n\r\n"),
    )
    .await;
    assert!(allowed.contains("200 OK"));
}

#[tokio::test]
async fn rate_limit_rejects_when_empty() {
    let mw = MiddlewareConfig {
        rate_limit_capacity: Some(0),
        rate_limit_refill_per_s: Some(0.0),
        ..Default::default()
    };
    let res = one_request(config(mw), "GET /hello/bob HTTP/1.1\r\nHost: x\r\n\r\n").await;
    assert!(res.contains("429 Too Many Requests"));
}

#[tokio::test]
async fn csrf_blocks_unsafe_without_token() {
    let mut cfg = config(MiddlewareConfig {
        csrf: true,
        ..Default::default()
    });
    cfg.routes = vec![RouteConfig {
        method: "POST".into(),
        path: "/submit".into(),
        response: "ok".into(),
    }];
    let res = one_request(
        cfg,
        "POST /submit HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(res.contains("403 Forbidden"));
}
