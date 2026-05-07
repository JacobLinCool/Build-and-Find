use crate::config::ConfigOverrides;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "nanoweb",
    version = env!("CARGO_PKG_VERSION"),
    about = "A self-contained HTTP/1.1 web framework and server",
    long_about = None
)]
pub struct Cli {
    /// Address to listen on
    #[arg(long, value_name = "ADDR")]
    pub listen: Option<String>,

    /// Path to config file
    #[arg(long, short = 'c', value_name = "FILE")]
    pub config: Option<String>,

    /// TLS certificate file
    #[arg(long, value_name = "FILE")]
    pub tls_cert: Option<String>,

    /// TLS key file
    #[arg(long, value_name = "FILE")]
    pub tls_key: Option<String>,

    /// Enable metrics endpoint (path)
    #[arg(long, value_name = "PATH")]
    pub metrics: Option<String>,

    /// Serve static files from directory
    #[arg(long, value_name = "DIR")]
    pub static_dir: Option<String>,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,
}

impl Cli {
    pub fn into_overrides(self) -> ConfigOverrides {
        ConfigOverrides {
            listen: self.listen,
            log_level: self.log_level,
            tls_cert: self.tls_cert,
            tls_key: self.tls_key,
            metrics_path: self.metrics,
            static_dir: self.static_dir,
        }
    }
}

pub fn parse() -> Cli {
    Cli::parse()
}
