use crate::config::{self, Config};
use crate::error::NanowebError;
use std::env;

#[derive(Debug, Clone)]
pub struct Cli {
    pub listen: Option<String>,
    pub config: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub metrics: Option<String>,
    pub static_dir: Option<String>,
    pub log_level: Option<String>,
    pub version: bool,
    pub help: bool,
}

pub fn parse_args() -> Cli {
    let mut cli = Cli {
        listen: None,
        config: None,
        tls_cert: None,
        tls_key: None,
        metrics: None,
        static_dir: None,
        log_level: None,
        version: false,
        help: false,
    };
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--listen" => cli.listen = args.next(),
            "--config" => cli.config = args.next(),
            "--tls-cert" => cli.tls_cert = args.next(),
            "--tls-key" => cli.tls_key = args.next(),
            "--metrics" => cli.metrics = args.next(),
            "--static-dir" => cli.static_dir = args.next(),
            "--log-level" => cli.log_level = args.next(),
            "--version" => cli.version = true,
            "--help" => cli.help = true,
            _ => {}
        }
    }
    cli
}

pub fn load_config(cli: &Cli) -> Result<Config, NanowebError> {
    let mut cfg = if let Some(path) = &cli.config {
        config::load(path)?
    } else if std::path::Path::new("./nanoweb.toml").exists() {
        config::load("./nanoweb.toml")?
    } else {
        Config::default()
    };
    if let Some(v) = &cli.listen {
        cfg.listen = v.clone();
    }
    if let Some(v) = &cli.tls_cert {
        cfg.tls_cert = Some(v.clone());
    }
    if let Some(v) = &cli.tls_key {
        cfg.tls_key = Some(v.clone());
    }
    if let Some(v) = &cli.metrics {
        cfg.metrics = v.clone();
    }
    if let Some(v) = &cli.static_dir {
        cfg.static_dir = Some(v.clone());
    }
    if let Some(v) = &cli.log_level {
        cfg.log_level = v.clone();
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_defaults() {
        let _ = parse_args;
    }
}
