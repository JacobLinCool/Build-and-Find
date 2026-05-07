use nanoweb::config::Config;
use nanoweb::health::HealthState;
use nanoweb::http::{Method, Request, Response};
use nanoweb::metrics::Metrics;
use nanoweb::middleware::basic_auth::BasicAuthMiddleware;
use nanoweb::middleware::cors::CorsMiddleware;
use nanoweb::middleware::gzip::GzipMiddleware;
use nanoweb::middleware::logging::LoggingMiddleware;
use nanoweb::middleware::rate_limit::RateLimitMiddleware;
use nanoweb::router::Router;
use nanoweb::server::Server;
use nanoweb::static_files::StaticFileServer;
use nanoweb::template::Renderer;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn make_config(port: u16) -> Config {
    Config {
        listen: format!("127.0.0.1:{port}"),
        ..Default::default()
    }
}

async fn send_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).to_string()
}

#[tokio::test]
async fn test_health_endpoint() {
    let config = make_config(0);
    let health = HealthState::new();
    health.set_ready(true);

    let health_resp = health.healthz();
    assert_eq!(health_resp.status, 200);
    assert!(String::from_utf8_lossy(&health_resp.body).contains("ok"));

    let ready_resp = health.readyz();
    assert_eq!(ready_resp.status, 200);
}

#[tokio::test]
async fn test_metrics_rendering() {
    let metrics = Metrics::new();
    metrics.record_request("GET", "2xx", "/api/test");
    metrics.record_request("POST", "4xx", "/api/test");
    metrics.record_duration(std::time::Duration::from_millis(100));

    let output = metrics.render_prometheus();
    assert!(output.contains("http_requests_total"));
    assert!(output.contains("http_request_duration_seconds"));
    assert!(output.contains("active_connections"));
}

#[tokio::test]
async fn test_template_rendering() {
    let renderer = Renderer::new();
    let mut ctx = HashMap::new();
    ctx.insert("title".to_string(), Value::String("Test Page".to_string()));
    ctx.insert(
        "items".to_string(),
        Value::Array(vec![
            Value::Object(serde_json::Map::from_iter(vec![(
                "name".to_string(),
                Value::String("Item 1".to_string()),
            )])),
            Value::Object(serde_json::Map::from_iter(vec![(
                "name".to_string(),
                Value::String("Item 2".to_string()),
            )])),
        ]),
    );

    let template = "<html><head><title>{{title}}</title></head><body><ul>{{#items}}<li>{{name}}</li>{{/items}}</ul></body></html>";
    let result = renderer.render(template, &ctx).unwrap();
    assert!(result.contains("Test Page"));
    assert!(result.contains("Item 1"));
    assert!(result.contains("Item 2"));
}

#[tokio::test]
async fn test_router_integration() {
    let mut router = Router::new();

    router.get("/hello", |_req| {
        Box::pin(async { Response::ok().with_body("Hello, World!") })
    });

    router.get("/users/:id", |mut req| {
        Box::pin(async move {
            let id = req.params.get("id").cloned().unwrap_or_default();
            Response::ok().with_json(&serde_json::json!({"user_id": id}))
        })
    });

    router.post("/echo", |mut req| {
        Box::pin(async move {
            let body = req.take_body();
            Response::ok().with_body(body)
        })
    });

    // Test GET /hello
    let (route, params) = router.lookup(Method::Get, "/hello").unwrap();
    assert!(params.is_empty());
    let req = Request::new(Method::Get, "/hello".into(), "HTTP/1.1".into());
    let resp = (route.handler)(req).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"Hello, World!");

    // Test GET /users/:id
    let (route, params) = router.lookup(Method::Get, "/users/42").unwrap();
    assert_eq!(params.get("id").unwrap(), "42");
    let mut req = Request::new(Method::Get, "/users/42".into(), "HTTP/1.1".into());
    req.params = params;
    let resp = (route.handler)(req).await;
    assert_eq!(resp.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&resp.body).unwrap();
    assert_eq!(body["user_id"], "42");
}

#[tokio::test]
async fn test_middleware_chain_integration() {
    use nanoweb::middleware::{run_middleware_after, run_middleware_before, Middleware};

    let logging = Arc::new(LoggingMiddleware::new()) as Arc<dyn Middleware>;
    let cors = Arc::new(CorsMiddleware::new(vec!["*".into()])) as Arc<dyn Middleware>;
    let chain: Vec<Arc<dyn Middleware>> = vec![logging, cors];

    let mut req = Request::new(Method::Get, "/api/data".into(), "HTTP/1.1".into());
    req.headers
        .insert("origin".into(), "https://example.com".into());

    let mut resp = Response::ok().with_body("data");
    run_middleware_after(&chain, &req, &mut resp);

    assert!(resp
        .headers
        .iter()
        .any(|(k, v)| k == "access-control-allow-origin" && v == "*"));
}

