use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone, Default)]
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
    use clap::CommandFactory;

    #[test]
    fn help_is_valid() {
        Cli::command().debug_assert();
    }
}
