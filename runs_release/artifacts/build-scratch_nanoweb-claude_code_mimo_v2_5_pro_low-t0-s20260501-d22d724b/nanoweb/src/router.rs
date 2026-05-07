use crate::http::{Method, Request, Response};
use crate::middleware::Middleware;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type Handler =
    Box<dyn Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync>;
pub type MiddlewareChain = Vec<Arc<dyn Middleware>>;

#[derive(Clone)]
pub struct Route {
    pub handler: Arc<Handler>,
    pub middleware: MiddlewareChain,
}

struct TrieNode {
    literal: HashMap<String, TrieNode>,
    param: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    handlers: HashMap<Method, Route>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            literal: HashMap::new(),
            param: None,
            param_name: None,
            wildcard: None,
            wildcard_name: None,
            handlers: HashMap::new(),
        }
    }
}

pub struct Router {
    root: TrieNode,
    global_middleware: MiddlewareChain,
}

impl Router {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
            global_middleware: Vec::new(),
        }
    }

    pub fn middleware(mut self, mw: impl Middleware + 'static) -> Self {
        self.global_middleware.push(Arc::new(mw));
        self
    }

    pub fn route(
        &mut self,
        method: Method,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut node = &mut self.root;

        for seg in &segments {
            if seg.starts_with('*') {
                let name = seg[1..].to_string();
                if node.wildcard.is_none() {
                    node.wildcard = Some(Box::new(TrieNode::new()));
                    node.wildcard_name = Some(name);
                }
                node = node.wildcard.as_mut().unwrap();
            } else if seg.starts_with(':') {
                let name = seg[1..].to_string();
                if node.param.is_none() {
                    node.param = Some(Box::new(TrieNode::new()));
                    node.param_name = Some(name);
                }
                node = node.param.as_mut().unwrap();
            } else {
                node = node
                    .literal
                    .entry(seg.to_string())
                    .or_insert_with(TrieNode::new);
            }
        }

        node.handlers.insert(
            method,
            Route {
                handler: Arc::new(Box::new(handler)),
                middleware: self.global_middleware.clone(),
            },
        );
    }

    pub fn get(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Get, path, handler);
    }

    pub fn post(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Post, path, handler);
    }

    pub fn put(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Put, path, handler);
    }

    pub fn delete(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Delete, path, handler);
    }

    pub fn patch(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Patch, path, handler);
    }

    pub fn head(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Head, path, handler);
    }

    pub fn options(
        &mut self,
        path: &str,
        handler: impl Fn(Request) -> Pin<Box<dyn Future<Output = Response> + Send>>
            + Send
            + Sync
            + 'static,
    ) {
        self.route(Method::Options, path, handler);
    }

    pub fn lookup(&self, method: Method, path: &str) -> Option<(Route, HashMap<String, String>)> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        self.lookup_node(&self.root, &segments, 0, method, &mut params)
    }

    fn lookup_node(
        &self,
        node: &TrieNode,
        segments: &[&str],
        idx: usize,
        method: Method,
        params: &mut HashMap<String, String>,
    ) -> Option<(Route, HashMap<String, String>)> {
        if idx == segments.len() {
            return node
                .handlers
                .get(&method)
                .map(|r| (r.clone(), params.clone()));
        }

        let seg = segments[idx];

        // Try literal
        if let Some(child) = node.literal.get(seg) {
            if let Some(result) = self.lookup_node(child, segments, idx + 1, method, params) {
                return Some(result);
            }
        }

        // Try param
        if let Some(ref child) = node.param {
            if let Some(ref name) = node.param_name {
                params.insert(name.clone(), seg.to_string());
                let result = self.lookup_node(child, segments, idx + 1, method, params);
                params.remove(name);
                if let Some(r) = result {
                    return Some(r);
                }
            }
        }

        // Try wildcard
        if let Some(ref child) = node.wildcard {
            if let Some(ref name) = node.wildcard_name {
                let remaining = segments[idx..].join("/");
                params.insert(name.clone(), remaining);
                // Wildcard matches everything from here
                if let Some(route) = child.handlers.get(&method) {
                    return Some((route.clone(), params.clone()));
                }
                // Also check handlers on wildcard node's children (shouldn't happen normally)
                params.remove(name);
            }
        }

        // For HEAD, fall back to GET
        if method == Method::Head {
            return self.lookup_node(node, segments, idx, Method::Get, params);
        }

        None
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<Method> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        let mut methods = Vec::new();
        self.collect_methods(&self.root, &segments, 0, &mut params, &mut methods);
        methods
    }

    fn collect_methods(
        &self,
        node: &TrieNode,
        segments: &[&str],
        idx: usize,
        params: &mut HashMap<String, String>,
        methods: &mut Vec<Method>,
    ) {
        if idx == segments.len() {
            for m in node.handlers.keys() {
                if !methods.contains(m) {
                    methods.push(*m);
                }
            }
            return;
        }

        let seg = segments[idx];

        if let Some(child) = node.literal.get(seg) {
            self.collect_methods(child, segments, idx + 1, params, methods);
        }

        if let Some(ref child) = node.param {
            if let Some(ref name) = node.param_name {
                params.insert(name.clone(), seg.to_string());
                self.collect_methods(child, segments, idx + 1, params, methods);
                params.remove(name);
            }
        }

        if let Some(ref child) = node.wildcard {
            if node.wildcard_name.is_some() {
                for m in child.handlers.keys() {
                    if !methods.contains(m) {
                        methods.push(*m);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Request, Response};

    fn ok_handler(_req: Request) -> Pin<Box<dyn Future<Output = Response> + Send>> {
        Box::pin(async { Response::ok() })
    }

    #[test]
    fn test_literal_route() {
        let mut router = Router::new();
        router.get("/hello", ok_handler);
        let (route, params) = router.lookup(Method::Get, "/hello").unwrap();
        assert!(params.is_empty());
    }

    #[test]
    fn test_param_route() {
        let mut router = Router::new();
        router.get("/users/:id", ok_handler);
        let (_, params) = router.lookup(Method::Get, "/users/42").unwrap();
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_wildcard_route() {
        let mut router = Router::new();
        router.get("/static/*path", ok_handler);
        let (_, params) = router.lookup(Method::Get, "/static/css/style.css").unwrap();
        assert_eq!(params.get("path").unwrap(), "css/style.css");
    }

    #[test]
    fn test_allowed_methods() {
        let mut router = Router::new();
        router.get("/resource", ok_handler);
        router.post("/resource", ok_handler);
        router.delete("/resource", ok_handler);
        let methods = router.allowed_methods("/resource");
        assert!(methods.contains(&Method::Get));
        assert!(methods.contains(&Method::Post));
        assert!(methods.contains(&Method::Delete));
    }

    #[test]
    fn test_no_match() {
        let mut router = Router::new();
        router.get("/hello", ok_handler);
        assert!(router.lookup(Method::Get, "/world").is_none());
    }

    #[test]
    fn test_head_fallback_to_get() {
        let mut router = Router::new();
        router.get("/page", ok_handler);
        let result = router.lookup(Method::Head, "/page");
        assert!(result.is_some());
    }
}
