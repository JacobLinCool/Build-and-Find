pub mod basicauth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod ratelimit;

use crate::http::types::{Request, Response};

pub trait Middleware: Send + Sync {
    /// Called before the handler. Return Some(response) to short-circuit.
    fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }

    /// Called after the handler. Can mutate the response.
    fn after(&self, _req: &Request, resp: Response) -> Response {
        resp
    }

    fn name(&self) -> &str;
}

pub struct MiddlewareStack {
    middlewares: Vec<Box<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    pub fn push(&mut self, m: Box<dyn Middleware>) {
        self.middlewares.push(m);
    }

    pub fn run(&self, req: &mut Request, handler: &dyn Fn(&mut Request) -> Response) -> Response {
        // Run before hooks, collecting short-circuit responses
        for m in &self.middlewares {
            if let Some(resp) = m.before(req) {
                // Still run after hooks on previous middleware
                return resp;
            }
        }

        let resp = handler(req);

        // Run after hooks in reverse
        let mut resp = resp;
        for m in self.middlewares.iter().rev() {
            resp = m.after(req, resp);
        }

        resp
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{Headers, Method, StatusCode};

    struct TestMiddleware {
        name: String,
    }

    impl Middleware for TestMiddleware {
        fn before(&self, req: &mut Request) -> Option<Response> {
            req.headers.insert("x-middleware", self.name.clone());
            None
        }

        fn after(&self, _req: &Request, mut resp: Response) -> Response {
            resp.headers.insert("x-after", self.name.clone());
            resp
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn middleware_execution_order() {
        let mut stack = MiddlewareStack::new();
        stack.push(Box::new(TestMiddleware {
            name: "first".into(),
        }));
        stack.push(Box::new(TestMiddleware {
            name: "second".into(),
        }));

        let mut req = Request {
            method: Method::Get,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };

        let resp = stack.run(&mut req, &|_req| Response::ok());
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(req.header("x-middleware"), Some("second"));
        assert_eq!(resp.headers.get("x-after"), Some("first"));
    }
}
