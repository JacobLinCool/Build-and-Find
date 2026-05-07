use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "nanoweb",
    version,
    about = "A self-contained HTTP/1.1 web framework and server",
    long_about = None
)]
pub struct Cli {
    /// Address and port to listen on
    #[arg(long, value_name = "ADDR:PORT")]
    pub listen: Option<String>,

    /// Path to TOML configuration file
    #[arg(long, value_name = "PATH", default_value = "./nanoweb.toml")]
    pub config: String,

    /// Path to TLS certificate (PEM)
    #[arg(long, value_name = "PATH")]
    pub tls_cert: Option<String>,

    /// Path to TLS private key (PEM)
    #[arg(long, value_name = "PATH")]
    pub tls_key: Option<String>,

    /// Metrics endpoint path
    #[arg(long, value_name = "PATH")]
    pub metrics: Option<String>,

    /// Directory to serve static files from
    #[arg(long, value_name = "DIR")]
    pub static_dir: Option<String>,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,
}

impl Cli {
    pub fn parse_args() -> Self {
        Cli::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::parse_from(["nanoweb"]);
        assert_eq!(cli.config, "./nanoweb.toml");
        assert!(cli.listen.is_none());
        assert!(cli.tls_cert.is_none());
    }

    #[test]
    fn test_cli_with_listen() {
        let cli = Cli::parse_from(["nanoweb", "--listen", "0.0.0.0:9090"]);
        assert_eq!(cli.listen.as_deref(), Some("0.0.0.0:9090"));
    }

    #[test]
    fn test_cli_with_config() {
        let cli = Cli::parse_from(["nanoweb", "--config", "/etc/nanoweb.toml"]);
        assert_eq!(cli.config, "/etc/nanoweb.toml");
    }
}
