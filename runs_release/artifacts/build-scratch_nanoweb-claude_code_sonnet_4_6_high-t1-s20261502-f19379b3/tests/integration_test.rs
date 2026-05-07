use nanoweb::{
    config::Config,
    middleware::{
        basic_auth::BasicAuthMiddleware,
        cors::{CorsConfig, CorsMiddleware},
        gzip::GzipMiddleware,
        logging::LoggingMiddleware,
        rate_limit::RateLimitMiddleware,
        FnHandler,
    },
    server::Server,
    Request, Response,
};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

async fn start_test_server(
    setup: impl FnOnce(&mut nanoweb::Router),
) -> (u16, broadcast::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let (shutdown_tx, _) = broadcast::channel::<()>(1);
    let shutdown_for_server = shutdown_tx.clone();

    let mut config = Config::default();
    config.listen = format!("127.0.0.1:{}", port);

    let mut server = Server::new(config).unwrap();
    {
        let router = server.router_mut();
        setup(router);
    }

    tokio::spawn(async move {
        server
            .run_on_listener(listener, shutdown_for_server)
            .await
            .unwrap();
    });

    // Brief pause for server to be ready
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    (port, shutdown_tx)
}

async fn http_get(port: u16, path: &str) -> (u16, String, Vec<(String, String)>) {
    http_request(port, "GET", path, &[], &[]).await
}

async fn http_request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, String, Vec<(String, String)>) {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();

    let mut req = format!("{} {} HTTP/1.1\r\nHost: localhost\r\n", method, path);
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("Connection: close\r\n\r\n");

    stream.write_all(req.as_bytes()).await.unwrap();
    if !body.is_empty() {
        stream.write_all(body).await.unwrap();
    }

    let mut response_bytes = Vec::new();
    stream.read_to_end(&mut response_bytes).await.unwrap();

    let response_str = String::from_utf8_lossy(&response_bytes).to_string();

    // Parse status
    let first_line = response_str.lines().next().unwrap_or("");
    let status: u16 = first_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Parse headers
    let mut resp_headers = Vec::new();
    let mut in_headers = false;
    let mut body_start = 0;
    let mut header_done = false;

    for (i, line) in response_str.lines().enumerate() {
        if i == 0 {
            in_headers = true;
            continue;
        }
        if in_headers && !header_done {
            if line.is_empty() {
                header_done = true;
                continue;
            }
            if let Some(colon) = line.find(':') {
                resp_headers.push((
                    line[..colon].trim().to_lowercase().to_string(),
                    line[colon + 1..].trim().to_string(),
                ));
            }
        }
    }

    // Extract body (everything after \r\n\r\n)
    let body_str = if let Some(pos) = response_str.find("\r\n\r\n") {
        response_str[pos + 4..].to_string()
    } else {
        String::new()
    };

    (status, body_str, resp_headers)
}

