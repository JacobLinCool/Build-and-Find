//! Nanoweb — a small, educational HTTP/1.1 web framework.
//!
//! The crate is organised into independent modules so that each
//! concept — parser, router, middleware, body, session, metrics,
//! config — can be reasoned about in isolation.
//!
//! ```no_run
//! use nanoweb::{
//!     default_middleware_chain,
//!     http::types::{Method, Response},
//!     router::{handler, Router},
//! };
//!
//! let mut r = Router::new();
//! r.add(
//!     Method::Get,
//!     "/hello",
//!     handler(|_req| async { Ok(Response::text("hi")) }),
//! )
//! .unwrap();
//! let _chain = default_middleware_chain();
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

pub use config::NanowebConfig;
pub use error::{NanowebError, Result};
pub use server::Server;

use std::collections::HashMap;
use std::sync::Arc;

use crate::middleware::basic_auth::BasicAuthMiddleware;
use crate::middleware::cors::CorsMiddleware;
use crate::middleware::csrf::CsrfMiddleware;
use crate::middleware::digest_auth::{DigestAuthConfig, DigestAuthMiddleware};
use crate::middleware::gzip::GzipMiddleware;
use crate::middleware::logger::{LoggerMiddleware, LogLevel};
use crate::middleware::rate_limit::RateLimitMiddleware;
use crate::middleware::Chain;

/// Build the default middleware chain in the order preserved by the
/// dispatcher (outer-to-inner): logger, rate limit, CORS, gzip, CSRF.
///
/// The `digest_auth` middleware is *not* included by default — it is
/// retained for compatibility but must be opted in explicitly.
pub fn default_middleware_chain() -> Chain {
    let mut c = Chain::new();
    c.push(LoggerMiddleware::new(LogLevel::Info));
    c.push(RateLimitMiddleware::default());
    c.push(CorsMiddleware::default());
    c.push(GzipMiddleware::default());
    c.push(CsrfMiddleware::default());
    c
}

/// Build the complete default middleware chain including the legacy
/// `digest_auth` middleware. Applications that must honour existing
/// digest-based clients should call this factory when constructing
/// their `Server`.
pub fn default_middleware_chain_with_digest(users: &[(&str, &str)]) -> Chain {
    let mut c = default_middleware_chain();
    let mut map = HashMap::new();
    for (u, p) in users {
        map.insert((*u).to_string(), (*p).to_string());
    }
    let cfg = DigestAuthConfig::new("nanoweb", map);
    c.push(DigestAuthMiddleware::new(cfg));
    c
}

/// Convenience: assemble a full `Server` from a `NanowebConfig` and a
/// pre-built router/chain.
pub fn assemble(
    config: NanowebConfig,
    router: crate::router::Router,
    chain: Chain,
) -> Server {
    let metrics = crate::metrics::Metrics::default();
    let health = crate::health::HealthState::new();
    Server::new(router, chain, health, metrics, config)
}

/// Reference to the default basic-auth middleware used by examples.
pub fn default_basic_auth(realm: &str, users: &[(&str, &str)]) -> Arc<BasicAuthMiddleware> {
    let creds = users
        .iter()
        .map(|(u, p)| ((*u).to_string(), (*p).to_string()))
        .collect();
    Arc::new(BasicAuthMiddleware::new(realm, creds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chain_has_expected_names() {
        let c = default_middleware_chain();
        let names = c.names();
        assert!(names.contains(&"logger"));
        assert!(names.contains(&"rate_limit"));
        assert!(names.contains(&"cors"));
        assert!(names.contains(&"gzip"));
        assert!(names.contains(&"csrf"));
    }

    #[test]
    fn digest_variant_adds_digest_auth() {
        let c = default_middleware_chain_with_digest(&[("u", "p")]);
        assert!(c.names().contains(&"digest_auth"));
    }
}
