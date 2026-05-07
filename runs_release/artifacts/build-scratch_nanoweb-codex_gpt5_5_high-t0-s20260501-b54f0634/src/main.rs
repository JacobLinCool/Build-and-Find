use nanoweb::{cli::Cli, config::Config, server};

#[tokio::main]
async fn main() {
    let cli = Cli::parse_args();
    let result = match Config::load(cli) {
        Ok(config) => server::run(config).await,
        Err(err) => Err(err),
    };
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