#[tokio::test]
async fn test_basic_get_request() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/hello",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("Hello, World!"))
            })),
        );
    })
    .await;

    let (status, body, _) = http_get(port, "/hello").await;
    assert_eq!(status, 200);
    assert_eq!(body, "Hello, World!");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_path_parameters() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/users/:id",
            Arc::new(FnHandler::new(|req: Request| async move {
                let id = req.params.get("id").cloned().unwrap_or_default();
                Ok(Response::text(format!("user:{}", id)))
            })),
        );
    })
    .await;

    let (status, body, _) = http_get(port, "/users/42").await;
    assert_eq!(status, 200);
    assert_eq!(body, "user:42");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_wildcard_path() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/files/*path",
            Arc::new(FnHandler::new(|req: Request| async move {
                let path = req.params.get("path").cloned().unwrap_or_default();
                Ok(Response::text(format!("file:{}", path)))
            })),
        );
    })
    .await;

    let (status, body, _) = http_get(port, "/files/docs/readme.txt").await;
    assert_eq!(status, 200);
    assert_eq!(body, "file:docs/readme.txt");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_404_not_found() {
    let (port, shutdown) = start_test_server(|_router| {}).await;

    let (status, _, _) = http_get(port, "/nonexistent").await;
    assert_eq!(status, 404);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_405_method_not_allowed() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/resource",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("ok"))
            })),
        );
    })
    .await;

    let (status, _, headers) = http_request(port, "POST", "/resource", &[], &[]).await;
    assert_eq!(status, 405);
    let allow = headers
        .iter()
        .find(|(k, _)| k == "allow")
        .map(|(_, v)| v.as_str());
    assert!(allow.is_some());

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_post_with_json_body() {
    let (port, shutdown) = start_test_server(|router| {
        router.route(
            "POST",
            "/echo",
            Arc::new(FnHandler::new(|req: Request| async move {
                let body = String::from_utf8_lossy(&req.body).to_string();
                Ok(Response::text(body))
            })),
            vec![],
        );
    })
    .await;

    let body = br#"{"key":"value"}"#;
    let (status, resp_body, _) = http_request(
        port,
        "POST",
        "/echo",
        &[("Content-Type", "application/json")],
        body,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(resp_body, r#"{"key":"value"}"#);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_head_request_no_body() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/data",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("some content"))
            })),
        );
    })
    .await;

    let (status, body, _) = http_request(port, "HEAD", "/data", &[], &[]).await;
    assert_eq!(status, 200);
    assert!(body.is_empty());

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_x_request_id_header() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/id",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("ok"))
            })),
        );
    })
    .await;

    let (_, _, headers) = http_get(port, "/id").await;
    let req_id = headers
        .iter()
        .find(|(k, _)| k == "x-request-id")
        .map(|(_, v)| v.as_str());
    assert!(req_id.is_some());
    assert!(!req_id.unwrap().is_empty());

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_logging_middleware() {
    let (port, shutdown) = start_test_server(|router| {
        router.add_global_middleware(Arc::new(LoggingMiddleware));
        router.get(
            "/logged",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("logged"))
            })),
        );
    })
    .await;

    let (status, _, _) = http_get(port, "/logged").await;
    assert_eq!(status, 200);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_gzip_middleware() {
    let (port, shutdown) = start_test_server(|router| {
        router.add_global_middleware(Arc::new(GzipMiddleware::with_min_size(10)));
        let big_body = "hello world ".repeat(100);
        router.get(
            "/big",
            Arc::new(FnHandler::new(move |_req: Request| {
                let b = big_body.clone();
                async move { Ok(Response::text(b)) }
            })),
        );
    })
    .await;

    let (status, _, headers) =
        http_request(port, "GET", "/big", &[("Accept-Encoding", "gzip")], &[]).await;
    assert_eq!(status, 200);
    let enc = headers
        .iter()
        .find(|(k, _)| k == "content-encoding")
        .map(|(_, v)| v.as_str());
    assert_eq!(enc, Some("gzip"));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_cors_middleware() {
    let (port, shutdown) = start_test_server(|router| {
        router.add_global_middleware(Arc::new(CorsMiddleware::new(CorsConfig::default())));
        router.get(
            "/api",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("api"))
            })),
        );
    })
    .await;

    let (status, _, headers) = http_request(
        port,
        "GET",
        "/api",
        &[("Origin", "https://example.com")],
        &[],
    )
    .await;
    assert_eq!(status, 200);
    let cors = headers
        .iter()
        .find(|(k, _)| k == "access-control-allow-origin")
        .map(|(_, v)| v.as_str());
    assert!(cors.is_some());

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_rate_limit_middleware() {
    let (port, shutdown) = start_test_server(|router| {
        router.add_global_middleware(Arc::new(RateLimitMiddleware::with_capacity(100.0, 1000.0)));
        router.get(
            "/limited",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("ok"))
            })),
        );
    })
    .await;

    let (status, _, _) = http_get(port, "/limited").await;
    assert_eq!(status, 200);

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_basic_auth_middleware() {
    use base64::{engine::general_purpose, Engine};

    let (port, shutdown) = start_test_server(|router| {
        router.add_global_middleware(Arc::new(BasicAuthMiddleware::single(
            "Test", "admin", "secret",
        )));
        router.get(
            "/protected",
            Arc::new(FnHandler::new(|_req: Request| async {
                Ok(Response::text("secret data"))
            })),
        );
    })
    .await;

    // Without credentials
    let (status, _, _) = http_get(port, "/protected").await;
    assert_eq!(status, 401);

    // With correct credentials
    let encoded = general_purpose::STANDARD.encode("admin:secret");
    let auth_header = format!("Basic {}", encoded);
    let (status, body, _) = http_request(
        port,
        "GET",
        "/protected",
        &[("Authorization", &auth_header)],
        &[],
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, "secret data");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_chunked_request_body() {
    let (port, shutdown) = start_test_server(|router| {
        router.route(
            "POST",
            "/chunk",
            Arc::new(FnHandler::new(|req: Request| async move {
                let body = String::from_utf8_lossy(&req.body).to_string();
                Ok(Response::text(body))
            })),
            vec![],
        );
    })
    .await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();

    let request = "POST /chunk HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response_str = String::from_utf8_lossy(&response).to_string();

    assert!(response_str.starts_with("HTTP/1.1 200"));
    assert!(response_str.ends_with("hello"));

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_query_string_access() {
    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/search",
            Arc::new(FnHandler::new(|req: Request| async move {
                let params = req.query_params();
                let q = params.get("q").cloned().unwrap_or_default();
                Ok(Response::text(format!("query:{}", q)))
            })),
        );
    })
    .await;

    let (status, body, _) = http_get(port, "/search?q=rust").await;
    assert_eq!(status, 200);
    assert_eq!(body, "query:rust");

    let _ = shutdown.send(());
}

#[tokio::test]
async fn test_json_response() {
    use serde_json::json;

    let (port, shutdown) = start_test_server(|router| {
        router.get(
            "/json",
            Arc::new(FnHandler::new(|_req: Request| async {
                Response::json(&serde_json::json!({"status": "ok", "count": 42}))
            })),
        );
    })
    .await;

    let (status, body, headers) = http_get(port, "/json").await;
    assert_eq!(status, 200);
    let ct = headers
        .iter()
        .find(|(k, _)| k == "content-type")
        .map(|(_, v)| v.as_str());
    assert!(ct.unwrap_or("").contains("application/json"));

    let _ = shutdown.send(());
}
