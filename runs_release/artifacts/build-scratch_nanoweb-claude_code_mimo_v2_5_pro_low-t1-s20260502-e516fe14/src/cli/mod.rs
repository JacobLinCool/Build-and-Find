use clap::Parser;

/// nanoweb - A self-contained HTTP/1.1 web framework and server
#[derive(Parser, Debug)]
#[command(name = "nanoweb", version, about, long_about = None, disable_version_flag = true)]
pub struct Cli {
    /// Address and port to listen on
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub listen: String,

    /// Path to TOML configuration file
    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: String,

    /// TLS certificate file (PEM)
    #[arg(long)]
    pub tls_cert: Option<String>,

    /// TLS private key file (PEM)
    #[arg(long)]
    pub tls_key: Option<String>,

    /// Metrics endpoint path
    #[arg(long, default_value = "/metrics")]
    pub metrics: String,

    /// Static files directory
    #[arg(long)]
    pub static_dir: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Print version and exit
    #[arg(long)]
    pub version: bool,
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
    fn test_default_args() {
        let cli = Cli::try_parse_from(["nanoweb"]).unwrap();
        assert_eq!(cli.listen, "127.0.0.1:8080");
        assert_eq!(cli.config, "./nanoweb.toml");
        assert_eq!(cli.metrics, "/metrics");
        assert_eq!(cli.log_level, "info");
    }

    #[test]
    fn test_custom_args() {
        let cli = Cli::try_parse_from([
            "nanoweb",
            "--listen",
            "0.0.0.0:3000",
            "--config",
            "/etc/nanoweb.toml",
            "--log-level",
            "debug",
            "--static-dir",
            "/var/www",
        ])
        .unwrap();
        assert_eq!(cli.listen, "0.0.0.0:3000");
        assert_eq!(cli.config, "/etc/nanoweb.toml");
        assert_eq!(cli.log_level, "debug");
        assert_eq!(cli.static_dir, Some("/var/www".into()));
    }
}
