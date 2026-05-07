use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
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
    pub config: PathBuf,

    /// Path to the TLS certificate file (PEM)
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// Path to the TLS private key file (PEM)
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Path for the Prometheus metrics endpoint
    #[arg(long, default_value = "/metrics")]
    pub metrics: String,

    /// Directory to serve static files from
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
    fn test_cli_defaults() {
        let cli = Cli::try_parse_from(["nanoweb"]).unwrap();
        assert_eq!(cli.listen, "127.0.0.1:8080");
        assert_eq!(cli.config, PathBuf::from("./nanoweb.toml"));
        assert_eq!(cli.metrics, "/metrics");
        assert_eq!(cli.log_level, "info");
    }

    #[test]
    fn test_cli_custom_values() {
        let cli = Cli::try_parse_from([
            "nanoweb",
            "--listen",
            "0.0.0.0:3000",
            "--config",
            "/etc/nanoweb.toml",
            "--log-level",
            "debug",
        ])
        .unwrap();
        assert_eq!(cli.listen, "0.0.0.0:3000");
        assert_eq!(cli.config, PathBuf::from("/etc/nanoweb.toml"));
        assert_eq!(cli.log_level, "debug");
    }
}
