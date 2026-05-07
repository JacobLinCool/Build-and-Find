use nanoweb::{
    config::Config,
    http::{Method, Response},
    middleware::{
        auth::BasicAuth, cors::Cors, csrf::Csrf, gzip::ResponseGzip, logging::RequestLogging,
        rate_limit::RateLimit,
    },
    server::{run, App},
};
use std::{net::TcpListener, sync::Arc, time::Duration};

async fn spawn_app() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut app = App::new(Config {
        listen: Some(addr.to_string()),
        metrics: Some("/metrics".into()),
        static_dir: None,
        tls_cert: None,
        tls_key: None,
        log_level: None,
        graceful_timeout_s: Some(2),
        session_secret: None,
    });
    app.router
        .add(Method::Get, "/hello", |_| Response::text(200, "world"));
    app.router
        .add(Method::Post, "/echo", |req| Response::new(200, req.body));
    app.middleware.push(RequestLogging);
    app.middleware.push(ResponseGzip);
    app.middleware.push(RateLimit::new(100.0, 100.0));
    app.middleware
        .push(Cors::new(vec!["http://localhost".into()]));
    app.middleware.push(Csrf);
    app.middleware.push(BasicAuth::new("u", "p"));
    let handle = tokio::spawn(async move {
        let _ = run(Arc::new(app)).await;
    });
    (format!("http://{}", addr), handle)
}

#[tokio::test]
async fn end_to_end() {
    let (base, handle) = spawn_app().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let client = reqwest::Client::new();
    let health = client
        .get(format!("{}/healthz", base))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);
    let hello = client
        .get(format!("{}/hello", base))
        .header("authorization", "Basic dTpw")
        .send()
        .await
        .unwrap();
    assert_eq!(hello.status(), 200);
    let metrics = client
        .get(format!("{}/metrics", base))
        .send()
        .await
        .unwrap();
    assert_eq!(metrics.status(), 200);
    handle.abort();
}
