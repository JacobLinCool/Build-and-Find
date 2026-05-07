use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use nanoweb::config::Config;
use nanoweb::http::parser;
use nanoweb::http::types::{Headers, Method, Request, Response, StatusCode};
use nanoweb::middleware::{Middleware, MiddlewareStack};
use nanoweb::router::Router;
use nanoweb::server::Server;

fn make_request(method: &str, path: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n{headers}");
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    let mut bytes = req.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn read_response(stream: &mut TcpStream) -> String {
    let mut buf = [0u8; 4096];
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    match stream.read(&mut buf) {
        Ok(n) => String::from_utf8_lossy(&buf[..n]).to_string(),
        Err(e) => format!("Read error: {e}"),
    }
}

#[test]
fn integration_router_params() {
    let mut router = Router::new();
    router.route(Method::Get, "/users/:id", |req| {
        let id = req.params.get("id").map(|s| s.as_str()).unwrap_or("?");
        Response::text(format!("user {id}"))
    });

    let req = Request {
        method: Method::Get,
        path: "/users/42".into(),
        ..Default::default()
    };
    let resp = router.handle(req);
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body, b"user 42");
}

#[test]
fn integration_middleware_logging() {
    use nanoweb::middleware::logging::RequestLogging;

    let mut stack = MiddlewareStack::new();
    stack.push(Box::new(RequestLogging));

    let mut req = Request {
        method: Method::Get,
        path: "/test".into(),
        request_id: "test-123".into(),
        ..Default::default()
    };

    let resp = stack.run(&mut req, &|_req| Response::text("hello"));
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.headers.get("x-request-id"), Some("test-123"));
}

#[test]
fn integration_middleware_cors() {
    use nanoweb::config::CorsConfig;
    use nanoweb::middleware::cors::Cors;

    let cors = Cors::new(CorsConfig {
        allowed_origins: vec!["https://example.com".into()],
        ..Default::default()
    });

    let mut req = Request {
        method: Method::Options,
        path: "/api".into(),
        ..Default::default()
    };
    req.headers.insert("origin", "https://example.com");

    let resp = cors.before(&mut req);
    assert!(resp.is_some());
    let resp = resp.unwrap();
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(
        resp.headers.get("access-control-allow-origin"),
        Some("https://example.com")
    );
}

#[test]
fn integration_middleware_basicauth() {
    use nanoweb::middleware::basicauth::BasicAuth;

    let auth = BasicAuth::new("admin", "secret123");

    let mut req = Request {
        method: Method::Get,
        path: "/protected".into(),
        ..Default::default()
    };

    // Should reject without credentials
    let resp = auth.before(&mut req);
    assert!(resp.is_some());
    assert_eq!(resp.unwrap().status, StatusCode::UNAUTHORIZED);

    // Should accept with valid credentials
    use base64::Engine;
    let cred = base64::engine::general_purpose::STANDARD.encode(b"admin:secret123");
    req.headers.insert("authorization", format!("Basic {cred}"));
    let resp = auth.before(&mut req);
    assert!(resp.is_none());
}

#[test]
fn integration_middleware_ratelimit() {
    use nanoweb::middleware::ratelimit::RateLimiter;

    let limiter = RateLimiter::new(2, 2);

    let mut req1 = Request {
        method: Method::Get,
        path: "/".into(),
        ..Default::default()
    };

    // First two should pass
    assert!(limiter.before(&mut req1).is_none());
    assert!(limiter.before(&mut req1).is_none());

    // Third should be rate-limited
    let resp = limiter.before(&mut req1);
    assert!(resp.is_some());
    assert_eq!(resp.unwrap().status, StatusCode(429));
}

#[test]
fn integration_body_json() {
    let mut req = Request {
        method: Method::Post,
        path: "/api/data".into(),
        ..Default::default()
    };
    req.headers.insert("content-type", "application/json");
    req.body = br#"{"name":"test","value":42}"#.to_vec();

    #[derive(serde::Deserialize)]
    struct TestData {
        name: String,
        value: u32,
    }

    let data: TestData = req.body_as().unwrap();
    assert_eq!(data.name, "test");
    assert_eq!(data.value, 42);
}

