//! nanoweb — a self-contained HTTP/1.1 web framework and server.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use nanoweb::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut router = Router::new();
//!     router.get("/hello", |req| Box::pin(async move {
//!         Ok(Response::ok().with_text("Hello, world!"))
//!     }));
//! }
//! ```

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

/// Re-exports for common usage.
pub mod prelude {
    pub use crate::error::NanowebError;
    pub use crate::http::types::{Headers, Method, Version};
    pub use crate::http::{Request, Response};
    pub use crate::middleware::{Middleware, Next};
    pub use crate::router::{BoxFuture, Router};
    pub use crate::template::{Context, Renderer, Value};
}
