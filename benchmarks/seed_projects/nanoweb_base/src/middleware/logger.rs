//! Structured request logger. Emits one line per request to stderr
//! including the per-request id, method, path, status, and elapsed ms.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::error::NanowebError;
use crate::http::types::{Request, Response, X_REQUEST_ID_HEADER};
use crate::middleware::{Middleware, MiddlewareOutcome};

/// Log levels the middleware understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn parse(raw: &str) -> Option<LogLevel> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// A request logger.
pub struct LoggerMiddleware {
    pub level: LogLevel,
    // One atomic counter helps with test isolation if multiple loggers
    // are wired into a chain.
    req_count: AtomicU64,
}

impl Default for LoggerMiddleware {
    fn default() -> Self {
        Self::new(LogLevel::Info)
    }
}

impl LoggerMiddleware {
    pub fn new(level: LogLevel) -> Self {
        Self {
            level,
            req_count: AtomicU64::new(0),
        }
    }

    fn log_line(&self, line: &str) {
        if matches!(self.level, LogLevel::Trace | LogLevel::Debug | LogLevel::Info) {
            eprintln!("{}", line);
        }
    }
}

thread_local! {
    static START: std::cell::RefCell<Option<(Instant, String)>> = const { std::cell::RefCell::new(None) };
}

impl Middleware for LoggerMiddleware {
    fn name(&self) -> &'static str {
        "logger"
    }

    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<MiddlewareOutcome, NanowebError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.req_count.fetch_add(1, Ordering::Relaxed);
            // Stash a start timestamp keyed by request id in a simple
            // thread-local. The server is single-threaded per task so
            // this is fine for non-concurrent tests; production logging
            // would use per-task storage.
            START.with(|slot| {
                *slot.borrow_mut() = Some((Instant::now(), req.id.as_str().to_string()));
            });
            self.log_line(&format!(
                "[{}] [{}] {} {}",
                self.level.as_str(),
                req.id.as_str(),
                req.method.as_str(),
                req.path
            ));
            Ok(MiddlewareOutcome::Continue(req))
        })
    }

    fn after<'a>(
        &'a self,
        req: &'a Request,
        mut resp: Response,
    ) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send + 'a>> {
        Box::pin(async move {
            // Mirror the request id on the response.
            resp.headers
                .set(X_REQUEST_ID_HEADER, req.id.as_str().to_string());

            let elapsed_ms = START.with(|slot| {
                let start = slot.borrow_mut().take();
                match start {
                    Some((t, _id)) => t.elapsed().as_millis(),
                    None => 0,
                }
            });
            self.log_line(&format!(
                "[{}] [{}] {} {} -> {} ({}ms)",
                self.level.as_str(),
                req.id.as_str(),
                req.method.as_str(),
                req.path,
                resp.status.0,
                elapsed_ms
            ));
            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Method, StatusCode};

    #[tokio::test(flavor = "current_thread")]
    async fn adds_x_request_id() {
        let m = LoggerMiddleware::new(LogLevel::Error);
        let mut req = Request::new(Method::Get, "/x");
        req.id = crate::http::types::RequestId("abc".into());
        match m.before(req.clone()).await.unwrap() {
            MiddlewareOutcome::Continue(_) => {}
            _ => panic!("logger should not short-circuit"),
        }
        let resp = Response::status(StatusCode::OK);
        let out = m.after(&req, resp).await.unwrap();
        assert_eq!(out.headers.get(X_REQUEST_ID_HEADER), Some("abc"));
    }

    #[test]
    fn log_level_parses() {
        assert_eq!(LogLevel::parse("INFO"), Some(LogLevel::Info));
        assert!(LogLevel::parse("chatty").is_none());
    }
}
