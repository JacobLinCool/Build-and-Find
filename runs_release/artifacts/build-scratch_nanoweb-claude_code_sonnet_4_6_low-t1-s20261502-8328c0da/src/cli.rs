use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "nanoweb",
    version,
    about = "A lightweight HTTP/1.1 web framework",
    long_about = None
)]
pub struct CliArgs {
    /// Address and port to listen on
    #[arg(long, value_name = "ADDR:PORT")]
    pub listen: Option<String>,

    /// Path to TOML config file
    #[arg(long, value_name = "PATH", default_value = "nanoweb.toml")]
    pub config: PathBuf,

    /// TLS certificate file (PEM)
    #[arg(long, value_name = "PATH")]
    pub tls_cert: Option<PathBuf>,

    /// TLS key file (PEM)
    #[arg(long, value_name = "PATH")]
    pub tls_key: Option<PathBuf>,

    /// Metrics endpoint path
    #[arg(long, value_name = "PATH")]
    pub metrics: Option<String>,

    /// Static files directory
    #[arg(long, value_name = "DIR")]
    pub static_dir: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,
}
