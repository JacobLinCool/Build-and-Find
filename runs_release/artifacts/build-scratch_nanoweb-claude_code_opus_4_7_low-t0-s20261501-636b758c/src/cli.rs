//! CLI flags. Every flag has a matching TOML key.

use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone)]
#[command(name = "nanoweb", version)]
pub struct Args {
    #[arg(long)]
    pub listen: Option<String>,
    #[arg(long, default_value = "./nanoweb.toml")]
    pub config: String,
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
