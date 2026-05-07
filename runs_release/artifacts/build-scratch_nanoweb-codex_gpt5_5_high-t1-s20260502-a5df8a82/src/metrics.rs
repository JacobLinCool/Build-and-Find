use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};
use crate::middleware::Middleware;

const BUCKETS: [f64; 8] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 1.0, 5.0];

#[derive(Clone, Default)]
pub struct Metrics {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    requests: BTreeMap<(String, String, String), u64>,
    durations: BTreeMap<(String, String, String), Vec<f64>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, method: &str, status_class: &str, route: &str, seconds: f64) {
        let mut inner = self.inner.lock().expect("metrics mutex poisoned");
        let key = (
            method.to_string(),
            status_class.to_string(),
            route.to_string(),
        );
        *inner.requests.entry(key.clone()).or_default() += 1;
        inner.durations.entry(key).or_default().push(seconds);
    }

    pub fn render(&self) -> String {
        let inner = self.inner.lock().expect("metrics mutex poisoned");
        let mut out = String::new();
        out.push_str("# TYPE nanoweb_http_requests_total counter\n");
        for ((method, class, route), value) in &inner.requests {
            out.push_str(&format!(
                "nanoweb_http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(class),
                esc(route),
                value
            ));
        }
        out.push_str("# TYPE nanoweb_http_request_duration_seconds histogram\n");
        for ((method, class, route), values) in &inner.durations {
            for bucket in BUCKETS {
                let cumulative = values.iter().filter(|value| **value <= bucket).count();
                out.push_str(&format!(
                    "nanoweb_http_request_duration_seconds_bucket{{method=\"{}\",status_class=\"{}\",route=\"{}\",le=\"{}\"}} {}\n",
                    esc(method),
                    esc(class),
                    esc(route),
                    bucket,
                    cumulative
                ));
            }
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_count{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(class),
                esc(route),
                values.len()
            ));
            out.push_str(&format!(
                "nanoweb_http_request_duration_seconds_sum{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                esc(method),
                esc(class),
                esc(route),
                values.iter().sum::<f64>()
            ));
        }
        out
    }

    pub fn handler(&self) -> impl crate::router::Handler {
        let metrics = self.clone();
        move |_req: Request| {
            let metrics = metrics.clone();
            async move {
                let mut response = Response::text(StatusCode::OK, metrics.render());
                response.set_header("Content-Type", "text/plain; version=0.0.4");
                Ok(response)
            }
        }
    }
}

pub struct MetricsMiddleware {
    metrics: Metrics,
    starts: Mutex<HashMap<String, Instant>>,
}

impl MetricsMiddleware {
    pub fn new(metrics: Metrics) -> Self {
        Self {
            metrics,
            starts: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    async fn before(&self, request: &mut Request) -> Result<Option<Response>> {
        self.starts
            .lock()
            .expect("metrics timer mutex poisoned")
            .insert(request.request_id.clone(), Instant::now());
        Ok(None)
    }

    async fn after(&self, request: &Request, response: &mut Response) -> Result<()> {
        let route = request.route_pattern.as_deref().unwrap_or(&request.path);
        let seconds = self
            .starts
            .lock()
            .expect("metrics timer mutex poisoned")
            .remove(&request.request_id)
            .map(|started| started.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        self.metrics.record(
            request.method.as_str(),
            &response.status.class(),
            route,
            seconds,
        );
        Ok(())
    }
}

fn esc(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counter() {
        let metrics = Metrics::new();
        metrics.record("GET", "2xx", "/", 0.01);
        assert!(metrics.render().contains("nanoweb_http_requests_total"));
    }
}
