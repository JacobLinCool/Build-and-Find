use async_trait::async_trait;
use nanoweb::config::Config;
use nanoweb::http::{Request, Response, StatusCode};
use nanoweb::middleware::{BasicAuth, Compression, Cors, Csrf, RateLimit, RequestLogger};
use nanoweb::router::Handler;
use nanoweb::server::{run_with_listener, App};
use serde::Deserialize;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// A spawned in-process server bound to an ephemeral port.
struct Spawned {
    addr: std::net::SocketAddr,
    shutdown: watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

impl Spawned {
    async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.handle).await;
    }
}

async fn spawn(app: App) -> Spawned {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = watch::channel(false);
    let app = Arc::new(app);
    let h = tokio::spawn(async move {
        run_with_listener(app, listener, rx).await;
    });
    Spawned {
        addr,
        shutdown: tx,
        handle: h,
    }
}

fn cfg() -> Config {
    let cli = nanoweb::cli::CliArgs {
        listen: None,
        config: None,
        tls_cert: None,
        tls_key: None,
        metrics: None,
        static_dir: None,
        log_level: None,
    };
    let mut c = Config::load(&cli).unwrap();
    // make rate-limit unlikely to interfere by default
    c.rate_limit_capacity = 1000.0;
    c.rate_limit_refill_per_sec = 1000.0;
    c
}

/// Minimal HTTP/1.1 client: sends raw bytes, returns (status, headers, body).
async fn http_request_raw(
    addr: std::net::SocketAddr,
    raw: &[u8],
) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(raw).await.unwrap();
    s.flush().await.unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf).await;
    parse_response(&buf)
}

fn parse_response(buf: &[u8]) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let head_end = find(buf, b"\r\n\r\n").expect("response head");
    let head = std::str::from_utf8(&buf[..head_end]).unwrap();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let mut sl = status_line.split(' ');
    let _ = sl.next();
    let status: u16 = sl.next().unwrap().parse().unwrap();
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut body = buf[head_end + 4..].to_vec();
    if let Some((_, te)) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Transfer-Encoding"))
    {
        if te.eq_ignore_ascii_case("chunked") {
            body = decode_chunked(&body);
        }
    }
    (status, headers, body)
}

fn decode_chunked(buf: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let line_end = find(&buf[pos..], b"\r\n").unwrap();
        let size_line = std::str::from_utf8(&buf[pos..pos + line_end]).unwrap();
        let size = usize::from_str_radix(size_line.split(';').next().unwrap().trim(), 16).unwrap();
        pos += line_end + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&buf[pos..pos + size]);
        pos += size + 2;
    }
    out
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    for i in 0..=haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn req(method: &str, path: &str, host: &str) -> Vec<u8> {
    format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").into_bytes()
}

// ---------------- Tests ----------------

struct EchoHandler;
#[async_trait]
impl Handler for EchoHandler {
    async fn handle(&self, req: Request) -> Response {
        let id = req.param("id").unwrap_or("").to_string();
        let mut r = Response::ok();
        r.headers.insert("Content-Type", "text/plain");
        r.body = format!("id={}", id).into_bytes();
        r
    }
}

