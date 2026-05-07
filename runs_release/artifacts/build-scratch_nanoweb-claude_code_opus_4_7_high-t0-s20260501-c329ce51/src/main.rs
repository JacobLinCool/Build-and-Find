use clap::Parser;
use nanoweb::cli::CliArgs;
use nanoweb::config::Config;
#[cfg(not(feature = "tls"))]
use nanoweb::error::NanowebError;
use nanoweb::error::Result;
use nanoweb::http::{Method, Request, Response, StatusCode};
use nanoweb::middleware::{BasicAuth, Compression, Cors, Csrf, RateLimit, RequestLogger};
use nanoweb::router::Handler;
use nanoweb::server::{serve, App};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = CliArgs::parse();

    #[cfg(not(feature = "tls"))]
    if cli.tls_cert.is_some() || cli.tls_key.is_some() {
        return Err(NanowebError::Tls(
            "TLS support disabled at compile time (build with --features tls)".into(),
        ));
    }

    let cfg = Config::load(&cli)?;

    let env =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(cfg.log_level.clone()));
    tracing_subscriber::fmt().with_env_filter(env).init();

    let mut app = App::new(cfg.clone());

    // Standard middlewares (deterministic order).
    app.chain = app
        .chain
        .push(RequestLogger::new())
        .push(Compression::new())
        .push(RateLimit::new(
            cfg.rate_limit_capacity,
            cfg.rate_limit_refill_per_sec,
        ))
        .push(Cors::new(cfg.allowed_origins.clone()))
        .push(Csrf::new());
    if let Some(realm) = cfg.basic_auth_realm.clone() {
        if !cfg.basic_auth_users.is_empty() {
            app.chain = app
                .chain
                .push(BasicAuth::new(realm, cfg.basic_auth_users.clone()));
        }
    }

    register_default_routes(&mut app);

    serve(app).await
}

fn register_default_routes(app: &mut App) {
    let metrics = app.metrics.clone();
    let metrics_path = app.config.metrics_path.clone();
    let health = app.health.clone();

    app.router.get(&metrics_path, MetricsHandler { metrics });
    app.router.get(
        "/healthz",
        HealthLiveHandler {
            health: health.clone(),
        },
    );
    app.router.get(
        "/readyz",
        HealthReadyHandler {
            health: health.clone(),
        },
    );
    app.router.get("/", |_req: Request| async move {
        let body = "<html><body><h1>nanoweb</h1><p>It works.</p></body></html>";
        Response::html(body)
    });
}

struct MetricsHandler {
    metrics: nanoweb::metrics::Metrics,
}

#[async_trait::async_trait]
impl Handler for MetricsHandler {
    async fn handle(&self, _req: Request) -> Response {
        let mut r = Response::new(StatusCode::OK);
        r.headers
            .insert("Content-Type", "text/plain; version=0.0.4");
        r.body = self.metrics.render_prometheus().into_bytes();
        r
    }
}

struct HealthLiveHandler {
    health: nanoweb::health::Health,
}

#[async_trait::async_trait]
impl Handler for HealthLiveHandler {
    async fn handle(&self, _req: Request) -> Response {
        self.health.liveness_response()
    }
}

struct HealthReadyHandler {
    health: nanoweb::health::Health,
}

#[async_trait::async_trait]
impl Handler for HealthReadyHandler {
    async fn handle(&self, _req: Request) -> Response {
        self.health.readiness_response()
    }
}

#[allow(dead_code)]
fn _force_arc(a: Arc<()>) -> Arc<()> {
    a
}

#[allow(dead_code)]
fn _force_method_use(m: Method) -> Method {
    m
}