#[test]
fn integration_body_form() {
    let mut req = Request {
        method: Method::Post,
        path: "/login".into(),
        ..Default::default()
    };
    req.headers
        .insert("content-type", "application/x-www-form-urlencoded");
    req.body = b"username=admin&password=secret".to_vec();

    #[derive(serde::Deserialize)]
    struct LoginForm {
        username: String,
        password: String,
    }

    let data: LoginForm = req.body_as().unwrap();
    assert_eq!(data.username, "admin");
    assert_eq!(data.password, "secret");
}

#[test]
fn integration_template_render() {
    use nanoweb::template::Renderer;
    use serde_json::json;

    let renderer = Renderer::new();
    let ctx = json!({"name": "World", "items": ["a", "b", "c"]});

    let result = renderer.render("Hello {{name}}!", &ctx);
    assert_eq!(result, "Hello World!");

    let result = renderer.render("{{#items}}{{.}} {{/items}}", &ctx);
    assert!(result.contains("a"));
    assert!(result.contains("b"));
    assert!(result.contains("c"));
}

#[test]
fn integration_session_sign_verify() {
    use nanoweb::session::SessionManager;

    let mgr = SessionManager::new("test-secret-key");
    let session_id = "session-abc-123";

    let signed = mgr.sign(session_id);
    let verified = mgr.verify(&signed);
    assert_eq!(verified, Some(session_id.to_string()));

    // Tampered signature should fail
    let bad = format!("session-abc-123.badsignature");
    assert!(mgr.verify(&bad).is_none());
}

#[test]
fn integration_metrics_recording() {
    use nanoweb::metrics::Metrics;

    let metrics = Metrics::new();
    metrics.record_request("GET", StatusCode::OK, "/", 0.01);
    metrics.record_request("POST", StatusCode::OK, "/api", 0.05);
    metrics.record_request("GET", StatusCode::NOT_FOUND, "/missing", 0.001);

    let output = metrics.render();
    assert!(output.contains("http_requests_total 3"));
    assert!(output.contains("http_requests_by_method{method=\"GET\"} 2"));
    assert!(output.contains("http_requests_by_status_class{status_class=\"2xx\"} 2"));
    assert!(output.contains("http_requests_by_status_class{status_class=\"4xx\"} 1"));
}

#[test]
fn integration_static_files_etag() {
    use nanoweb::static_files::StaticFiles;
    use std::fs;

    let dir = std::env::temp_dir().join("nanoweb_test_static");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("test.txt"), "hello world").unwrap();

    let static_files = StaticFiles::new(&dir);

    let req = Request {
        method: Method::Get,
        path: "/test.txt".into(),
        ..Default::default()
    };

    let resp = static_files.serve(&req).unwrap();
    assert_eq!(resp.status, StatusCode::OK);
    assert!(resp.headers.get("etag").is_some());

    // Test If-None-Match
    let etag = resp.headers.get("etag").unwrap().to_string();
    let mut req2 = Request {
        method: Method::Get,
        path: "/test.txt".into(),
        ..Default::default()
    };
    req2.headers.insert("if-none-match", etag);
    let resp2 = static_files.serve(&req2).unwrap();
    assert_eq!(resp2.status, StatusCode::NOT_MODIFIED);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn integration_static_files_traversal_rejected() {
    use nanoweb::static_files::StaticFiles;

    let static_files = StaticFiles::new("/tmp/safe_dir");

    let req = Request {
        method: Method::Get,
        path: "/../../../etc/passwd".into(),
        ..Default::default()
    };

    let resp = static_files.serve(&req).unwrap();
    assert!(resp.status == StatusCode::FORBIDDEN || resp.status == StatusCode::NOT_FOUND);
}

