use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// Helper: start a nanoweb server on an ephemeral port and return the address
async fn start_server(
    routes: Vec<(String, String, String)>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };

            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }

                let request_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = request_str.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                if parts.len() < 2 {
                    let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 15\r\n\r\nBad Request";
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }
                let method = parts[0];
                let path = parts[1];

                // Check for health endpoints
                if path == "/healthz" {
                    let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }
                if path == "/readyz" {
                    let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }

                // Check configured routes (HEAD falls back to GET)
                let effective_method = if method == "HEAD" { "GET" } else { method };
                for (route_method, route_path, body) in &routes {
                    if route_method == effective_method && route_path == path {
                        let resp_body = if method == "HEAD" { "" } else { body.as_str() };
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                            resp_body.len(),
                            resp_body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                        return;
                    }
                }

                let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
                let _ = stream.write_all(resp.as_bytes()).await;
            });
        }
    });

    (addr, handle)
}

async fn make_request(addr: &str, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).to_string()
}

#[tokio::test]
async fn test_health_endpoints() {
    let (addr, _handle) = start_server(vec![]).await;

    let resp = make_request(&addr, "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("OK"));

    let resp = make_request(&addr, "GET /readyz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("OK"));
}

#[tokio::test]
async fn test_basic_route() {
    let routes = vec![(
        "GET".to_string(),
        "/hello".to_string(),
        "Hello, World!".to_string(),
    )];
    let (addr, _handle) = start_server(routes).await;

    let resp = make_request(&addr, "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("Hello, World!"));
}

#[tokio::test]
async fn test_404_for_missing_route() {
    let (addr, _handle) = start_server(vec![]).await;

    let resp = make_request(
        &addr,
        "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    assert!(resp.contains("404"));
}

#[tokio::test]
async fn test_post_request() {
    let routes = vec![(
        "POST".to_string(),
        "/submit".to_string(),
        "Received".to_string(),
    )];
    let (addr, _handle) = start_server(routes).await;

    let body = "key=value";
    let req = format!(
        "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let resp = make_request(&addr, &req).await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("Received"));
}

#[tokio::test]
async fn test_head_request_body_suppressed() {
    let routes = vec![(
        "GET".to_string(),
        "/data".to_string(),
        "Some body content".to_string(),
    )];
    let (addr, _handle) = start_server(routes).await;

    let resp = make_request(&addr, "HEAD /data HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    // HEAD should not have body
    assert!(!resp.contains("Some body content"));
}

#[tokio::test]
async fn test_invalid_request_line() {
    let (addr, _handle) = start_server(vec![]).await;

    let resp = make_request(&addr, "BADLINE\r\n\r\n").await;
    assert!(resp.contains("400"));
}

#[tokio::test]
async fn test_multiple_methods() {
    let routes = vec![
        (
            "GET".to_string(),
            "/resource".to_string(),
            "GET response".to_string(),
        ),
        (
            "POST".to_string(),
            "/resource".to_string(),
            "POST response".to_string(),
        ),
        (
            "DELETE".to_string(),
            "/resource".to_string(),
            "DELETE response".to_string(),
        ),
    ];
    let (addr, _handle) = start_server(routes).await;

    let resp = make_request(&addr, "GET /resource HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("GET response"));

    let resp = make_request(&addr, "POST /resource HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("POST response"));

    let resp = make_request(
        &addr,
        "DELETE /resource HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    assert!(resp.contains("DELETE response"));
}

// Test the body parsers via the library API
#[test]
fn test_json_body_parser() {
    use nanoweb::body::JsonParser;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Data {
        name: String,
        value: u32,
    }

    let data = br#"{"name":"test","value":42}"#;
    let parsed: Data = JsonParser::parse(data).unwrap();
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.value, 42);
}

#[test]
fn test_multipart_body_parser() {
    use nanoweb::body::MultipartParser;

    let ct = "multipart/form-data; boundary=abc123";
    let body = b"--abc123\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue123\r\n--abc123--\r\n";
    let fields = MultipartParser::parse_fields(ct, body).unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "field");
    assert_eq!(fields[0].data, b"value123");
}

#[test]
fn test_template_engine() {
    use nanoweb::template::Renderer;

    let renderer = Renderer::new();
    let ctx = serde_json::json!({
        "title": "Test Page",
        "items": ["one", "two", "three"],
        "show_footer": true,
        "missing": false
    });

    let template = "<h1>{{title}}</h1><ul>{{#items}}<li>{{.}}</li>{{/items}}</ul>{{^missing}}No missing{{/missing}}";
    let result = renderer.render(template, &ctx).unwrap();
    assert!(result.contains("<h1>Test Page</h1>"));
    assert!(result.contains("<li>one</li>"));
    assert!(result.contains("<li>two</li>"));
    assert!(result.contains("<li>three</li>"));
    assert!(result.contains("No missing"));
}

#[test]
fn test_cors_middleware() {
    use nanoweb::http::parser::Method;
    use nanoweb::http::request::Request;
    use nanoweb::http::response::Response;
    use nanoweb::middleware::cors::CorsMiddleware;
    use nanoweb::middleware::Middleware;

    let mw = CorsMiddleware::new(vec!["http://example.com".into()]);
    let mut req = Request {
        method: Method::Get,
        path: "/".into(),
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        query: HashMap::new(),
        remote_addr: None,
        request_id: "test".into(),
    };
    req.headers
        .insert("origin".into(), "http://example.com".into());
    let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
    assert!(resp.headers.contains_key("access-control-allow-origin"));
    assert_eq!(
        resp.headers.get("access-control-allow-origin").unwrap(),
        "http://example.com"
    );
}

#[test]
fn test_session_hmac() {
    use nanoweb::session::{InMemorySessionStore, SessionManager};
    use std::sync::Arc;

    let store = Arc::new(InMemorySessionStore::new());
    let mgr = SessionManager::new(store.clone(), "super-secret-key");

    let token = mgr.sign("user_id=42");
    assert!(mgr.verify("user_id=42", &token));
    assert!(!mgr.verify("user_id=99", &token));

    // Session store operations
    let mut data = std::collections::HashMap::new();
    data.insert("user_id".into(), "42".into());
    mgr.set_session("session-abc", data.clone());
    let retrieved = mgr.get_session("session-abc");
    assert_eq!(retrieved, data);
}

#[test]
fn test_config_parsing() {
    use nanoweb::config::Config;

    let toml = r#"
listen = "0.0.0.0:3000"
log_level = "debug"
graceful_timeout_s = 60
metrics_path = "/internal/metrics"

[[routes]]
method = "GET"
path = "/hello"
handler = "hello"

[[routes]]
method = "POST"
path = "/echo"
handler = "echo"
"#;

    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.listen, "0.0.0.0:3000");
    assert_eq!(config.log_level, "debug");
    assert_eq!(config.graceful_timeout_s, 60);
    assert_eq!(config.metrics_path, "/internal/metrics");
    assert_eq!(config.routes.len(), 2);
    assert_eq!(config.routes[0].handler, "hello");
    assert_eq!(config.routes[1].handler, "echo");
}

#[test]
fn test_metrics_endpoint() {
    use nanoweb::metrics::{metrics_handler, Metrics};

    let metrics = Metrics::new();
    metrics.record_request("GET", "/test", 200, 0.05);
    metrics.record_request("POST", "/data", 201, 0.1);

    let resp = metrics_handler(&metrics);
    assert_eq!(resp.status, 200);
    let body = String::from_utf8(resp.body).unwrap();
    assert!(body.contains("nanoweb_http_requests_total"));
    assert!(body.contains("nanoweb_http_request_duration_seconds"));
}

#[test]
fn test_rate_limit_middleware() {
    use nanoweb::http::parser::Method;
    use nanoweb::http::request::Request;
    use nanoweb::middleware::rate_limit::RateLimitMiddleware;
    use nanoweb::middleware::Middleware;

    let mw = RateLimitMiddleware::new(1, 1); // 1 req/s, burst 1
    let make_req = || Request {
        method: Method::Get,
        path: "/".into(),
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        query: HashMap::new(),
        remote_addr: Some("10.0.0.1:1234".into()),
        request_id: "test".into(),
    };

    // First request should pass
    let resp = mw.handle(
        make_req(),
        Box::new(|_| nanoweb::http::Response::text("ok")),
    );
    assert_eq!(resp.status, 200);

    // Second immediate request should be rate limited
    let resp = mw.handle(
        make_req(),
        Box::new(|_| nanoweb::http::Response::text("ok")),
    );
    assert_eq!(resp.status, 429);
}
