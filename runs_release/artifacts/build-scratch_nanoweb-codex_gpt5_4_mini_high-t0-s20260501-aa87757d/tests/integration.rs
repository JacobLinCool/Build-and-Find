use async_trait::async_trait;
use base64::Engine;
use flate2::read::GzDecoder;
use nanoweb::{
    app::{App, AppState},
    config::{Config, MiddlewareConfig},
    http::{
        parser::{parse_request, ParseError},
        types::{Method, Request, Response, StatusCode},
    },
    middleware::Handler,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::Read, net::SocketAddr, sync::Arc};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Person {
    name: String,
    age: u8,
}

struct PersonHandler;
struct ParamHandler;

#[async_trait]
impl Handler for PersonHandler {
    async fn handle(&self, req: Request, _ctx: Arc<AppState>) -> nanoweb::Result<Response> {
        let person: Person = req.body_as()?;
        Response::json(&person)
    }
}

#[async_trait]
impl Handler for ParamHandler {
    async fn handle(&self, req: Request, _ctx: Arc<AppState>) -> nanoweb::Result<Response> {
        Ok(Response::text(
            StatusCode::Ok,
            format!(
                "{}:{}",
                req.param("id").unwrap_or(""),
                req.param("path").unwrap_or("")
            ),
        ))
    }
}

struct TestServer {
    addr: SocketAddr,
    shutdown: oneshot::Sender<()>,
    join: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn shutdown(self) {
        let _ = self.shutdown.send(());
        let _ = self.join.await;
    }
}

async fn spawn_test_server(app: Arc<App>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut rx => {
                    break;
                }
                accept = listener.accept() => {
                    let (stream, remote_addr) = match accept {
                        Ok(value) => value,
                        Err(_) => break,
                    };
                    let app = app.clone();
                    tokio::spawn(async move {
                        let _ = serve_connection(stream, remote_addr, app).await;
                    });
                }
            }
        }
    });
    TestServer {
        addr,
        shutdown: tx,
        join,
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    remote_addr: SocketAddr,
    app: Arc<App>,
) -> nanoweb::Result<()> {
    let mut buffer = Vec::new();
    let mut temp = [0u8; 4096];
    loop {
        match parse_request(&buffer) {
            Ok((parsed, consumed)) => {
                buffer.drain(..consumed);
                let request = Request {
                    method: parsed.method,
                    target: parsed.target,
                    path: parsed.path,
                    query: parsed.query,
                    version: parsed.version,
                    headers: parsed.headers,
                    body: parsed.body,
                    params: HashMap::new(),
                    request_id: "test".to_string(),
                    remote_addr: Some(remote_addr),
                    route_pattern: None,
                    connection_close: parsed.connection_close,
                };
                let response = app.handle(request).await?;
                let bytes = response.into_http_bytes(Some("test"), false);
                stream.write_all(&bytes).await?;
                break;
            }
            Err(ParseError::Incomplete) => {
                let n = stream.read(&mut temp).await?;
                if n == 0 {
                    break;
                }
                buffer.extend_from_slice(&temp[..n]);
            }
            Err(ParseError::Protocol(err)) => {
                let bytes =
                    Response::text(StatusCode::BadRequest, err).into_http_bytes(None, false);
                let _ = stream.write_all(&bytes).await;
                break;
            }
        }
    }
    Ok(())
}

fn base_config() -> Config {
    Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        tls_listen: None,
        tls_cert: None,
        tls_key: None,
        metrics: "/metrics".to_string(),
        static_mount: "/static".to_string(),
        static_dir: None,
        log_level: "info".to_string(),
        graceful_timeout_s: 1,
        session_secret: Some("secret".to_string()),
        session_cookie_name: None,
        routes: vec![],
        middleware: vec![],
        cors: None,
        rate_limit: None,
        csrf: None,
        basic_auth: None,
    }
}

async fn client_request(
    addr: SocketAddr,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> reqwest::Response {
    let client = reqwest::Client::builder().build().unwrap();
    let mut req = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap(),
        format!("http://{}{}", addr, path),
    );
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    if !body.is_empty() {
        req = req.body(body);
    }
    req.send().await.unwrap()
}