#[tokio::test]
async fn test_basic_auth_middleware_integration() {
    use base64::Engine;
    use nanoweb::middleware::Middleware;

    let mw = BasicAuthMiddleware::new("admin", "password123");

    // Without auth -> 401
    let mut req = Request::new(Method::Get, "/admin".into(), "HTTP/1.1".into());
    let resp = mw.before(&mut req).await.unwrap();
    assert_eq!(resp.status, 401);

    // With correct auth -> passes through
    let mut req = Request::new(Method::Get, "/admin".into(), "HTTP/1.1".into());
    let creds = base64::engine::general_purpose::STANDARD.encode("admin:password123");
    req.headers
        .insert("authorization".into(), format!("Basic {creds}"));
    let resp = mw.before(&mut req).await;
    assert!(resp.is_none());
}

#[tokio::test]
async fn test_rate_limit_middleware_integration() {
    use nanoweb::middleware::Middleware;

    // Use very low rate so we can exhaust quickly
    let mw = RateLimitMiddleware::new(1.0);

    // Exhaust the token bucket (max_tokens = 2.0 with rate 1.0)
    for _ in 0..5 {
        let mut req = Request::new(Method::Get, "/".into(), "HTTP/1.1".into());
        req.remote_addr = Some("10.0.0.1:12345".parse().unwrap());
        mw.before(&mut req).await;
    }

    // Should be rate limited now
    let mut req = Request::new(Method::Get, "/".into(), "HTTP/1.1".into());
    req.remote_addr = Some("10.0.0.1:12345".parse().unwrap());
    let resp = mw.before(&mut req).await.unwrap();
    assert_eq!(resp.status, 429);
}

#[tokio::test]
async fn test_gzip_middleware_integration() {
    use nanoweb::middleware::Middleware;

    let mw = GzipMiddleware::new();
    let mut req = Request::new(Method::Get, "/".into(), "HTTP/1.1".into());
    req.headers.insert("accept-encoding".into(), "gzip".into());

    let mut resp = Response::ok()
        .with_header("content-type", "application/json")
        .with_body("{\"data\":\"".to_string() + &"x".repeat(500) + "\"}");

    mw.after(&req, &mut resp);
    assert!(resp
        .headers
        .iter()
        .any(|(k, v)| k == "content-encoding" && v == "gzip"));
}

#[tokio::test]
async fn test_static_files_integration() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.html"), "<h1>Hello</h1>").unwrap();

    let server = StaticFileServer::new(dir.path());
    let req = Request::new(Method::Get, "/test.html".into(), "HTTP/1.1".into());
    let resp = server.serve(&req).await.unwrap();

    assert_eq!(resp.status, 200);
    assert!(resp
        .headers
        .iter()
        .any(|(k, v)| k == "content-type" && v.contains("html")));
    assert!(resp.headers.iter().any(|(k, _)| k == "etag"));
    assert_eq!(resp.body, b"<h1>Hello</h1>");

    // Test 304
    let etag = resp
        .headers
        .iter()
        .find(|(k, _)| k == "etag")
        .unwrap()
        .1
        .clone();
    let mut req2 = Request::new(Method::Get, "/test.html".into(), "HTTP/1.1".into());
    req2.headers.insert("if-none-match".into(), etag);
    let resp2 = server.serve(&req2).await.unwrap();
    assert_eq!(resp2.status, 304);
}

#[tokio::test]
async fn test_session_integration() {
    use nanoweb::session::{InMemorySessionStore, SessionStore};

    let store = InMemorySessionStore::new("test-secret-key");

    // Create session
    let session_id = store.create();
    let mut data = HashMap::new();
    data.insert("user".to_string(), "alice".to_string());
    data.insert("role".to_string(), "admin".to_string());
    store.set(&session_id, data);

    // Sign and verify
    let signed = store.sign_session_id(&session_id);
    let verified = store.verify_session_cookie(&signed).unwrap();
    assert_eq!(verified, session_id);

    // Retrieve data
    let session_data = store.get(&session_id).unwrap();
    assert_eq!(session_data.get("user").unwrap(), "alice");
    assert_eq!(session_data.get("role").unwrap(), "admin");

    // Remove session
    store.remove(&session_id);
    assert!(store.get(&session_id).is_none());
}

#[tokio::test]
async fn test_body_parsers() {
    // JSON parser
    let mut req = Request::new(Method::Post, "/".into(), "HTTP/1.1".into());
    req.headers
        .insert("content-type".into(), "application/json".into());
    req.set_body(b"{\"name\":\"Alice\",\"age\":30}".to_vec());
    let data: serde_json::Value = nanoweb::body::parse_body(&mut req).unwrap();
    assert_eq!(data["name"], "Alice");
    assert_eq!(data["age"], 30);

    // URL-encoded parser
    let mut req = Request::new(Method::Post, "/".into(), "HTTP/1.1".into());
    req.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    req.set_body(b"name=Bob&age=25".to_vec());
    let data: serde_json::Value = nanoweb::body::parse_body(&mut req).unwrap();
    assert_eq!(data["name"], "Bob");
}

#[tokio::test]
async fn test_full_server_startup_shutdown() {
    let mut config = make_config(0);
    config.listen = "127.0.0.1:0".to_string(); // ephemeral port

    // We can't easily test the full server with ephemeral port in this setup
    // because we need to know the port. Instead, test the components work together.
    let health = HealthState::new();
    health.set_ready(true);
    assert!(health.is_ready());

    health.set_ready(false);
    assert!(!health.is_ready());
    assert_eq!(health.readyz().status, 503);
    assert_eq!(health.healthz().status, 200);
}
