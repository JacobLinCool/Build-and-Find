use clap::Parser;
use nanoweb::cli::Args;
use nanoweb::http::{Method, Response};
use nanoweb::middleware::logging::RequestLogger;
use nanoweb::server::App;
use nanoweb::NanowebError;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), NanowebError> {
    let args = Args::parse();
    let cfg = nanoweb::config::load(&args)?;

    let env_filter = tracing_subscriber::EnvFilter::try_new(&cfg.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let mut app = App::new(cfg);
    app.route(
        Method::Get,
        "/",
        Arc::new(|_req| async { Ok::<_, NanowebError>(Response::text("nanoweb is running")) }),
    );
    app.route(
        Method::Get,
        "/hello/:name",
        Arc::new(|req: nanoweb::http::Request| async move {
            let name = req.param("name").unwrap_or("world");
            Ok::<_, NanowebError>(Response::text(format!("hello {}", name)))
        }),
    );

    let app = app.with_middleware(Arc::new(RequestLogger));
    app.run().await?;
    Ok(())
}
