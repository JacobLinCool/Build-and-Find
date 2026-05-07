use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use nanoweb::config::{Config, MiddlewareConfig, RouteConfig};
use nanoweb::health::HealthState;
use nanoweb::http::{Method, Request, Response};
use nanoweb::metrics::MetricsRegistry;
use nanoweb::middleware::MiddlewareChain;
use nanoweb::router::Router;
use nanoweb::server::AppState;
use nanoweb::session::{InMemorySessionStore, SessionManager};
use nanoweb::template::Renderer;

fn make_config(listen: &str) -> Config {
    Config {
        listen: listen.to_string(),
        tls_cert: None,
        tls_key: None,
        metrics: "/metrics".into(),
        static_dir: None,
        log_level: "warn".into(),
        graceful_timeout_s: 5,
        session_secret: "test-secret".into(),
        routes: vec![
            RouteConfig {
                path: "/hello".into(),
                method: "GET".into(),
                handler: "hello".into(),
                file: None,
                body: Some("Hello, World!".into()),
                content_type: Some("text/plain; charset=utf-8".into()),
                status: Some(200),
            },
            RouteConfig {
                path: "/echo".into(),
                method: "POST".into(),
                handler: "echo".into(),
                file: None,
                body: None,
                content_type: None,
                status: Some(200),
            },
        ],
        middleware: MiddlewareConfig::default(),
    }
}

fn build_test_state_with_middleware(config: Config, middleware: MiddlewareChain) -> Arc<AppState> {
    let metrics = Arc::new(MetricsRegistry::new());
    let health = HealthState::new();
    let renderer = Renderer::new();
    let session_manager =
        SessionManager::new(Box::new(InMemorySessionStore::new()), b"test-secret");

    let mut router = Router::new();
    for route in &config.routes {
        let method = match route.method.to_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            _ => continue,
        };
        let body = route.body.clone();
        let status = route.status.unwrap_or(200);
        let content_type = route.content_type.clone();
        router.add(
            method,
            &route.path,
            Box::new(move |_req: Request| {
                let body = body.clone();
                let content_type = content_type.clone();
                Box::pin(async move {
                    if let Some(body) = body {
                        let ct = content_type.unwrap_or_else(|| "text/plain".into());
                        let mut resp = Response::new(status);
                        resp.set_header("content-type", ct);
                        resp.set_header("content-length", body.len().to_string());
                        resp.body = body.into_bytes();
                        resp
                    } else {
                        Response::text(200, "echo")
                    }
                })
            }),
        );
    }

    Arc::new(AppState {
        router,
        middleware,
        metrics,
        health,
        static_handler: None,
        session_manager,
        renderer,
        config,
    })
}

fn build_test_state(config: Config) -> Arc<AppState> {
    build_test_state_with_middleware(config, Vec::new())
}

async fn send_raw_request(addr: &str, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).to_string()
}

async fn start_test_server(config: Config) -> String {
    let state = build_test_state(config.clone());
    let addr = config.listen.clone();
    let listener = TcpListener::bind(&addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _addr)) = listener.accept().await {
                let state = state.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = match stream.read(&mut buf).await {
                        Ok(n) if n > 0 => n,
                        _ => return,
                    };
                    buf.truncate(n);

                    let mut req = match nanoweb::http::parser::parse_request(
                        &buf,
                        "127.0.0.1:0".parse().unwrap(),
                        "test".into(),
                    ) {
                        Ok(r) => r,
                        Err(e) => {
                            let resp = Response::text(e.status, &e.message);
                            let bytes = format!(
                                "HTTP/1.1 {} {}\r\ncontent-length: {}\r\n\r\n",
                                resp.status,
                                nanoweb::error::HttpError::status_text(resp.status),
                                resp.body.len()
                            );
                            let _ = stream.write_all(bytes.as_bytes()).await;
                            let _ = stream.write_all(&resp.body).await;
                            return;
                        }
                    };

                    req.headers
                        .insert("x-request-id".into(), req.request_id.clone());
                    let mut resp = nanoweb::server::handle_request(&req, &state).await;
                    resp.set_header("x-request-id", &req.request_id);
                    let status_text = nanoweb::error::HttpError::status_text(resp.status);
                    let mut output = format!("HTTP/1.1 {} {}\r\n", resp.status, status_text);
                    for (name, value) in &resp.headers {
                        output.push_str(&format!("{}: {}\r\n", name, value));
                    }
                    output.push_str(&format!("content-length: {}\r\n", resp.body.len()));
                    output.push_str("\r\n");
                    let _ = stream.write_all(output.as_bytes()).await;
                    let _ = stream.write_all(&resp.body).await;
                });
            }
        }
    });

    actual_addr.to_string()
}

