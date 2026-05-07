use clap::Parser;
use nanoweb::{cli::Cli, config::Config, error::Result, server};

#[tokio::main]
async fn main() {
    if let Err(err) = real_main().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(cli.config.clone(), &cli)?;
    server::run(config).await
}
