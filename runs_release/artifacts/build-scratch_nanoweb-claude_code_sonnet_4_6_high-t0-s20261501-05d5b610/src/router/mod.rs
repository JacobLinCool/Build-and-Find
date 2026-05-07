pub mod trie;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use self::trie::Trie;
use crate::error::NanowebError;
use crate::http::types::Method;
use crate::http::{Request, Response};
use crate::middleware::{Middleware, MiddlewareChain};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub type HandlerFn =
    Arc<dyn Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync>;

#[derive(Clone)]
pub struct RouteEntry {
    pub handlers: HashMap<String, HandlerFn>,
    pub middleware: Vec<Arc<dyn Middleware>>,
}

impl RouteEntry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            middleware: Vec::new(),
        }
    }
}

impl Default for RouteEntry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Router {
    trie: Trie<RouteEntry>,
    global_middleware: Vec<Arc<dyn Middleware>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            trie: Trie::new(),
            global_middleware: Vec::new(),
        }
    }

    pub fn use_middleware(&mut self, mw: impl Middleware + 'static) {
        self.global_middleware.push(Arc::new(mw));
    }

    pub fn use_middleware_arc(&mut self, mw: Arc<dyn Middleware>) {
        self.global_middleware.push(mw);
    }

    fn add_route(&mut self, method: &str, pattern: &str, handler: HandlerFn) {
        let entry = self.trie.lookup_mut_or_insert(pattern);
        entry.handlers.insert(method.to_uppercase(), handler);
    }

    pub fn get(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("GET", pattern, Arc::new(handler));
    }

    pub fn post(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("POST", pattern, Arc::new(handler));
    }

    pub fn put(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("PUT", pattern, Arc::new(handler));
    }

    pub fn delete(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("DELETE", pattern, Arc::new(handler));
    }

    pub fn patch(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("PATCH", pattern, Arc::new(handler));
    }

    pub fn head(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("HEAD", pattern, Arc::new(handler));
    }

    pub fn options(
        &mut self,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route("OPTIONS", pattern, Arc::new(handler));
    }

    pub fn route(
        &mut self,
        method: &str,
        pattern: &str,
        handler: impl Fn(Request) -> BoxFuture<Result<Response, NanowebError>> + Send + Sync + 'static,
    ) {
        self.add_route(method, pattern, Arc::new(handler));
    }

    pub async fn handle(&self, mut req: Request) -> Result<Response, NanowebError> {
        let path = req.path.clone();
        let method = req.method.clone();

        match self.trie.lookup(&path) {
            Some((entry, params)) => {
                req.params = params;

                // Build method string to look up
                let method_str = method.as_str();

                // HEAD falls back to GET handler
                let handler = if method_str == "HEAD" {
                    entry
                        .handlers
                        .get("HEAD")
                        .or_else(|| entry.handlers.get("GET"))
                } else if method_str == "OPTIONS" {
                    // Auto-respond with allowed methods
                    let handler = entry.handlers.get("OPTIONS");
                    if handler.is_none() {
                        let allowed = get_allowed_methods(entry);
                        let mut resp = Response::new(200);
                        resp.headers.insert("Allow", allowed.join(", "));
                        resp.headers.insert("Content-Length", "0");
                        return Ok(resp);
                    }
                    handler
                } else {
                    entry.handlers.get(method_str)
                };

                match handler {
                    Some(handler) => {
                        let suppress_body = method == Method::Head;

                        // Build middleware chain: global + route-specific
                        let mut all_middleware = self.global_middleware.clone();
                        all_middleware.extend(entry.middleware.clone());

                        let chain = MiddlewareChain::new(all_middleware, handler.clone());
                        let mut response = chain.run(req).await?;

                        if suppress_body {
                            let len = response.body.len();
                            response.body.clear();
                            response.headers.insert("Content-Length", len.to_string());
                        }

                        Ok(response)
                    }
                    None => {
                        let allowed = get_allowed_methods(entry);
                        Err(NanowebError::MethodNotAllowed(allowed))
                    }
                }
            }
            None => Err(NanowebError::NotFound),
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

fn get_allowed_methods(entry: &RouteEntry) -> Vec<String> {
    let mut methods: Vec<String> = entry
        .handlers
        .keys()
        .filter(|m| m.as_str() != "HEAD")
        .cloned()
        .collect();

    // If GET is in the list, HEAD is implicitly allowed
    if entry.handlers.contains_key("GET") && !methods.contains(&"HEAD".to_string()) {
        methods.push("HEAD".to_string());
    }

    // OPTIONS is always implicitly supported
    if !methods.contains(&"OPTIONS".to_string()) {
        methods.push("OPTIONS".to_string());
    }

    methods.sort();
    methods
}

// Extension to Trie to allow mutable insertion
impl<V: Default> Trie<V> {
    pub fn lookup_mut_or_insert(&mut self, pattern: &str) -> &mut V {
        self.get_or_insert(pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler(body: &'static str) -> HandlerFn {
        Arc::new(move |_req| Box::pin(async move { Ok(Response::ok().with_text(body)) }))
    }

    #[tokio::test]
    async fn test_basic_routing() {
        let mut router = Router::new();
        router.get("/hello", |_| {
            Box::pin(async { Ok(Response::ok().with_text("hello")) })
        });

        let req = Request::new(
            Method::Get,
            "/hello".to_string(),
            crate::http::Version::Http11,
            crate::http::Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = router.handle(req).await.unwrap();
        assert_eq!(resp.status, 200);
    }

    #[tokio::test]
    async fn test_not_found() {
        let router = Router::new();
        let req = Request::new(
            Method::Get,
            "/missing".to_string(),
            crate::http::Version::Http11,
            crate::http::Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let err = router.handle(req).await.unwrap_err();
        assert!(matches!(err, NanowebError::NotFound));
    }

    #[tokio::test]
    async fn test_method_not_allowed() {
        let mut router = Router::new();
        router.get("/resource", |_| Box::pin(async { Ok(Response::ok()) }));

        let req = Request::new(
            Method::Post,
            "/resource".to_string(),
            crate::http::Version::Http11,
            crate::http::Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let err = router.handle(req).await.unwrap_err();
        assert!(matches!(err, NanowebError::MethodNotAllowed(_)));
    }

    #[tokio::test]
    async fn test_head_fallback_to_get() {
        let mut router = Router::new();
        router.get("/data", |_| {
            Box::pin(async { Ok(Response::ok().with_text("body content")) })
        });

        let req = Request::new(
            Method::Head,
            "/data".to_string(),
            crate::http::Version::Http11,
            crate::http::Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = router.handle(req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert!(resp.body.is_empty());
    }

    #[tokio::test]
    async fn test_options_auto_response() {
        let mut router = Router::new();
        router.get("/resource", |_| Box::pin(async { Ok(Response::ok()) }));
        router.post("/resource", |_| Box::pin(async { Ok(Response::ok()) }));

        let req = Request::new(
            Method::Options,
            "/resource".to_string(),
            crate::http::Version::Http11,
            crate::http::Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = router.handle(req).await.unwrap();
        assert_eq!(resp.status, 200);
        let allow = resp.headers.get("allow").unwrap();
        assert!(allow.contains("GET"));
        assert!(allow.contains("POST"));
    }

    #[tokio::test]
    async fn test_path_params() {
        let mut router = Router::new();
        router.get("/users/:id", |req| {
            Box::pin(async move {
                let id = req.params.get("id").cloned().unwrap_or_default();
                Ok(Response::ok().with_text(id))
            })
        });

        let req = Request::new(
            Method::Get,
            "/users/42".to_string(),
            crate::http::Version::Http11,
            crate::http::Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        let resp = router.handle(req).await.unwrap();
        assert_eq!(String::from_utf8(resp.body).unwrap(), "42");
    }
}
