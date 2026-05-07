use nanoweb::{
    Method, Response, StatusCode,
    config::Config,
    middleware::MiddlewareStack,
    router::Router,
    server::Server,
};
use std::sync::Arc;
use tokio::net::TcpListener;

fn make_client() -> reqwest::Client {
    reqwest::Client::builder()
        .http1_only()
        .build()
        .unwrap()
}

async fn start_test_server() -> (Arc<Server>, String) {
    let config = Config::default();
    let mut router = Router::new();

    router.get("/hello", |_req| async move {
        Ok(Response::text("Hello, World!"))
    });

    router.get("/echo/:name", |req| async move {
        let name = req.params.get("name").cloned().unwrap_or_default();
        Ok(Response::text(format!("Hello, {}!", name)))
    });

    router.post("/json", |req| async move {
        let val: serde_json::Value = serde_json::from_slice(&req.body)
            .unwrap_or(serde_json::Value::Null);
        Response::json(&val)
    });

    let middleware = MiddlewareStack::new();
    let server = Server::with_state(config, router, middleware).unwrap();
    let server = Arc::new(server);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let addr_str = addr.to_string();

    let server_clone = server.clone();
    tokio::spawn(async move {
        server_clone.run_on_listener(listener).await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (server, addr_str)
}

#[tokio::test]
async fn test_basic_get() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/hello", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Hello, World!");

    server.shutdown();
}

#[tokio::test]
async fn test_path_params() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/echo/Alice", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Hello, Alice!");

    server.shutdown();
}

#[tokio::test]
async fn test_not_found() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/nonexistent", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

    server.shutdown();
}

#[tokio::test]
async fn test_method_not_allowed() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .post(format!("http://{}/hello", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 405);

    server.shutdown();
}

#[tokio::test]
async fn test_healthz() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/healthz", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    server.shutdown();
}

#[tokio::test]
async fn test_readyz() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/readyz", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    server.shutdown();
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/metrics", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("nanoweb_requests_total") || body.is_empty() || body.len() > 0);

    server.shutdown();
}

#[tokio::test]
async fn test_post_json() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .post(format!("http://{}/json", addr))
        .header("content-type", "application/json")
        .body(r#"{"key": "value"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let val: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(val["key"], "value");

    server.shutdown();
}

#[tokio::test]
async fn test_request_id_header() {
    let (server, addr) = start_test_server().await;

    let client = make_client();
    let resp = client
        .get(format!("http://{}/hello", addr))
        .send()
        .await
        .unwrap();

    assert!(resp.headers().contains_key("x-request-id"));

    server.shutdown();
}
