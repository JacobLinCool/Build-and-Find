pub mod trie;

use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};
use crate::middleware::{Handler, Middleware, Next};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use trie::{RouteMatch, Trie};

pub struct Router {
    trie: RwLock<Trie>,
    global_middleware: Vec<Arc<dyn Middleware>>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            trie: RwLock::new(Trie::new()),
            global_middleware: Vec::new(),
        }
    }

    pub fn add_global_middleware(&mut self, m: Arc<dyn Middleware>) {
        self.global_middleware.push(m);
    }

    pub fn route(
        &self,
        method: &str,
        path: &str,
        handler: Arc<dyn Handler>,
        middleware: Vec<Arc<dyn Middleware>>,
    ) {
        self.trie
            .write()
            .unwrap()
            .insert(method, path, handler, middleware);
    }

    pub fn get(&self, path: &str, handler: Arc<dyn Handler>) {
        self.route("GET", path, handler, vec![]);
    }

    pub fn post(&self, path: &str, handler: Arc<dyn Handler>) {
        self.route("POST", path, handler, vec![]);
    }

    pub fn put(&self, path: &str, handler: Arc<dyn Handler>) {
        self.route("PUT", path, handler, vec![]);
    }

    pub fn delete(&self, path: &str, handler: Arc<dyn Handler>) {
        self.route("DELETE", path, handler, vec![]);
    }

    pub fn patch(&self, path: &str, handler: Arc<dyn Handler>) {
        self.route("PATCH", path, handler, vec![]);
    }

    pub fn dispatch(&self, mut req: Request) -> Result<(Next, Request), NanowebError> {
        let trie = self.trie.read().unwrap();
        match trie.lookup(&req.method, &req.path) {
            Some(RouteMatch {
                handler,
                middleware,
                params,
                allowed_methods: _,
            }) => {
                req.params = params;

                // Global middleware + route middleware
                let mut stack = self.global_middleware.clone();
                stack.extend(middleware);

                Ok((Next::new(handler, stack), req))
            }
            None => {
                // Check if path exists for any method (405 vs 404)
                let allowed = trie.lookup_node_allowed_methods(&req.path);
                if allowed.is_empty() {
                    Err(NanowebError::NotFound)
                } else {
                    Err(NanowebError::MethodNotAllowed(allowed))
                }
            }
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::FnHandler;

    fn make_req(method: Method, path: &str) -> Request {
        Request {
            method,
            target: path.to_string(),
            path: path.to_string(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[test]
    fn test_dispatch_found() {
        let router = Router::new();
        router.get(
            "/hello",
            Arc::new(FnHandler::new(|_req| async { Ok(Response::text("hi")) })),
        );
        let req = make_req(Method::Get, "/hello");
        assert!(router.dispatch(req).is_ok());
    }

    #[test]
    fn test_dispatch_not_found() {
        let router = Router::new();
        let req = make_req(Method::Get, "/missing");
        match router.dispatch(req) {
            Err(NanowebError::NotFound) => {}
            other => panic!("expected NotFound, got {:?}", other.err()),
        }
    }

    #[test]
    fn test_dispatch_method_not_allowed() {
        let router = Router::new();
        router.get(
            "/thing",
            Arc::new(FnHandler::new(|_req| async { Ok(Response::text("hi")) })),
        );
        let req = make_req(Method::Post, "/thing");
        match router.dispatch(req) {
            Err(NanowebError::MethodNotAllowed(_)) => {}
            other => panic!("expected MethodNotAllowed, got {:?}", other.err()),
        }
    }

    #[tokio::test]
    async fn test_dispatch_and_run() {
        let router = Router::new();
        router.get(
            "/hello",
            Arc::new(FnHandler::new(|_req| async { Ok(Response::text("hi")) })),
        );
        let req = make_req(Method::Get, "/hello");
        let (next, req) = router.dispatch(req).unwrap();
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status, 200);
    }
}
