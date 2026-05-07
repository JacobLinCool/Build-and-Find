use nanoweb::{cli, error::NanowebError, server::App};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    if let Err(e) = async_main().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

async fn async_main() -> Result<(), NanowebError> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let cli = cli::parse_args();
    if cli.help {
        println!("nanoweb --listen <ADDR:PORT> --config <PATH> --tls-cert <PATH> --tls-key <PATH> --metrics <PATH> --static-dir <DIR> --log-level <LEVEL> --version --help");
        return Ok(());
    }
    if cli.version {
        println!("nanoweb 0.1.0");
        return Ok(());
    }
    let cfg = cli::load_config(&cli)?;
    let app = Arc::new(App::new(cfg));
    app.run().await
}
