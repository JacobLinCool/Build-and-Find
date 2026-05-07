//! `nanoweb` is a self-contained HTTP/1.1 web framework and server.
//!
//! See `README.md` for an architectural overview and quickstart.

pub mod body;
pub mod cli;
pub mod config;
pub mod error;
pub mod health;
pub mod http;
pub mod log;
pub mod metrics;
pub mod middleware;
pub mod router;
pub mod server;
pub mod session;
pub mod static_files;
pub mod template;
pub mod tls;

pub use config::Config;
pub use error::{NanowebError, Result};
pub use http::{Method, Request, Response, StatusCode};
pub use middleware::Middleware;
pub use router::Router;
pub use server::App;
pub use template::Renderer;
