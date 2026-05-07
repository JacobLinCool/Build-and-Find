pub mod body;
pub mod cli;
pub mod config;
pub mod error;
pub mod health;
pub mod http;
pub mod metrics;
pub mod middleware;
pub mod router;
pub mod session;
pub mod static_files;
pub mod template;
pub mod tls;

#[cfg(feature = "tls")]
pub mod tls_module {
    pub use crate::tls::*;
}

pub use config::{Cli, CliArgs, Config};
pub use error::{NanowebError, Result};
pub use http::{Request, Response};
pub use server::Server;

pub mod server;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lib_compiles() {
        let _config = Config::default_config();
    }
}
