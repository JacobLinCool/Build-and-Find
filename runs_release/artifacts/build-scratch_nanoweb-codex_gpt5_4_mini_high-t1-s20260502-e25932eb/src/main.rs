use nanoweb::cli::Cli;
use nanoweb::config::Config;
use nanoweb::server;

#[tokio::main]
async fn main() {
    let cli = Cli::parse_args();
    let config = match Config::load(cli) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(config.log_level.clone())
        .with_target(false)
        .compact()
        .init();

    if let Err(err) = server::run(config).await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
