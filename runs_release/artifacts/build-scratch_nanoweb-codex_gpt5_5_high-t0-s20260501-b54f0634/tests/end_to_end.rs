use base64::{Engine, engine::general_purpose};
use nanoweb::{
    http::{Method, Request, Response},
    middleware::{BasicAuth, Cors, Csrf, Gzip, RateLimit, RequestLogger},
    router::App,
    server::serve_with_shutdown,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

async fn start(app: App) -> (SocketAddr, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        serve_with_shutdown(
            listener,
            Arc::new(app),
            async {
                let _ = rx.await;
            },
            Duration::from_secs(2),
        )
        .await
        .unwrap();
    });
    (addr, tx, handle)
}

async fn request(addr: SocketAddr, raw: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.unwrap();
    out
}

fn app_with_route(method: Method) -> App {
    let mut app = App::new();
    app.add_route(method, "/hit", |_req: Request| async move {
        Response::text(200, "x".repeat(256))
    });
    app
}

#[tokio::test]
async fn request_logger_allows_request() {
    let mut app = app_with_route(Method::Get);
    app.add_middleware(RequestLogger);
    let (addr, shutdown, handle) = start(app).await;
    let response = request(addr, "GET /hit HTTP/1.1\r\nHost: local\r\n\r\n").await;
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    let _ = shutdown.send(());
    handle.await.unwrap();
}

#[tokio::test]
async fn gzip_compresses_response() {
    let mut app = app_with_route(Method::Get);
    app.add_middleware(Gzip::new());
    let (addr, shutdown, handle) = start(app).await;
    let response = request(
        addr,
        "GET /hit HTTP/1.1\r\nHost: local\r\nAccept-Encoding: gzip\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.contains("Content-Encoding: gzip"));
    let _ = shutdown.send(());
    handle.await.unwrap();
}

#[tokio::test]
async fn cors_adds_origin_header() {
    let mut app = app_with_route(Method::Get);
    app.add_middleware(Cors::new(["https://example.test"]));
    let (addr, shutdown, handle) = start(app).await;
    let response = request(
        addr,
        "GET /hit HTTP/1.1\r\nHost: local\r\nOrigin: https://example.test\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&response);
    assert!(text.contains("Access-Control-Allow-Origin: https://example.test"));
    let _ = shutdown.send(());
    handle.await.unwrap();
}

#[tokio::test]
async fn csrf_accepts_double_submit_cookie() {
    let mut app = app_with_route(Method::Post);
    app.add_middleware(Csrf);
    let (addr, shutdown, handle) = start(app).await;
    let response = request(
        addr,
        "POST /hit HTTP/1.1\r\nHost: local\r\nCookie: csrf=t\r\nX-CSRF-Token: t\r\nContent-Length: 0\r\n\r\n",
    )
    .await;
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    let _ = shutdown.send(());
    handle.await.unwrap();
}

#[tokio::test]
async fn basic_auth_guards_route() {
    let mut app = app_with_route(Method::Get);
    app.add_middleware(BasicAuth::new("user", "pass"));
    let (addr, shutdown, handle) = start(app).await;
    let auth = general_purpose::STANDARD.encode("user:pass");
    let response = request(
        addr,
        &format!("GET /hit HTTP/1.1\r\nHost: local\r\nAuthorization: Basic {auth}\r\n\r\n"),
    )
    .await;
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
    let _ = shutdown.send(());
    handle.await.unwrap();
}

#[tokio::test]
async fn rate_limit_blocks_second_request() {
    let mut app = app_with_route(Method::Get);
    app.add_middleware(RateLimit::new(1, 0.0));
    let (addr, shutdown, handle) = start(app).await;
    let first = request(addr, "GET /hit HTTP/1.1\r\nHost: local\r\n\r\n").await;
    let second = request(addr, "GET /hit HTTP/1.1\r\nHost: local\r\n\r\n").await;
    assert!(String::from_utf8_lossy(&first).starts_with("HTTP/1.1 200"));
    assert!(String::from_utf8_lossy(&second).starts_with("HTTP/1.1 429"));
    let _ = shutdown.send(());
    handle.await.unwrap();
}
