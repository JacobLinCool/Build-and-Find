use clap::Parser;
use nanoweb::cli::Cli;
use nanoweb::config::Config;
use nanoweb::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let config = Config::with_cli_args(&cli.args)?;

    let server = Server::new(config);

    if let Err(e) = server.run().await {
        eprintln!("[nanoweb] fatal error: {e}");
        std::process::exit(1);
    }

    Ok(())
}
