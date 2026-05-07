use clap::Parser as ClapParser;
use nanoweb::config::{Config, ConfigOverrides};
use nanoweb::error::NanowebError;
use nanoweb::http::types::{Request, Response};
use nanoweb::server::App;
use tracing_subscriber::EnvFilter;

#[derive(clap::Parser, Debug)]
#[command(
    name = "nanoweb",
    version = env!("CARGO_PKG_VERSION"),
    about = "A self-contained HTTP/1.1 web framework and server"
)]
struct Cli {
    #[arg(long, value_name = "ADDR")]
    listen: Option<String>,

    #[arg(long, short = 'c', value_name = "FILE")]
    config: Option<String>,

    #[arg(long, value_name = "FILE")]
    tls_cert: Option<String>,

    #[arg(long, value_name = "FILE")]
    tls_key: Option<String>,

    #[arg(long, value_name = "PATH")]
    metrics: Option<String>,

    #[arg(long, value_name = "DIR")]
    static_dir: Option<String>,

    #[arg(long, value_name = "LEVEL", default_value = "info")]
    log_level: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Load config from file or defaults
    let mut config = if let Some(ref config_path) = cli.config {
        Config::from_file(std::path::Path::new(config_path))?
    } else {
        Config::default()
    };

    // Apply CLI overrides
    let overrides = ConfigOverrides {
        listen: cli.listen,
        log_level: cli.log_level,
        tls_cert: cli.tls_cert,
        tls_key: cli.tls_key,
        metrics_path: cli.metrics,
        static_dir: cli.static_dir,
    };
    config = config.apply_overrides(overrides);

    // Init tracing
    let filter = EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Build app
    let app = App::new()
        .get("/", |_req: Request| async {
            Ok::<Response, NanowebError>(Response::ok().with_html("<h1>Welcome to nanoweb!</h1>"))
        })
        .get("/healthz", |_req: Request| async {
            Ok::<Response, NanowebError>(Response::ok().with_json("{\"status\":\"ok\"}"))
        })
        .with_metrics();

    let server = app.into_server(config);
    server.run().await?;

    Ok(())
}
