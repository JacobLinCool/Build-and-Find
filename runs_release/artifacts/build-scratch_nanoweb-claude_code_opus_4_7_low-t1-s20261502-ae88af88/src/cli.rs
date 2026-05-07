use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Default, Clone)]
#[command(name = "nanoweb", version, about = "HTTP/1.1 framework and server")]
pub struct CliArgs {
    /// Address to listen on (e.g. 127.0.0.1:8080).
    #[arg(long)]
    pub listen: Option<String>,

    /// Path to a TOML configuration file.
    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: PathBuf,

    /// PEM TLS certificate file.
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// PEM TLS private key file.
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Path of the metrics endpoint (default /metrics).
    #[arg(long)]
    pub metrics: Option<String>,

    /// Directory to serve static files from.
    #[arg(long)]
    pub static_dir: Option<PathBuf>,

    /// Log level (e.g. info, debug, trace).
    #[arg(long)]
    pub log_level: Option<String>,
}