// ============ Basic Route Tests ============

#[tokio::test]
async fn test_get_route() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(&addr, "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("Hello, World!"));
}

#[tokio::test]
async fn test_404() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(
        &addr,
        "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    assert!(resp.contains("404"));
}

#[tokio::test]
async fn test_405_method_not_allowed() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(&addr, "DELETE /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("405"));
}

#[tokio::test]
async fn test_post_with_content_length() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(
        &addr,
        "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhello",
    )
    .await;
    assert!(resp.contains("200 OK"));
}

#[tokio::test]
async fn test_bad_request() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(&addr, "GARBAGE\r\n\r\n").await;
    assert!(resp.contains("400"));
}

// ============ Built-in Endpoint Tests ============

#[tokio::test]
async fn test_healthz() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(&addr, "GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
}

#[tokio::test]
async fn test_readyz() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(&addr, "GET /readyz HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
}

#[tokio::test]
async fn test_metrics() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    send_raw_request(&addr, "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    let resp = send_raw_request(&addr, "GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("http_request_duration_seconds") || resp.contains("http_requests_total"));
}

// ============ Request ID ============

#[tokio::test]
async fn test_request_id_header() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(&addr, "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
    assert!(resp.contains("x-request-id:"));
}

// ============ Method Handling ============

#[tokio::test]
async fn test_head_method() {
    let state = build_test_state(make_config("127.0.0.1:0"));
    let mut req = Request {
        method: Method::Head,
        path: "/hello".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-head".into(),
    };
    req.headers.insert("host".into(), "localhost".into());
    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 200);
    assert!(
        resp.body.is_empty(),
        "HEAD response body should be suppressed"
    );
}

#[tokio::test]
async fn test_options_auto_list() {
    let state = build_test_state(make_config("127.0.0.1:0"));
    let mut req = Request {
        method: Method::Options,
        path: "/hello".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-options".into(),
    };
    req.headers.insert("host".into(), "localhost".into());
    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 405);
    let allow = resp.headers.iter().find(|(k, _)| k == "allow");
    assert!(allow.is_some());
}

// ============ Middleware Tests ============

#[tokio::test]
async fn test_cors_middleware() {
    use nanoweb::middleware::cors::CorsMiddleware;

    let config = make_config("127.0.0.1:0");
    let state = build_test_state_with_middleware(
        config,
        vec![Box::new(CorsMiddleware::new(vec![
            "https://example.com".into()
        ]))],
    );

    let mut req = Request {
        method: Method::Get,
        path: "/hello".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-cors".into(),
    };
    req.headers.insert("host".into(), "localhost".into());
    req.headers
        .insert("origin".into(), "https://example.com".into());

    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 200);
    let cors_header = resp
        .headers
        .iter()
        .find(|(k, _)| k == "access-control-allow-origin");
    assert!(cors_header.is_some());
    assert_eq!(cors_header.unwrap().1, "https://example.com");
}

#[tokio::test]
async fn test_cors_preflight() {
    use nanoweb::middleware::cors::CorsMiddleware;

    let config = make_config("127.0.0.1:0");
    let state = build_test_state_with_middleware(
        config,
        vec![Box::new(CorsMiddleware::new(vec!["*".into()]))],
    );

    let mut req = Request {
        method: Method::Options,
        path: "/hello".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-cors-preflight".into(),
    };
    req.headers.insert("host".into(), "localhost".into());
    req.headers
        .insert("origin".into(), "https://example.com".into());

    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 204);
    let methods = resp
        .headers
        .iter()
        .find(|(k, _)| k == "access-control-allow-methods");
    assert!(methods.is_some());
}

#[tokio::test]
async fn test_rate_limit_middleware() {
    use nanoweb::middleware::rate_limit::RateLimitMiddleware;

    let config = make_config("127.0.0.1:0");
    let state =
        build_test_state_with_middleware(config, vec![Box::new(RateLimitMiddleware::new(2, 0.0))]);

    let make_req = || {
        let mut req = Request {
            method: Method::Get,
            path: "/hello".into(),
            query: None,
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            addr: "127.0.0.1:12345".parse().unwrap(),
            request_id: "test-rl".into(),
        };
        req.headers.insert("host".into(), "localhost".into());
        req
    };

    let resp = nanoweb::server::handle_request(&make_req(), &state).await;
    assert_eq!(resp.status, 200);
    let resp = nanoweb::server::handle_request(&make_req(), &state).await;
    assert_eq!(resp.status, 200);
    let resp = nanoweb::server::handle_request(&make_req(), &state).await;
    assert_eq!(resp.status, 429);
}