#[tokio::test]
async fn body_parsers_route_params_head_options_and_metrics_work() {
    let mut config = base_config();
    config.middleware.push(MiddlewareConfig::Logging);
    let mut app = App::from_config(config).unwrap();
    app.router
        .add(
            Method::GET,
            "/users/:id",
            Arc::new(ParamHandler),
            Vec::new(),
        )
        .unwrap();
    app.router
        .add(Method::POST, "/people", Arc::new(PersonHandler), Vec::new())
        .unwrap();

    let server = spawn_test_server(Arc::new(app)).await;
    let addr = server.addr;

    let response = client_request(addr, Method::GET, "/users/42", &[], Vec::new()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "42:");

    let response = client_request(addr, Method::HEAD, "/users/42", &[], Vec::new()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "");

    let response = client_request(addr, Method::OPTIONS, "/users/42", &[], Vec::new()).await;
    assert_eq!(response.status(), 200);
    let allow = response
        .headers()
        .get("allow")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(allow.contains("GET"));
    assert!(allow.contains("HEAD"));
    assert!(allow.contains("OPTIONS"));

    let json = client_request(
        addr,
        Method::POST,
        "/people",
        &[("content-type", "application/json")],
        br#"{"name":"Ada","age":32}"#.to_vec(),
    )
    .await;
    assert_eq!(json.status(), 200);
    assert_eq!(
        json.json::<Person>().await.unwrap(),
        Person {
            name: "Ada".into(),
            age: 32
        }
    );

    let form = client_request(
        addr,
        Method::POST,
        "/people",
        &[("content-type", "application/x-www-form-urlencoded")],
        b"name=Ada&age=32".to_vec(),
    )
    .await;
    assert_eq!(form.status(), 200);
    assert_eq!(
        form.json::<Person>().await.unwrap(),
        Person {
            name: "Ada".into(),
            age: 32
        }
    );

    let boundary = "boundary123";
    let multipart = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nAda\r\n--{b}\r\nContent-Disposition: form-data; name=\"age\"\r\n\r\n32\r\n--{b}--\r\n",
        b = boundary
    );
    let multipart_response = client_request(
        addr,
        Method::POST,
        "/people",
        &[(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )],
        multipart.into_bytes(),
    )
    .await;
    assert_eq!(multipart_response.status(), 200);
    assert_eq!(
        multipart_response.json::<Person>().await.unwrap(),
        Person {
            name: "Ada".into(),
            age: 32
        }
    );

    let metrics = client_request(addr, Method::GET, "/metrics", &[], Vec::new()).await;
    assert_eq!(metrics.status(), 200);
    let text = metrics.text().await.unwrap();
    assert!(text.contains("nanoweb_http_requests_total"));

    server.shutdown().await;
}

