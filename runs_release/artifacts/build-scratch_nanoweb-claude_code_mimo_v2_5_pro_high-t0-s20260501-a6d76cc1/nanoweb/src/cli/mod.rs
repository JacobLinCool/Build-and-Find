use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "nanoweb",
    version,
    about = "A self-contained HTTP/1.1 web framework and server"
)]
pub struct Cli {
    /// Address and port to listen on
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub listen: String,

    /// Path to the TOML configuration file
    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: String,

    /// Path to TLS certificate (PEM)
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// Path to TLS private key (PEM)
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Metrics endpoint path
    #[arg(long, default_value = "/metrics")]
    pub metrics: String,

    /// Static files directory
    #[arg(long)]
    pub static_dir: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cli() {
        let cli = Cli::parse_from(["nanoweb"]);
        assert_eq!(cli.listen, "127.0.0.1:8080");
        assert_eq!(cli.config, "./nanoweb.toml");
        assert_eq!(cli.metrics, "/metrics");
        assert_eq!(cli.log_level, "info");
    }

    #[test]
    fn test_custom_cli() {
        let cli = Cli::parse_from([
            "nanoweb",
            "--listen",
            "0.0.0.0:3000",
            "--config",
            "/etc/nanoweb.toml",
            "--log-level",
            "debug",
        ]);
        assert_eq!(cli.listen, "0.0.0.0:3000");
        assert_eq!(cli.config, "/etc/nanoweb.toml");
        assert_eq!(cli.log_level, "debug");
    }
}
