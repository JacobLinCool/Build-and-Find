use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "nanoweb", version)]
pub struct Cli {
    #[arg(long)]
    pub listen: Option<String>,
    #[arg(long)]
    pub config: Option<String>,
    #[arg(long)]
    pub tls_cert: Option<String>,
    #[arg(long)]
    pub tls_key: Option<String>,
    #[arg(long)]
    pub metrics: Option<String>,
    #[arg(long)]
    pub static_dir: Option<String>,
    #[arg(long)]
    pub log_level: Option<String>,
}
