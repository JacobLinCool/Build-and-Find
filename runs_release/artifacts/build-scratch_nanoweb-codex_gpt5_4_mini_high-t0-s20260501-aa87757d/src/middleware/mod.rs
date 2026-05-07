pub mod basic_auth;
pub mod config;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::{
    app::AppState,
    error::Result,
    http::types::{Request, Response},
};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, req: Request, ctx: Arc<AppState>) -> Result<Response>;
}

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn handle(&self, req: Request, ctx: Arc<AppState>, next: Next<'_>) -> Result<Response>;
}

pub struct Next<'a> {
    middlewares: &'a [Arc<dyn Middleware>],
    handler: &'a dyn Handler,
    ctx: Arc<AppState>,
    index: usize,
}

impl<'a> Next<'a> {
    pub fn new(
        middlewares: &'a [Arc<dyn Middleware>],
        handler: &'a dyn Handler,
        ctx: Arc<AppState>,
    ) -> Self {
        Self {
            middlewares,
            handler,
            ctx,
            index: 0,
        }
    }

    pub async fn run(self, req: Request) -> Result<Response> {
        if self.index >= self.middlewares.len() {
            self.handler.handle(req, self.ctx).await
        } else {
            let next = Self {
                middlewares: self.middlewares,
                handler: self.handler,
                ctx: self.ctx.clone(),
                index: self.index + 1,
            };
            self.middlewares[self.index]
                .handle(req, self.ctx, next)
                .await
        }
    }
}

pub fn build_middleware_from_specs(
    specs: &[crate::config::MiddlewareConfig],
) -> Vec<Arc<dyn Middleware>> {
    use crate::middleware::{
        basic_auth::BasicAuthMiddleware, cors::CorsMiddleware, csrf::CsrfMiddleware,
        gzip::GzipMiddleware, logging::LoggingMiddleware, rate_limit::RateLimitMiddleware,
    };

    let mut middleware: Vec<Arc<dyn Middleware>> = Vec::new();
    for item in specs {
        match item {
            crate::config::MiddlewareConfig::Logging => {
                middleware.push(Arc::new(LoggingMiddleware))
            }
            crate::config::MiddlewareConfig::Gzip => middleware.push(Arc::new(GzipMiddleware)),
            crate::config::MiddlewareConfig::RateLimit {
                tokens_per_second,
                burst,
            } => middleware.push(Arc::new(RateLimitMiddleware::new(
                *tokens_per_second,
                *burst,
            ))),
            crate::config::MiddlewareConfig::Cors { allowed_origins } => {
                middleware.push(Arc::new(CorsMiddleware::new(allowed_origins.clone())))
            }
            crate::config::MiddlewareConfig::Csrf {
                cookie_name,
                header_name,
            } => middleware.push(Arc::new(CsrfMiddleware::new(
                cookie_name.clone(),
                header_name.clone(),
            ))),
            crate::config::MiddlewareConfig::BasicAuth { username, password } => middleware.push(
                Arc::new(BasicAuthMiddleware::new(username.clone(), password.clone())),
            ),
        }
    }
    middleware
}

pub fn build_standard_middleware(config: &crate::config::Config) -> Vec<Arc<dyn Middleware>> {
    use crate::middleware::{
        basic_auth::BasicAuthMiddleware, cors::CorsMiddleware, csrf::CsrfMiddleware,
        rate_limit::RateLimitMiddleware,
    };

    let mut middleware = build_middleware_from_specs(&config.middleware);

    if let Some(cors) = &config.cors {
        middleware.push(Arc::new(CorsMiddleware::new(cors.allowed_origins.clone())));
    }
    if let Some(rate_limit) = &config.rate_limit {
        middleware.push(Arc::new(RateLimitMiddleware::new(
            rate_limit.tokens_per_second,
            rate_limit.burst,
        )));
    }
    if let Some(csrf) = &config.csrf {
        middleware.push(Arc::new(CsrfMiddleware::new(
            csrf.cookie_name.clone(),
            csrf.header_name.clone(),
        )));
    }
    if let Some(basic_auth) = &config.basic_auth {
        middleware.push(Arc::new(BasicAuthMiddleware::new(
            basic_auth.username.clone(),
            basic_auth.password.clone(),
        )));
    }

    middleware
}