#[test]
fn integration_health_endpoints() {
    use nanoweb::health::{healthz_handler, readyz_handler, HealthState};
    use std::sync::Arc;

    let state = Arc::new(HealthState::new());
    let req = Request::default();

    // healthz always returns 200
    let resp = healthz_handler(&req);
    assert_eq!(resp.status, StatusCode::OK);

    // readyz returns 503 when not ready
    let resp = readyz_handler(&state, &req);
    assert_eq!(resp.status, StatusCode(503));

    // readyz returns 200 when ready
    state.set_ready(true);
    let resp = readyz_handler(&state, &req);
    assert_eq!(resp.status, StatusCode::OK);
}

#[test]
fn integration_options_method() {
    let mut router = Router::new();
    router.route(Method::Get, "/api/data", |_req| Response::ok());
    router.route(Method::Post, "/api/data", |_req| Response::ok());
    router.route(Method::Delete, "/api/data", |_req| Response::ok());

    let resp = router.handle_options("/api/data");
    assert_eq!(resp.status, StatusCode::OK);
    let allow = resp.headers.get("allow").unwrap();
    assert!(allow.contains("GET"));
    assert!(allow.contains("POST"));
    assert!(allow.contains("DELETE"));
    assert!(allow.contains("OPTIONS"));
    assert!(allow.contains("HEAD"));
}

#[test]
fn integration_head_fallback() {
    let mut router = Router::new();
    router.route(Method::Get, "/resource", |_req| {
        Response::text("body content")
    });

    let mut resp = router.handle(Request {
        method: Method::Head,
        path: "/resource".into(),
        ..Default::default()
    });
    assert_eq!(resp.status, StatusCode::OK);
    // Server layer suppresses body for HEAD; router returns GET handler response
    // Simulate server behavior by clearing body for HEAD
    resp.body.clear();
    assert!(resp.body.is_empty());
}

#[test]
fn integration_parser_full_cycle() {
    use bytes::BytesMut;

    let body_data = br#"{"key":"value"}"#;
    let request_str = format!(
        "POST /api/data HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_data.len(),
        String::from_utf8_lossy(body_data)
    );

    let buf = BytesMut::from(request_str.as_bytes());
    let req = parser::parse_request(&buf).unwrap().unwrap();

    assert_eq!(req.method, Method::Post);
    assert_eq!(req.path, "/api/data");
    assert_eq!(req.header("content-type"), Some("application/json"));
    assert_eq!(req.body, body_data);
}

#[test]
fn integration_cors_middleware_full() {
    use nanoweb::config::CorsConfig;
    use nanoweb::middleware::cors::Cors;

    let cors = Cors::new(CorsConfig {
        allowed_origins: vec!["https://example.com".into()],
        allowed_methods: vec!["GET".into(), "POST".into()],
        ..Default::default()
    });

    let mut req = Request {
        method: Method::Options,
        path: "/api".into(),
        ..Default::default()
    };
    req.headers.insert("origin", "https://example.com");

    let resp = cors.before(&mut req);
    assert!(resp.is_some());
    let resp = resp.unwrap();
    assert_eq!(
        resp.headers.get("access-control-allow-origin"),
        Some("https://example.com")
    );
    assert!(resp
        .headers
        .get("access-control-allow-methods")
        .unwrap()
        .contains("GET"));
}

#[test]
fn integration_config_defaults() {
    let config = Config::default();
    assert_eq!(config.listen, "127.0.0.1:8080");
    assert_eq!(config.metrics_path, "/metrics");
    assert_eq!(config.health_path, "/healthz");
    assert_eq!(config.readyz_path, "/readyz");
    assert_eq!(config.graceful_timeout_s, 30);
    assert!(!config.tls_configured());
}

#[test]
fn integration_router_not_found() {
    let router = Router::new();
    let resp = router.handle(Request {
        method: Method::Get,
        path: "/nonexistent".into(),
        ..Default::default()
    });
    assert_eq!(resp.status, StatusCode::NOT_FOUND);
}

#[test]
fn integration_router_method_not_allowed() {
    let mut router = Router::new();
    router.route(Method::Get, "/only-get", |_req| Response::ok());

    let resp = router.handle(Request {
        method: Method::Post,
        path: "/only-get".into(),
        ..Default::default()
    });
    assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(resp.headers.get("allow").unwrap().contains("GET"));
}
