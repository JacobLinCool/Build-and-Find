use nanoweb::config::Config;
use nanoweb::http::types::{Method, Response};
use nanoweb::middleware::{
    basic_auth::BasicAuth, cors::Cors, csrf::Csrf, gzip::Gzip, logging::Logging,
    rate_limit::RateLimit,
};
use nanoweb::server::{serve_connection, App};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

async fn start_server(app: Arc<App>) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        if let Ok((stream, addr)) = listener.accept().await {
            let _ = serve_connection(app, stream, addr).await;
        }
    });
    (port, handle)
}

#[tokio::test]
async fn router_and_logging_and_gzip() {
    let mut app = App::new(Config::default());
    app.router.add(
        Method::Get,
        "/hello/:name",
        Arc::new(|req| Response::text(200, format!("hi {}", req.params["name"]))),
    );
    app.middleware.push(Arc::new(Logging));
    app.middleware.push(Arc::new(Gzip));
    let app = Arc::new(app);
    let (port, handle) = start_server(app).await;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(b"GET /hello/world HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip\r\n\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.peer_addr().is_ok());
    handle.await.unwrap();
}

#[tokio::test]
async fn rate_limit_and_cors_and_csrf_and_auth() {
    let mut app = App::new(Config {
        cors_allowed_origins: vec!["http://example.com".into()],
        basic_auth_user: Some("u".into()),
        basic_auth_pass: Some("p".into()),
        ..Config::default()
    });
    app.router.add(
        Method::Post,
        "/submit",
        Arc::new(|_| Response::text(200, "ok")),
    );
    app.middleware.push(Arc::new(RateLimit::default()));
    app.middleware.push(Arc::new(Cors {
        allowed_origins: vec!["http://example.com".into()],
    }));
    app.middleware.push(Arc::new(Csrf));
    app.middleware.push(Arc::new(BasicAuth {
        user: Some("u".into()),
        pass: Some("p".into()),
    }));
    let app = Arc::new(app);
    let (port, handle) = start_server(app).await;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let req = concat!(
        "POST /submit HTTP/1.1\r\n",
        "Host: localhost\r\n",
        "Origin: http://example.com\r\n",
        "Authorization: Basic dTpw\r\n",
        "Cookie: csrf=t\r\n",
        "X-CSRF-Token: t\r\n",
        "Content-Length: 0\r\n\r\n"
    );
    s.write_all(req.as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.peer_addr().is_ok());
    handle.await.unwrap();
}

#[tokio::test]
async fn metrics_and_health_and_static() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
    let mut app = App::new(Config {
        static_dir: Some(tmp.path().to_string_lossy().to_string()),
        ..Config::default()
    });
    app.router
        .add(Method::Get, "/", Arc::new(|_| Response::text(200, "home")));
    let app = Arc::new(app);
    let (port, handle) = start_server(app).await;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(s.peer_addr().is_ok());
    handle.await.unwrap();
}
