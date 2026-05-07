use clap::Parser;
use nanoweb::{
    cli::Cli,
    config::Config,
    server::{run, App},
};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| "./nanoweb.toml".into());
    let cfg = Config::load(PathBuf::from(config_path))?.finalize(cli.clone());
    let app = Arc::new(App::new(cfg));
    run(app).await?;
    Ok(())
}
