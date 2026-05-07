use clap::Parser;

#[derive(Debug, Parser, Clone)]
#[command(author, version, about)]
pub struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub listen: String,
    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: String,
    #[arg(long)]
    pub tls_cert: Option<String>,
    #[arg(long)]
    pub tls_key: Option<String>,
    #[arg(long)]
    pub metrics: Option<String>,
    #[arg(long)]
    pub static_dir: Option<String>,
    #[arg(long)]
    pub log_level: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_is_available() {
        Cli::command().debug_assert();
    }
}