#[tokio::test]
async fn test_basic_auth_middleware() {
    use base64::Engine;
    use nanoweb::middleware::basic_auth::BasicAuthMiddleware;

    let config = make_config("127.0.0.1:0");
    let state = build_test_state_with_middleware(
        config,
        vec![Box::new(BasicAuthMiddleware::new(
            "admin".into(),
            "secret".into(),
        ))],
    );

    let mut req = Request {
        method: Method::Get,
        path: "/hello".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-auth".into(),
    };
    req.headers.insert("host".into(), "localhost".into());

    // Without auth -> 401
    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 401);

    // With correct auth -> 200
    let encoded = base64::engine::general_purpose::STANDARD.encode("admin:secret");
    req.headers
        .insert("authorization".into(), format!("Basic {encoded}"));
    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 200);
}

#[tokio::test]
async fn test_gzip_middleware() {
    use nanoweb::middleware::gzip::GzipMiddleware;

    // Create a config with a route that returns a large enough body
    let mut config = make_config("127.0.0.1:0");
    config.routes = vec![RouteConfig {
        path: "/big".into(),
        method: "GET".into(),
        handler: "big".into(),
        file: None,
        body: Some("Hello, World! ".repeat(100)),
        content_type: Some("text/plain; charset=utf-8".into()),
        status: Some(200),
    }];

    let state = build_test_state_with_middleware(config, vec![Box::new(GzipMiddleware)]);

    let mut req = Request {
        method: Method::Get,
        path: "/big".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-gzip".into(),
    };
    req.headers.insert("host".into(), "localhost".into());
    req.headers.insert("accept-encoding".into(), "gzip".into());

    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 200);
    let encoding = resp.headers.iter().find(|(k, _)| k == "content-encoding");
    assert!(
        encoding.is_some(),
        "gzip encoding should be applied to large responses"
    );
    assert_eq!(encoding.unwrap().1, "gzip");
}

#[tokio::test]
async fn test_csrf_middleware_blocks_post() {
    use nanoweb::middleware::csrf::CsrfMiddleware;

    let config = make_config("127.0.0.1:0");
    let state = build_test_state_with_middleware(config, vec![Box::new(CsrfMiddleware::new())]);

    let mut req = Request {
        method: Method::Post,
        path: "/echo".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-csrf".into(),
    };
    req.headers.insert("host".into(), "localhost".into());

    let resp = nanoweb::server::handle_request(&req, &state).await;
    assert_eq!(resp.status, 403);
}

// ============ Session Tests ============

#[tokio::test]
async fn test_session_cookie() {
    let config = make_config("127.0.0.1:0");
    let state = build_test_state(config);

    let mut session_data = HashMap::new();
    session_data.insert("user".into(), "alice".into());
    state
        .session_manager
        .set_session("test-session", session_data)
        .await;

    let session = state.session_manager.get_session("test-session").await;
    assert!(session.is_some());
    assert_eq!(
        session.unwrap().get("user").map(|s| s.as_str()),
        Some("alice")
    );
}

// ============ Template Tests ============

#[tokio::test]
async fn test_template_rendering() {
    let config = make_config("127.0.0.1:0");
    let state = build_test_state(config);

    use serde_json::json;
    let result = state
        .renderer
        .render("Hello, {{name}}!", &json!({"name": "nanoweb"}))
        .unwrap();
    assert_eq!(result, "Hello, nanoweb!");
}

// ============ Static File Tests ============

#[tokio::test]
async fn test_static_files_with_etag() {
    use nanoweb::static_files::StaticFileHandler;

    let dir = std::env::temp_dir().join("nanoweb_test_static");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.txt"), "hello static").unwrap();

    let handler = StaticFileHandler::new(&dir);
    let mut req = Request {
        method: Method::Get,
        path: "/test.txt".into(),
        query: None,
        version: "HTTP/1.1".into(),
        headers: HashMap::new(),
        body: Vec::new(),
        params: HashMap::new(),
        addr: "127.0.0.1:12345".parse().unwrap(),
        request_id: "test-static".into(),
    };

    let resp = handler.handle(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"hello static");

    let etag = resp
        .headers
        .iter()
        .find(|(k, _)| k == "etag")
        .unwrap()
        .1
        .clone();
    req.headers.insert("if-none-match".into(), etag);
    let resp = handler.handle(&req).unwrap();
    assert_eq!(resp.status, 304);

    std::fs::remove_dir_all(&dir).ok();
}

// ============ Query Params ============

#[tokio::test]
async fn test_query_params() {
    let config = make_config("127.0.0.1:0");
    let addr = start_test_server(config).await;
    let resp = send_raw_request(
        &addr,
        "GET /hello?foo=bar HTTP/1.1\r\nHost: localhost\r\n\r\n",
    )
    .await;
    assert!(resp.contains("200 OK"));
}
