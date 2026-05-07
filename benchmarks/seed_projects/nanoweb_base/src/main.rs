//! Binary entry point.

use std::process::ExitCode;

use nanoweb::cli::{self, HELP_TEXT, VERSION_TEXT};
use nanoweb::config;
use nanoweb::error::NanowebError;
use nanoweb::http::types::{Method, Response};
use nanoweb::middleware::logger::LoggerMiddleware;
use nanoweb::middleware::Chain;
use nanoweb::router::{handler, Router};
use nanoweb::static_files::{StaticFiles, StaticFilesConfig};
use nanoweb::{assemble, tls};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli_overrides = match cli::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{}", e);
            return ExitCode::from(2);
        }
    };

    if cli_overrides.show_help {
        println!("{}", HELP_TEXT);
        return ExitCode::from(0);
    }
    if cli_overrides.show_version {
        println!("{}", VERSION_TEXT);
        return ExitCode::from(0);
    }

    let cfg = match config::resolve(&cli_overrides) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {}", e);
            return ExitCode::from(2);
        }
    };

    if let Err(e) = tls::require_if_requested(
        cfg.server.tls_cert.as_ref().and_then(|p| p.to_str()),
        cfg.server.tls_key.as_ref().and_then(|p| p.to_str()),
    ) {
        eprintln!("{}", e);
        return ExitCode::from(2);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("runtime error: {}", e);
            return ExitCode::from(1);
        }
    };

    let status = runtime.block_on(async move { run(cfg).await });
    match status {
        Ok(()) => ExitCode::from(0),
        Err(e) => {
            eprintln!("fatal: {}", e);
            ExitCode::from(1)
        }
    }
}

async fn run(cfg: nanoweb::NanowebConfig) -> Result<(), NanowebError> {
    let mut router = Router::new();
    register_default_routes(&mut router, &cfg)?;

    // Build a minimal middleware chain from the resolved config.
    let mut chain = Chain::new();
    chain.push(LoggerMiddleware::new(cfg.log_level()));

    let server = assemble(cfg, router, chain);
    server.run().await
}

fn register_default_routes(
    router: &mut Router,
    cfg: &nanoweb::NanowebConfig,
) -> Result<(), NanowebError> {
    router.add(
        Method::Get,
        "/",
        handler(|_req| async { Ok(Response::html("<h1>nanoweb</h1>")) }),
    )?;

    router.add(
        Method::Get,
        "/hello/:name",
        handler(|req| async move {
            let name = req
                .param("name")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "world".to_string());
            Ok(Response::text(format!("hello, {}!", name)))
        }),
    )?;

    if let Some(dir) = cfg.server.static_dir.clone() {
        let sf = StaticFiles::new(StaticFilesConfig::new(dir));
        let sf_arc = std::sync::Arc::new(sf);
        let sf_for_get = sf_arc.clone();
        router.add(
            Method::Get,
            "/static/*path",
            handler(move |mut req| {
                let sf = sf_for_get.clone();
                async move {
                    if let Some(rest) = req.param("path").map(|s| s.to_string()) {
                        req.path = format!("/{}", rest);
                    }
                    sf.serve(&req)
                }
            }),
        )?;
    }

    Ok(())
}
