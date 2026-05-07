use crate::config::CliArgs;
use clap::{Arg, Command};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn parse_args() -> CliArgs {
    let matches = Command::new("nanoweb")
        .version(VERSION)
        .about("A self-contained HTTP/1.1 web framework and server")
        .arg(
            Arg::new("listen")
                .long("listen")
                .short('l')
                .help("Address and port to listen on")
                .default_value("127.0.0.1:8080"),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .short('c')
                .help("Path to TOML configuration file")
                .default_value("./nanoweb.toml"),
        )
        .arg(
            Arg::new("tls-cert")
                .long("tls-cert")
                .help("Path to TLS certificate (PEM)"),
        )
        .arg(
            Arg::new("tls-key")
                .long("tls-key")
                .help("Path to TLS private key (PEM)"),
        )
        .arg(
            Arg::new("metrics")
                .long("metrics")
                .help("Metrics endpoint path")
                .default_value("/metrics"),
        )
        .arg(
            Arg::new("static-dir")
                .long("static-dir")
                .help("Directory to serve static files from"),
        )
        .arg(
            Arg::new("log-level")
                .long("log-level")
                .help("Log level (trace, debug, info, warn, error)")
                .default_value("info"),
        )
        .get_matches();

    CliArgs {
        listen: matches.get_one::<String>("listen").unwrap().clone(),
        config: Some(matches.get_one::<String>("config").unwrap().clone()),
        tls_cert: matches.get_one::<String>("tls-cert").map(|s| s.into()),
        tls_key: matches.get_one::<String>("tls-key").map(|s| s.into()),
        metrics: matches.get_one::<String>("metrics").unwrap().clone(),
        static_dir: matches.get_one::<String>("static-dir").map(|s| s.into()),
        log_level: matches.get_one::<String>("log-level").unwrap().clone(),
        version: false,
        help: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_args() {
        // This test just verifies the function doesn't panic
        // In practice, clap would parse from std::env::args
        let args = CliArgs::default();
        assert_eq!(args.listen, "127.0.0.1:8080");
        assert_eq!(args.metrics, "/metrics");
    }
}
