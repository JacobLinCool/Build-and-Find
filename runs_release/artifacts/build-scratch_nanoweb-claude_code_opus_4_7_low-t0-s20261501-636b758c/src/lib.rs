//! A self-contained HTTP/1.1 web framework.

pub mod body;
pub mod cli;
pub mod config;
pub mod error;
pub mod health;
pub mod http;
pub mod metrics;
pub mod middleware;
pub mod router;
pub mod server;
pub mod session;
pub mod static_files;
pub mod template;
pub mod tls;

pub use error::NanowebError;
pub use http::{Method, Request, Response, StatusCode};
pub use router::Router;
pub use server::App;
