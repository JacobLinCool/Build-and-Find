//! CLI argument parsing.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone, Default)]
#[command(
    name = "nanoweb",
    about = "A self-contained HTTP/1.1 web framework and server"
)]
pub struct Args {
    /// Address to listen on, e.g. 127.0.0.1:8080.
    #[arg(long)]
    pub listen: Option<String>,

    /// Path to the TOML configuration file.
    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: PathBuf,

    /// Path to the TLS certificate (PEM).
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// Path to the TLS private key (PEM).
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Mount path for the Prometheus metrics endpoint (default /metrics).
    #[arg(long)]
    pub metrics: Option<String>,

    /// Directory whose contents should be served as static files.
    #[arg(long)]
    pub static_dir: Option<PathBuf>,

    /// Logging level (error, warn, info, debug, trace).
    #[arg(long)]
    pub log_level: Option<String>,
}