#[tokio::test]
async fn gzip_cors_rate_limit_basic_auth_and_csrf_work() {
    let mut config = base_config();
    config.middleware.push(MiddlewareConfig::Logging);
    config.middleware.push(MiddlewareConfig::Gzip);
    config.middleware.push(MiddlewareConfig::Cors {
        allowed_origins: vec!["https://example.com".to_string()],
    });
    config.middleware.push(MiddlewareConfig::Csrf {
        cookie_name: "csrf".to_string(),
        header_name: "x-csrf-token".to_string(),
    });
    config.middleware.push(MiddlewareConfig::BasicAuth {
        username: "user".to_string(),
        password: "pass".to_string(),
    });
    let mut app = App::from_config(config).unwrap();
    app.router
        .add(
            Method::GET,
            "/protected",
            Arc::new(TextHandler("hello")),
            Vec::new(),
        )
        .unwrap();
    app.router
        .add(
            Method::GET,
            "/submit",
            Arc::new(TextHandler("submitted")),
            Vec::new(),
        )
        .unwrap();
    app.router
        .add(
            Method::POST,
            "/submit",
            Arc::new(TextHandler("submitted")),
            Vec::new(),
        )
        .unwrap();

    let server = spawn_test_server(Arc::new(app)).await;
    let addr = server.addr;

    let auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("user:pass")
    );

    let gzip = client_request(
        addr,
        Method::GET,
        "/protected",
        &[("authorization", &auth), ("accept-encoding", "gzip")],
        Vec::new(),
    )
    .await;
    assert_eq!(gzip.status(), 200);
    assert_eq!(gzip.headers().get("content-encoding").unwrap(), "gzip");
    let compressed = gzip.bytes().await.unwrap();
    let mut decoder = GzDecoder::new(compressed.as_ref());
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    assert_eq!(decompressed, "hello");

    let cors = client_request(
        addr,
        Method::GET,
        "/protected",
        &[("authorization", &auth), ("origin", "https://example.com")],
        Vec::new(),
    )
    .await;
    assert_eq!(
        cors.headers().get("access-control-allow-origin").unwrap(),
        "https://example.com"
    );

    let unauthorized = client_request(addr, Method::GET, "/protected", &[], Vec::new()).await;
    assert_eq!(unauthorized.status(), 401);

    let cookie_response = client_request(
        addr,
        Method::GET,
        "/submit",
        &[("authorization", &auth)],
        Vec::new(),
    )
    .await;
    assert_eq!(cookie_response.status(), 200);
    let cookie = cookie_response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let token = cookie
        .split(';')
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(_, value)| value.to_string())
        .unwrap();

    let csrf_ok = client_request(
        addr,
        Method::POST,
        "/submit",
        &[
            ("authorization", &auth),
            ("cookie", &format!("csrf={token}")),
            ("x-csrf-token", &token),
        ],
        Vec::new(),
    )
    .await;
    assert_eq!(csrf_ok.status(), 200);

    let csrf_fail = client_request(
        addr,
        Method::POST,
        "/submit",
        &[
            ("authorization", &auth),
            ("cookie", &format!("csrf={token}")),
        ],
        Vec::new(),
    )
    .await;
    assert_eq!(csrf_fail.status(), 403);

    server.shutdown().await;
}

#[tokio::test]
async fn rate_limit_enforces_burst() {
    let mut config = base_config();
    config.middleware.push(MiddlewareConfig::Logging);
    config.middleware.push(MiddlewareConfig::RateLimit {
        tokens_per_second: 0.0,
        burst: 1,
    });
    let mut app = App::from_config(config).unwrap();
    app.router
        .add(
            Method::GET,
            "/limited",
            Arc::new(TextHandler("limited")),
            Vec::new(),
        )
        .unwrap();

    let server = spawn_test_server(Arc::new(app)).await;
    let addr = server.addr;

    let first = client_request(addr, Method::GET, "/limited", &[], Vec::new()).await;
    assert_eq!(first.status(), 200);

    let second = client_request(addr, Method::GET, "/limited", &[], Vec::new()).await;
    assert_eq!(second.status(), 429);

    server.shutdown().await;
}

#[tokio::test]
async fn static_files_and_etag_work() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();

    let mut config = base_config();
    config.static_dir = Some(dir.path().to_path_buf());
    config.middleware.push(MiddlewareConfig::Logging);
    let app = App::from_config(config).unwrap();
    let server = spawn_test_server(Arc::new(app)).await;
    let addr = server.addr;

    let response = client_request(addr, Method::GET, "/static/hello.txt", &[], Vec::new()).await;
    assert_eq!(response.status(), 200);
    let etag = response
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(response.text().await.unwrap(), "hi");

    let cached = client_request(
        addr,
        Method::GET,
        "/static/hello.txt",
        &[("if-none-match", &etag)],
        Vec::new(),
    )
    .await;
    assert_eq!(cached.status(), 304);

    server.shutdown().await;
}

struct TextHandler(&'static str);

#[async_trait]
impl Handler for TextHandler {
    async fn handle(&self, _req: Request, _ctx: Arc<AppState>) -> nanoweb::Result<Response> {
        Ok(Response::text(StatusCode::Ok, self.0))
    }
}
