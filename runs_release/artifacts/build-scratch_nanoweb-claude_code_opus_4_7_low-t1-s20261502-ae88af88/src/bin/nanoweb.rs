use clap::Parser;
use nanoweb::cli::CliArgs;
use nanoweb::config::Config;
use nanoweb::error::{NanowebError, Result};
use nanoweb::http::{Method, Response};
use nanoweb::middleware::{Cors, Gzip, Logging, RateLimit};
use nanoweb::router::handler;
use nanoweb::server::{App, Server};
use nanoweb::session::{InMemoryStore, SessionManager};
use nanoweb::static_files::StaticFiles;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = CliArgs::parse();
    let config_path = cli.config.clone();
    let cfg = Config::load(&config_path)?.apply_cli(&cli);
    init_logging(&cfg.log_level);

    if cfg.tls_cert.is_some() ^ cfg.tls_key.is_some() {
        return Err("--tls-cert and --tls-key must be provided together".into());
    }

    #[cfg(not(feature = "tls"))]
    if cfg.tls_cert.is_some() || cfg.tls_key.is_some() {
        return Err("TLS support is disabled (build without `--features tls`).".into());
    }

    let mut app = App::new();
    app.metrics_path = cfg.metrics.clone();
    app.graceful_timeout = Duration::from_secs(cfg.graceful_timeout_s);

    // Static files
    if let Some(dir) = &cfg.static_dir {
        app.static_files = Some(Arc::new(StaticFiles::new(dir.clone())));
    }

    // Sessions
    let store = Arc::new(InMemoryStore::new());
    app.session = Some(Arc::new(SessionManager::new(
        store,
        cfg.session_secret.as_bytes().to_vec(),
    )));

    // Built-in middleware: logging + gzip + rate limit + permissive CORS
    app.middleware.push(Arc::new(Logging));
    app.middleware.push(Arc::new(RateLimit::new(1000, 1000.0)));
    app.middleware.push(Arc::new(Gzip::new()));
    app.middleware.push(Arc::new(Cors::new()));

    // Routes from config
    let renderer = app.renderer.clone();
    for r in &cfg.routes {
        let kind = r.kind.clone();
        let body = r.body.clone().unwrap_or_default();
        let template = r.template.clone();
        let renderer_c = renderer.clone();
        let h = handler(move |_req| {
            let kind = kind.clone();
            let body = body.clone();
            let template = template.clone();
            let renderer_c = renderer_c.clone();
            async move {
                match kind.as_str() {
                    "json" => match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => Response::json(&v).unwrap_or_else(|_| Response::text(body)),
                        Err(_) => Response::text(body),
                    },
                    "html" => Response::html(body),
                    "template" => {
                        let tpl = template.unwrap_or(body);
                        let html = renderer_c.render(&tpl, &serde_json::json!({}));
                        Response::html(html)
                    }
                    _ => Response::text(body),
                }
            }
        });
        let method = Method::parse(&r.method.to_uppercase())
            .ok_or_else(|| NanowebError::Config(format!("bad method {}", r.method)))?;
        app.router.route(method, &r.path, h);
    }

    // Default index handler if no routes were configured.
    if cfg.routes.is_empty() {
        let renderer_c = renderer.clone();
        app.router.get(
            "/",
            handler(move |_req| {
                let r = renderer_c.clone();
                async move {
                    let html = r.render(
                        "<html><body><h1>{{title}}</h1></body></html>",
                        &serde_json::json!({"title": "nanoweb"}),
                    );
                    Response::html(html)
                }
            }),
        );
    }

    let addr: std::net::SocketAddr = cfg
        .listen
        .parse()
        .map_err(|e| format!("bad listen addr: {}", e))?;

    #[cfg(feature = "tls")]
    if let (Some(cert), Some(key)) = (&cfg.tls_cert, &cfg.tls_key) {
        let acc = nanoweb::tls::load_acceptor(cert, key)?;
        let tls_addr: std::net::SocketAddr = cfg
            .tls_listen
            .clone()
            .unwrap_or_else(|| "127.0.0.1:8443".to_string())
            .parse()?;
        app.tls = Some((tls_addr, acc));
    }

    let server = Server::new(app);
    server.run(addr).await?;
    Ok(())
}

fn init_logging(level: &str) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .try_init();
}

fn _unused() -> Result<()> {
    Err(NanowebError::Internal("never".into()))
}
