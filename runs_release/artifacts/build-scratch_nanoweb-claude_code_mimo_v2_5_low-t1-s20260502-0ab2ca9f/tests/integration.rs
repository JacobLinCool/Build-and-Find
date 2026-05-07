use std::collections::HashMap;
use std::sync::Arc;

use nanoweb::config::Config;
use nanoweb::handler::{simple_handler, HandlerFn};
use nanoweb::http::{Method, Response};
use nanoweb::middleware::cors::CorsMiddleware;
use nanoweb::middleware::gzip::GzipMiddleware;
use nanoweb::middleware::logging::LoggingMiddleware;
use nanoweb::middleware::rate_limit::RateLimitMiddleware;
use nanoweb::router::Router;
use nanoweb::server::Server;

fn setup_logging() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
}

async fn start_test_server(port: u16, middleware: Vec<Arc<dyn nanoweb::middleware::Middleware>>) {
    let mut router = Router::new();
    let mut handlers: HashMap<usize, HandlerFn> = HashMap::new();

    router.add_route("/", vec![Method::Get], vec![], "index");
    handlers.insert(
        0,
        simple_handler(|_req| {
            let mut resp = Response::new();
            resp.set_text("Hello, World!");
            resp
        }),
    );

    router.add_route("/json", vec![Method::Get], vec![], "json");
    handlers.insert(
        1,
        simple_handler(|_req| {
            let mut resp = Response::new();
            resp.set_json(r#"{"message":"ok"}"#);
            resp
        }),
    );

    router.add_route("/echo", vec![Method::Post], vec![], "echo");
    handlers.insert(
        2,
        simple_handler(|req| {
            let body = req.body.clone();
            let mut resp = Response::new();
            resp.body = body;
            resp.headers
                .insert("content-type".into(), "application/octet-stream".into());
            resp.headers
                .insert("content-length".into(), resp.body.len().to_string());
            resp
        }),
    );

    router.add_route("/users/:id", vec![Method::Get], vec![], "get_user");
    handlers.insert(
        3,
        simple_handler(|req| {
            let id = req.params.get("id").unwrap().clone();
            let mut resp = Response::new();
            resp.set_text(&format!("User: {}", id));
            resp
        }),
    );

    router.add_route("/static/*path", vec![Method::Get], vec![], "static");
    handlers.insert(
        4,
        simple_handler(|req| {
            let path = req.params.get("path").unwrap().clone();
            let mut resp = Response::new();
            resp.set_text(&format!("File: {}", path));
            resp
        }),
    );

    let mut server = Server::new(Config {
        listen: format!("127.0.0.1:{}", port),
        ..Config::default()
    });

    for mw in middleware {
        server.add_global_middleware(mw);
    }
    server.set_router(router, handlers);

    tokio::spawn(async move {
        let _ = server.run().await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_simple_get() {
    setup_logging();
    let port = 18200;
    start_test_server(port, vec![Arc::new(LoggingMiddleware::new())]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "Hello, World!");
}

#[tokio::test]
async fn test_json_route() {
    setup_logging();
    let port = 18201;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/json", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert!(resp.text().await.unwrap().contains("message"));
}

#[tokio::test]
async fn test_404() {
    setup_logging();
    let port = 18202;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/nonexistent", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn test_path_params() {
    setup_logging();
    let port = 18203;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/users/42", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "User: 42");
}

#[tokio::test]
async fn test_wildcard_routes() {
    setup_logging();
    let port = 18204;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/static/css/style.css", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "File: css/style.css");
}

#[tokio::test]
async fn test_post_echo() {
    setup_logging();
    let port = 18205;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/echo", port))
        .header("content-type", "text/plain")
        .body("hello")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "hello");
}

#[tokio::test]
async fn test_cors_middleware() {
    setup_logging();
    let port = 18206;
    start_test_server(
        port,
        vec![Arc::new(CorsMiddleware::new(vec!["*".to_string()], false))],
    )
    .await;

    let client = reqwest::Client::new();
    let resp = client
        .request(
            reqwest::Method::OPTIONS,
            format!("http://127.0.0.1:{}/", port),
        )
        .header("origin", "http://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204);
    assert!(resp.headers().get("access-control-allow-origin").is_some());
}

#[tokio::test]
async fn test_rate_limit_middleware() {
    setup_logging();
    let port = 18207;
    start_test_server(port, vec![Arc::new(RateLimitMiddleware::new(1.0))]).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", port);

    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 429);
}

#[tokio::test]
async fn test_health_endpoints() {
    setup_logging();
    let port = 18208;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("http://127.0.0.1:{}/healthz", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "OK");

    let resp = client
        .get(format!("http://127.0.0.1:{}/readyz", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "OK");
}

#[tokio::test]
async fn test_metrics_endpoint() {
    setup_logging();
    let port = 18209;
    start_test_server(port, vec![]).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/metrics", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("process_uptime_seconds"));
}

#[tokio::test]
async fn test_gzip_middleware() {
    setup_logging();
    let port = 18210;

    let mut router = Router::new();
    let mut handlers: HashMap<usize, HandlerFn> = HashMap::new();
    router.add_route("/", vec![Method::Get], vec![], "index");
    handlers.insert(
        0,
        simple_handler(|_req| {
            let mut resp = Response::new();
            let body = "x".repeat(1000);
            resp.set_text(&body);
            resp
        }),
    );

    let mut server = Server::new(Config {
        listen: format!("127.0.0.1:{}", port),
        ..Config::default()
    });
    server.add_global_middleware(Arc::new(GzipMiddleware::new()));
    server.set_router(router, handlers);

    tokio::spawn(async move {
        let _ = server.run().await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/", port))
        .header("accept-encoding", "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let encoding = resp.headers().get("content-encoding");
    assert!(encoding.is_some());
    assert_eq!(encoding.unwrap(), "gzip");
}

#[tokio::test]
async fn test_basic_auth_middleware() {
    setup_logging();
    use nanoweb::middleware::basic_auth::BasicAuthMiddleware;

    let port = 18211;
    let mut router = Router::new();
    let mut handlers: HashMap<usize, HandlerFn> = HashMap::new();
    router.add_route("/", vec![Method::Get], vec![], "index");
    handlers.insert(
        0,
        simple_handler(|_req| {
            let mut resp = Response::new();
            resp.set_text("secret");
            resp
        }),
    );

    let mut server = Server::new(Config {
        listen: format!("127.0.0.1:{}", port),
        ..Config::default()
    });
    server.add_global_middleware(Arc::new(BasicAuthMiddleware::new("admin", "password")));
    server.set_router(router, handlers);

    tokio::spawn(async move {
        let _ = server.run().await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", port);

    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);

    let resp = client
        .get(&url)
        .basic_auth("admin", Some("password"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.text().await.unwrap(), "secret");
}
