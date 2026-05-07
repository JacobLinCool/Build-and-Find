use clap::Parser;

#[derive(Debug, Parser, Clone)]
#[command(
    name = "nanoweb",
    version,
    about = "Small HTTP/1.1 framework and server"
)]
pub struct Cli {
    #[arg(long)]
    pub listen: Option<String>,
    #[arg(long)]
    pub config: Option<String>,
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

pub fn parse() -> Cli {
    Cli::parse()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn parses_listen() {
        let c = super::Cli::parse_from(["nanoweb", "--listen", "127.0.0.1:1"]);
        assert_eq!(c.listen.as_deref(), Some("127.0.0.1:1"));
    }
}
