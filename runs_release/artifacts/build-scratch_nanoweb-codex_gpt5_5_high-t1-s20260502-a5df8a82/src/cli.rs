use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "nanoweb",
    version,
    about = "Small HTTP/1.1 web framework and server"
)]
pub struct Cli {
    #[arg(long)]
    pub listen: Option<String>,
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,
    #[arg(long)]
    pub tls_key: Option<PathBuf>,
    #[arg(long)]
    pub metrics: Option<String>,
    #[arg(long)]
    pub static_dir: Option<PathBuf>,
    #[arg(long)]
    pub log_level: Option<String>,
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
    fn parses_listen_flag() {
        let cli = Cli::try_parse_from(["nanoweb", "--listen", "127.0.0.1:0"]).unwrap();
        assert_eq!(cli.listen.as_deref(), Some("127.0.0.1:0"));
    }
}
