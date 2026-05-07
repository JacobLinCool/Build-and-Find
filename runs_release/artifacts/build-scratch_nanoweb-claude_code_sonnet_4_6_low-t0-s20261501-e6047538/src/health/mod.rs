use crate::error::NanowebError;
use crate::http::types::{Request, Response, Status};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
}

pub type CheckFn = Box<dyn Fn() -> HealthStatus + Send + Sync>;

pub struct HealthChecker {
    checks: Arc<Mutex<HashMap<String, CheckFn>>>,
    ready: Arc<Mutex<bool>>,
}

impl HealthChecker {
    pub fn new() -> Self {
        HealthChecker {
            checks: Arc::new(Mutex::new(HashMap::new())),
            ready: Arc::new(Mutex::new(true)),
        }
    }

    pub fn add_check(
        &self,
        name: impl Into<String>,
        check: impl Fn() -> HealthStatus + Send + Sync + 'static,
    ) {
        let mut checks = self.checks.lock().unwrap();
        checks.insert(name.into(), Box::new(check));
    }

    pub fn set_ready(&self, ready: bool) {
        *self.ready.lock().unwrap() = ready;
    }

    pub async fn handle_health(&self, _req: Request) -> Result<Response, NanowebError> {
        let checks = self.checks.lock().unwrap();
        let mut results = HashMap::new();
        let mut all_healthy = true;

        for (name, check) in checks.iter() {
            let status = check();
            let healthy = status == HealthStatus::Healthy;
            if !healthy {
                all_healthy = false;
            }
            results.insert(
                name.clone(),
                match status {
                    HealthStatus::Healthy => "ok".to_string(),
                    HealthStatus::Unhealthy(msg) => format!("error: {}", msg),
                },
            );
        }

        let body = serde_json::json!({
            "status": if all_healthy { "healthy" } else { "unhealthy" },
            "checks": results,
        });

        let status = if all_healthy {
            Status::Ok
        } else {
            Status::ServiceUnavailable
        };

        Ok(Response::new(status).with_json(body.to_string()))
    }

    pub async fn handle_ready(&self, _req: Request) -> Result<Response, NanowebError> {
        let ready = *self.ready.lock().unwrap();
        if ready {
            Ok(Response::ok().with_json("{\"status\":\"ready\"}"))
        } else {
            Ok(Response::new(Status::ServiceUnavailable).with_json("{\"status\":\"not ready\"}"))
        }
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        HealthChecker::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_handler_no_checks() {
        let checker = HealthChecker::new();
        let req = Request::new(crate::http::types::Method::GET, "/healthz".to_string());
        let resp = checker.handle_health(req).await.unwrap();
        assert_eq!(resp.status.code(), 200);
    }

    #[tokio::test]
    async fn test_health_handler_unhealthy() {
        let checker = HealthChecker::new();
        checker.add_check("db", || {
            HealthStatus::Unhealthy("connection refused".to_string())
        });
        let req = Request::new(crate::http::types::Method::GET, "/healthz".to_string());
        let resp = checker.handle_health(req).await.unwrap();
        assert_eq!(resp.status.code(), 503);
    }

    #[tokio::test]
    async fn test_ready_handler() {
        let checker = HealthChecker::new();
        let req = Request::new(crate::http::types::Method::GET, "/readyz".to_string());
        let resp = checker.handle_ready(req).await.unwrap();
        assert_eq!(resp.status.code(), 200);
    }
}
