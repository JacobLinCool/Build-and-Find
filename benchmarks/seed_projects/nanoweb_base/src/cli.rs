//! Command-line parsing for the `nanoweb` binary.
//!
//! Supported flags:
//! * `--listen HOST:PORT`        (overrides `server.listen`)
//! * `--config PATH`             (config file path)
//! * `--config-format {toml,ini}` (legacy selector — defaults to toml)
//! * `--tls-cert PATH`           (overrides `server.tls_cert`)
//! * `--tls-key PATH`            (overrides `server.tls_key`)
//! * `--metrics {on,off}`        (overrides `server.metrics`)
//! * `--static-dir PATH`         (overrides `server.static_dir`)
//! * `--log-level LEVEL`         (overrides `server.log_level`)
//! * `--version`
//! * `--help`

use crate::config::ConfigFormat;
use crate::error::NanowebError;

/// Collected CLI flags. `None` means the flag was not provided and
/// therefore must not override the corresponding config value.
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub listen: Option<String>,
    pub config_path: Option<String>,
    pub config_format: Option<ConfigFormat>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub metrics: Option<bool>,
    pub static_dir: Option<String>,
    pub log_level: Option<String>,
    pub show_version: bool,
    pub show_help: bool,
}

pub const HELP_TEXT: &str = "nanoweb — a tiny HTTP/1.1 server\n\n\
USAGE:\n    nanoweb [flags]\n\n\
FLAGS:\n\
    --listen HOST:PORT        Address to bind (default 127.0.0.1:8080)\n\
    --config PATH             Path to a config file\n\
    --config-format FORMAT    Config file format: toml (default) or ini (legacy)\n\
    --tls-cert PATH           Path to a PEM-encoded TLS certificate chain\n\
    --tls-key PATH            Path to a PEM-encoded TLS private key\n\
    --metrics {on,off}        Expose Prometheus metrics at /metrics (default on)\n\
    --static-dir PATH         Serve files from this directory under /static/\n\
    --log-level LEVEL         trace|debug|info|warn|error (default info)\n\
    --version                 Print the version string and exit\n\
    --help                    Print this message and exit\n";

pub const VERSION_TEXT: &str = concat!(env!("CARGO_PKG_NAME"), " v", env!("CARGO_PKG_VERSION"));

pub fn parse(args: &[String]) -> Result<CliOverrides, NanowebError> {
    let mut out = CliOverrides::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--help" | "-h" => {
                out.show_help = true;
                i += 1;
            }
            "--version" | "-V" => {
                out.show_version = true;
                i += 1;
            }
            "--listen" => {
                out.listen = Some(take_value(args, &mut i, "--listen")?);
            }
            "--config" => {
                out.config_path = Some(take_value(args, &mut i, "--config")?);
            }
            "--config-format" => {
                let v = take_value(args, &mut i, "--config-format")?;
                let fmt = ConfigFormat::parse(&v).ok_or_else(|| {
                    NanowebError::Config(format!(
                        "invalid --config-format '{}', expected toml or ini",
                        v
                    ))
                })?;
                out.config_format = Some(fmt);
            }
            "--tls-cert" => {
                out.tls_cert = Some(take_value(args, &mut i, "--tls-cert")?);
            }
            "--tls-key" => {
                out.tls_key = Some(take_value(args, &mut i, "--tls-key")?);
            }
            "--metrics" => {
                let v = take_value(args, &mut i, "--metrics")?;
                out.metrics = Some(match v.to_ascii_lowercase().as_str() {
                    "on" | "true" | "1" => true,
                    "off" | "false" | "0" => false,
                    other => {
                        return Err(NanowebError::Config(format!(
                            "invalid --metrics '{}'",
                            other
                        )))
                    }
                });
            }
            "--static-dir" => {
                out.static_dir = Some(take_value(args, &mut i, "--static-dir")?);
            }
            "--log-level" => {
                out.log_level = Some(take_value(args, &mut i, "--log-level")?);
            }
            other => {
                return Err(NanowebError::Config(format!("unrecognised flag: {}", other)));
            }
        }
    }
    Ok(out)
}

fn take_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, NanowebError> {
    if *i + 1 >= args.len() {
        return Err(NanowebError::Config(format!("{} requires a value", flag)));
    }
    let v = args[*i + 1].clone();
    *i += 2;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_listen_and_config() {
        let o = parse(&argv(&["--listen", "0.0.0.0:7", "--config", "a.toml"])).unwrap();
        assert_eq!(o.listen.as_deref(), Some("0.0.0.0:7"));
        assert_eq!(o.config_path.as_deref(), Some("a.toml"));
    }

    #[test]
    fn parses_config_format() {
        let o = parse(&argv(&["--config-format", "ini"])).unwrap();
        assert_eq!(o.config_format, Some(ConfigFormat::Ini));
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(parse(&argv(&["--bogus"])).is_err());
    }

    #[test]
    fn rejects_missing_value() {
        assert!(parse(&argv(&["--listen"])).is_err());
    }

    #[test]
    fn parses_metrics_on_off() {
        let o = parse(&argv(&["--metrics", "off"])).unwrap();
        assert_eq!(o.metrics, Some(false));
    }

    #[test]
    fn help_flag_sets_flag() {
        let o = parse(&argv(&["--help"])).unwrap();
        assert!(o.show_help);
    }
}
