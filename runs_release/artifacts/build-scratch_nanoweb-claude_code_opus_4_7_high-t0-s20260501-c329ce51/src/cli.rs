use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "nanoweb",
    version,
    about = "nanoweb - HTTP/1.1 framework and server"
)]
pub struct CliArgs {
    /// Listening address for plain HTTP, e.g. 127.0.0.1:8080
    #[arg(long)]
    pub listen: Option<String>,

    /// Path to TOML configuration file (default ./nanoweb.toml)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Path to PEM TLS certificate
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// Path to PEM TLS private key
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Path at which Prometheus metrics are exposed
    #[arg(long)]
    pub metrics: Option<String>,

    /// Directory served as static files at /static/*
    #[arg(long)]
    pub static_dir: Option<PathBuf>,

    /// Log level (trace|debug|info|warn|error)
    #[arg(long)]
    pub log_level: Option<String>,
}
