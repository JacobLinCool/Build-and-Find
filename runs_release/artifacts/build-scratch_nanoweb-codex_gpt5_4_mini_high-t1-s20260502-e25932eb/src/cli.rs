use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "nanoweb", version, about = "nanoweb HTTP/1.1 server")]
pub struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub listen: String,

    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: String,

    #[arg(long)]
    pub tls_cert: Option<String>,

    #[arg(long)]
    pub tls_key: Option<String>,

    #[arg(long, default_value = "/metrics")]
    pub metrics: String,

    #[arg(long)]
    pub static_dir: Option<String>,

    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let cli = Cli::parse_from(["nanoweb"]);
        assert_eq!(cli.listen, "127.0.0.1:8080");
    }
}
