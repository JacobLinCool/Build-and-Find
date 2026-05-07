use crate::error::NanowebError;
use crate::http::types::{Request, Response};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct MetricsRegistry {
    inner: Arc<Mutex<MetricsInner>>,
}

struct MetricsInner {
    request_counts: HashMap<(String, String, String), u64>,
    request_duration_buckets: HashMap<(String, String), Vec<(f64, u64)>>,
    request_duration_sum: HashMap<(String, String), f64>,
    request_duration_count: HashMap<(String, String), u64>,
}

const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

impl MetricsRegistry {
    pub fn new() -> Self {
        MetricsRegistry {
            inner: Arc::new(Mutex::new(MetricsInner {
                request_counts: HashMap::new(),
                request_duration_buckets: HashMap::new(),
                request_duration_sum: HashMap::new(),
                request_duration_count: HashMap::new(),
            })),
        }
    }

    pub fn record_request(&self, method: &str, status_class: &str, route: &str) {
        let key = (
            method.to_string(),
            status_class.to_string(),
            route.to_string(),
        );
        let mut inner = self.inner.lock().unwrap();
        *inner.request_counts.entry(key).or_insert(0) += 1;
    }

    pub fn record_duration(&self, method: &str, route: &str, duration_secs: f64) {
        let key = (method.to_string(), route.to_string());
        let mut inner = self.inner.lock().unwrap();

        *inner.request_duration_sum.entry(key.clone()).or_insert(0.0) += duration_secs;
        *inner.request_duration_count.entry(key.clone()).or_insert(0) += 1;

        let buckets = inner
            .request_duration_buckets
            .entry(key)
            .or_insert_with(|| HISTOGRAM_BUCKETS.iter().map(|&b| (b, 0u64)).collect());
        for (bound, count) in buckets.iter_mut() {
            if duration_secs <= *bound {
                *count += 1;
            }
        }
    }

    pub fn render_text(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut out = String::new();

        // http_requests_total
        out.push_str("# HELP http_requests_total Total HTTP requests\n");
        out.push_str("# TYPE http_requests_total counter\n");
        for ((method, status_class, route), count) in &inner.request_counts {
            out.push_str(&format!(
                "http_requests_total{{method=\"{}\",status_class=\"{}\",route=\"{}\"}} {}\n",
                method, status_class, route, count
            ));
        }

        // http_request_duration_seconds
        out.push_str("# HELP http_request_duration_seconds HTTP request duration in seconds\n");
        out.push_str("# TYPE http_request_duration_seconds histogram\n");
        for ((method, route), buckets) in &inner.request_duration_buckets {
            let sum = inner
                .request_duration_sum
                .get(&(method.clone(), route.clone()))
                .copied()
                .unwrap_or(0.0);
            let count = inner
                .request_duration_count
                .get(&(method.clone(), route.clone()))
                .copied()
                .unwrap_or(0);

            for (bound, bucket_count) in buckets {
                out.push_str(&format!(
                    "http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"{}\"}} {}\n",
                    method, route, bound, bucket_count
                ));
            }
            out.push_str(&format!(
                "http_request_duration_seconds_bucket{{method=\"{}\",route=\"{}\",le=\"+Inf\"}} {}\n",
                method, route, count
            ));
            out.push_str(&format!(
                "http_request_duration_seconds_sum{{method=\"{}\",route=\"{}\"}} {}\n",
                method, route, sum
            ));
            out.push_str(&format!(
                "http_request_duration_seconds_count{{method=\"{}\",route=\"{}\"}} {}\n",
                method, route, count
            ));
        }

        out
    }

    pub async fn handle_metrics(&self, _req: Request) -> Result<Response, NanowebError> {
        let text = self.render_text();
        Ok(Response::ok()
            .with_body(text.into_bytes())
            .set_header("content-type", "text/plain; version=0.0.4; charset=utf-8"))
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        MetricsRegistry::new()
    }
}

pub struct RequestTimer {
    start: Instant,
    method: String,
    route: String,
    registry: MetricsRegistry,
}

impl RequestTimer {
    pub fn new(method: String, route: String, registry: MetricsRegistry) -> Self {
        RequestTimer {
            start: Instant::now(),
            method,
            route,
            registry,
        }
    }

    pub fn finish(self, status_class: &str) {
        let duration = self.start.elapsed().as_secs_f64();
        self.registry
            .record_request(&self.method, status_class, &self.route);
        self.registry
            .record_duration(&self.method, &self.route, duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_render() {
        let reg = MetricsRegistry::new();
        reg.record_request("GET", "2xx", "/api/users");
        reg.record_duration("GET", "/api/users", 0.05);

        let text = reg.render_text();
        assert!(text.contains("http_requests_total"));
        assert!(text.contains("http_request_duration_seconds"));
        assert!(text.contains("GET"));
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        let reg = MetricsRegistry::new();
        let req = Request::new(crate::http::types::Method::GET, "/metrics".to_string());
        let resp = reg.handle_metrics(req).await.unwrap();
        assert_eq!(resp.status.code(), 200);
    }
}
