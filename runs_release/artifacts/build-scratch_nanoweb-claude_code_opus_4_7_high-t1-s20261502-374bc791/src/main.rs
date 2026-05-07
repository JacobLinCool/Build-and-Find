use std::sync::Arc;

use clap::Parser;
use nanoweb::cli::Args;
use nanoweb::config::Config;
use nanoweb::error::Result;
use nanoweb::log::{self, Level};
use nanoweb::server::{run, App};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let cfg = Config::load(&args)?;

    if let Some(level) = Level::parse(&cfg.log_level) {
        log::set_level(level);
    }

    if cfg.tls_cert.is_some() || cfg.tls_key.is_some() {
        #[cfg(not(feature = "tls"))]
        {
            return Err(nanoweb::error::NanowebError::Tls(
                "TLS support is not compiled in (build with --features tls)".into(),
            ));
        }
    }

    let app = Arc::new(App::from_config(&cfg));
    run(cfg, app).await
}
