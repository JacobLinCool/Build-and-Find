use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(name = "nanoweb", version, about = "nanoweb HTTP/1.1 server")]
pub struct Cli {
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub listen: String,

    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: PathBuf,

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