#[tokio::test]
async fn end_to_end_param_route() {
    let mut app = App::new(cfg());
    app.router.get("/users/:id", EchoHandler);
    let s = spawn(app).await;
    let (status, _, body) = http_request_raw(s.addr, &req("GET", "/users/42", "x")).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"id=42");
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_head_falls_back_to_get_no_body() {
    let mut app = App::new(cfg());
    app.router.get("/foo", |_r: Request| async {
        Response::text("hello world".repeat(10))
    });
    let s = spawn(app).await;
    let (status, headers, body) = http_request_raw(s.addr, &req("HEAD", "/foo", "x")).await;
    assert_eq!(status, 200);
    assert!(body.is_empty());
    assert!(header(&headers, "Content-Length").is_some());
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_options_lists_allowed() {
    let mut app = App::new(cfg());
    app.router
        .get("/r", |_r: Request| async { Response::text("g") });
    app.router
        .post("/r", |_r: Request| async { Response::text("p") });
    let s = spawn(app).await;
    let (status, headers, _) = http_request_raw(s.addr, &req("OPTIONS", "/r", "x")).await;
    assert!(status == 204 || status == 200);
    let allow = header(&headers, "Allow").unwrap_or("");
    assert!(allow.contains("GET"));
    assert!(allow.contains("POST"));
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_404_and_400() {
    let app = App::new(cfg());
    let s = spawn(app).await;
    let (status, _, _) = http_request_raw(s.addr, &req("GET", "/missing", "x")).await;
    assert_eq!(status, 404);
    // malformed: missing target
    let bad = b"GET HTTP/1.1\r\n\r\n";
    let (status, _, _) = http_request_raw(s.addr, bad).await;
    assert_eq!(status, 400);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_chunked_body_post() {
    let mut app = App::new(cfg());
    app.router.post("/echo", |req: Request| async move {
        let mut r = Response::ok();
        r.body = req.body;
        r
    });
    let s = spawn(app).await;
    let raw = b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    let (status, _, body) = http_request_raw(s.addr, raw).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"hello world");
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_json_body() {
    #[derive(Deserialize)]
    struct Pair {
        a: i32,
        b: i32,
    }
    let mut app = App::new(cfg());
    app.router.post("/sum", |req: Request| async move {
        let p: Pair = match req.body_as() {
            Ok(p) => p,
            Err(e) => {
                let mut r = Response::new(StatusCode::BAD_REQUEST);
                r.body = e.to_string().into_bytes();
                return r;
            }
        };
        Response::text(format!("{}", p.a + p.b))
    });
    let s = spawn(app).await;
    let raw = b"POST /sum HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"a\":1,\"b\":2}";
    let (status, _, body) = http_request_raw(s.addr, raw).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"3");
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_logging_middleware_sets_request_id() {
    let mut app = App::new(cfg());
    app.chain = app.chain.push(RequestLogger::new());
    app.router
        .get("/x", |_r: Request| async { Response::text("ok") });
    let s = spawn(app).await;
    let (_, headers, _) = http_request_raw(s.addr, &req("GET", "/x", "x")).await;
    assert!(header(&headers, "X-Request-Id").is_some());
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_compression_middleware() {
    let mut app = App::new(cfg());
    app.chain = app.chain.push(Compression::with_min_size(0));
    app.router.get("/z", |_r: Request| async {
        Response::text("a".repeat(2048))
    });
    let s = spawn(app).await;
    let raw = b"GET /z HTTP/1.1\r\nHost: x\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n";
    let (status, headers, body) = http_request_raw(s.addr, raw).await;
    assert_eq!(status, 200);
    assert_eq!(header(&headers, "Content-Encoding"), Some("gzip"));
    // gzip header magic
    assert_eq!(&body[..2], &[0x1f, 0x8b]);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_rate_limit_middleware() {
    let mut app = App::new(cfg());
    app.chain = app.chain.push(RateLimit::new(2.0, 0.0));
    app.router
        .get("/q", |_r: Request| async { Response::text("ok") });
    let s = spawn(app).await;
    let mut allowed = 0;
    let mut limited = 0;
    for _ in 0..5 {
        let (status, _, _) = http_request_raw(s.addr, &req("GET", "/q", "x")).await;
        if status == 200 {
            allowed += 1;
        } else if status == 429 {
            limited += 1;
        }
    }
    assert_eq!(allowed, 2);
    assert!(limited >= 1);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_cors_preflight() {
    let mut app = App::new(cfg());
    app.chain = app
        .chain
        .push(Cors::new(vec!["https://example.com".into()]));
    app.router
        .get("/r", |_r: Request| async { Response::text("g") });
    let s = spawn(app).await;
    let raw = b"OPTIONS /r HTTP/1.1\r\nHost: x\r\nOrigin: https://example.com\r\nConnection: close\r\n\r\n";
    let (status, headers, _) = http_request_raw(s.addr, raw).await;
    assert_eq!(status, 204);
    assert_eq!(
        header(&headers, "Access-Control-Allow-Origin"),
        Some("https://example.com")
    );
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_csrf_blocks_post_without_token() {
    let mut app = App::new(cfg());
    app.chain = app.chain.push(Csrf::new());
    app.router
        .post("/x", |_r: Request| async { Response::text("ok") });
    let s = spawn(app).await;
    let raw = b"POST /x HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let (status, _, _) = http_request_raw(s.addr, raw).await;
    assert_eq!(status, 403);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_basic_auth_blocks_then_allows() {
    let mut app = App::new(cfg());
    app.chain = app
        .chain
        .push(BasicAuth::new("test", vec![("u".into(), "p".into())]));
    app.router
        .get("/p", |_r: Request| async { Response::text("ok") });
    let s = spawn(app).await;
    let (status, _, _) = http_request_raw(s.addr, &req("GET", "/p", "x")).await;
    assert_eq!(status, 401);

    let creds = base64::engine::general_purpose::STANDARD.encode("u:p".as_bytes());
    let raw = format!(
        "GET /p HTTP/1.1\r\nHost: x\r\nAuthorization: Basic {}\r\nConnection: close\r\n\r\n",
        creds
    );
    let (status, _, _) = http_request_raw(s.addr, raw.as_bytes()).await;
    assert_eq!(status, 200);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_static_files_etag() {
    let dir = tempfile_dir();
    std::fs::write(dir.join("hello.txt"), b"hello").unwrap();
    let mut app = App::new(cfg()).with_static_dir(&dir);
    app.router
        .get("/x", |_r: Request| async { Response::text("ok") });
    let s = spawn(app).await;
    let (status, headers, body) =
        http_request_raw(s.addr, &req("GET", "/static/hello.txt", "x")).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"hello");
    let etag = header(&headers, "ETag").unwrap().to_string();
    let raw = format!(
        "GET /static/hello.txt HTTP/1.1\r\nHost: x\r\nIf-None-Match: {}\r\nConnection: close\r\n\r\n",
        etag
    );
    let (status, _, _) = http_request_raw(s.addr, raw.as_bytes()).await;
    assert_eq!(status, 304);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_static_files_traversal_rejected() {
    let dir = tempfile_dir();
    std::fs::write(dir.join("a.txt"), b"a").unwrap();
    let app = App::new(cfg()).with_static_dir(&dir);
    let s = spawn(app).await;
    // /../etc/passwd attempt via percent-decoded path is not done by our parser,
    // but try literal traversal
    let (status, _, _) = http_request_raw(s.addr, &req("GET", "/static/../etc/passwd", "x")).await;
    assert!(status == 403 || status == 404);
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_metrics_endpoint() {
    let mut app = App::new(cfg());
    let metrics = app.metrics.clone();
    let path = app.config.metrics_path.clone();
    app.router
        .get("/x", |_r: Request| async { Response::text("ok") });
    app.router.get(
        &path,
        MetricsHandler {
            metrics: metrics.clone(),
        },
    );
    let s = spawn(app).await;
    let _ = http_request_raw(s.addr, &req("GET", "/x", "x")).await;
    let (status, _, body) = http_request_raw(s.addr, &req("GET", "/metrics", "x")).await;
    assert_eq!(status, 200);
    let text = std::str::from_utf8(&body).unwrap();
    assert!(text.contains("nanoweb_http_requests_total"));
    s.shutdown().await;
}

struct MetricsHandler {
    metrics: nanoweb::metrics::Metrics,
}

#[async_trait]
impl Handler for MetricsHandler {
    async fn handle(&self, _req: Request) -> Response {
        let mut r = Response::ok();
        r.headers
            .insert("Content-Type", "text/plain; version=0.0.4");
        r.body = self.metrics.render_prometheus().into_bytes();
        r
    }
}

#[tokio::test]
async fn end_to_end_health_endpoints() {
    let mut app = App::new(cfg());
    let h = app.health.clone();
    app.router.get("/healthz", HealthLive { h: h.clone() });
    app.router.get("/readyz", HealthReady { h: h.clone() });
    let s = spawn(app).await;
    let (status, _, _) = http_request_raw(s.addr, &req("GET", "/healthz", "x")).await;
    assert_eq!(status, 200);
    let (status, _, _) = http_request_raw(s.addr, &req("GET", "/readyz", "x")).await;
    assert_eq!(status, 200);
    s.shutdown().await;
}

struct HealthLive {
    h: nanoweb::health::Health,
}
#[async_trait]
impl Handler for HealthLive {
    async fn handle(&self, _req: Request) -> Response {
        self.h.liveness_response()
    }
}

struct HealthReady {
    h: nanoweb::health::Health,
}
#[async_trait]
impl Handler for HealthReady {
    async fn handle(&self, _req: Request) -> Response {
        self.h.readiness_response()
    }
}

#[tokio::test]
async fn end_to_end_template_rendered_html() {
    let mut app = App::new(cfg());
    app.router.get("/page", |_r: Request| async {
        let renderer = nanoweb::template::Renderer::new();
        let body = renderer
            .render(
                "<h1>{{title}}</h1>{{#items}}<li>{{.}}</li>{{/items}}",
                &serde_json::json!({"title":"hi","items":["a","b"]}),
            )
            .unwrap();
        Response::html(body)
    });
    let s = spawn(app).await;
    let (status, headers, body) = http_request_raw(s.addr, &req("GET", "/page", "x")).await;
    assert_eq!(status, 200);
    assert!(header(&headers, "Content-Type")
        .unwrap()
        .contains("text/html"));
    assert_eq!(body, b"<h1>hi</h1><li>a</li><li>b</li>");
    s.shutdown().await;
}

#[tokio::test]
async fn end_to_end_full_chain_smoke() {
    let mut app = App::new(cfg());
    app.chain = app
        .chain
        .push(RequestLogger::new())
        .push(Compression::with_min_size(0))
        .push(RateLimit::new(100.0, 100.0))
        .push(Cors::new(vec!["*".into()]));
    app.router
        .get("/hello", |_r: Request| async { Response::text("hello") });
    let s = spawn(app).await;
    let (status, _, _) = http_request_raw(s.addr, &req("GET", "/hello", "x")).await;
    assert_eq!(status, 200);
    s.shutdown().await;
}

// ---- helpers ----

fn tempfile_dir() -> std::path::PathBuf {
    use rand::RngCore;
    let mut buf = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut buf);
    let p = std::env::temp_dir().join(format!("nanoweb-test-{}", hex::encode(buf)));
    std::fs::create_dir_all(&p).unwrap();
    p
}

use base64::Engine as _;
