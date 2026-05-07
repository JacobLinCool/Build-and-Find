use nanoweb::cli::Cli;
use nanoweb::config::Config;

#[tokio::main]
async fn main() {
    let cli = Cli::parse_args();
    match Config::load(&cli) {
        Ok(config) => {
            if let Err(err) = nanoweb::server::run(config).await {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}
