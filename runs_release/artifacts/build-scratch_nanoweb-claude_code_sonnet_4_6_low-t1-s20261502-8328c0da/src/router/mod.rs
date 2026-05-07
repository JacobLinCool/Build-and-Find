use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};

pub type HandlerFn = Arc<
    dyn Fn(Request) -> Pin<Box<dyn Future<Output = Result<Response, NanowebError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct Route {
    pub method: Method,
    pub pattern: String,
    pub handler: HandlerFn,
}

/// Trie node types
#[derive(Debug, Clone, PartialEq)]
enum NodeKind {
    Literal(String),
    Param(String),   // :name
    Wildcard(String), // *name
}

struct TrieNode {
    kind: NodeKind,
    routes: HashMap<String, HandlerFn>, // method -> handler
    children: Vec<TrieNode>,
}

impl TrieNode {
    fn new(kind: NodeKind) -> Self {
        TrieNode {
            kind,
            routes: HashMap::new(),
            children: Vec::new(),
        }
    }

    fn insert(&mut self, segments: &[&str], method: Method, handler: HandlerFn) {
        if segments.is_empty() {
            self.routes.insert(method.as_str().to_string(), handler);
            return;
        }

        let seg = segments[0];
        let kind = if seg.starts_with(':') {
            NodeKind::Param(seg[1..].to_string())
        } else if seg.starts_with('*') {
            NodeKind::Wildcard(seg[1..].to_string())
        } else {
            NodeKind::Literal(seg.to_string())
        };

        // Find existing child with matching kind
        if let Some(child) = self.children.iter_mut().find(|c| c.kind == kind) {
            child.insert(&segments[1..], method, handler);
            return;
        }

        // Create new child
        let mut child = TrieNode::new(kind);
        child.insert(&segments[1..], method, handler);
        self.children.push(child);
    }

    fn lookup<'a>(
        &'a self,
        segments: &[&'a str],
        params: &mut HashMap<String, String>,
    ) -> Option<(&'a HashMap<String, HandlerFn>, Vec<String>)> {
        if segments.is_empty() {
            if self.routes.is_empty() {
                return None;
            }
            return Some((&self.routes, params.values().cloned().collect()));
        }

        let seg = segments[0];

        for child in &self.children {
            match &child.kind {
                NodeKind::Literal(lit) => {
                    if lit == seg {
                        if let Some(result) = child.lookup(&segments[1..], params) {
                            return Some(result);
                        }
                    }
                }
                NodeKind::Param(name) => {
                    params.insert(name.clone(), seg.to_string());
                    if let Some(result) = child.lookup(&segments[1..], params) {
                        return Some(result);
                    }
                    params.remove(name);
                }
                NodeKind::Wildcard(name) => {
                    // Consume remaining segments
                    let rest = segments.join("/");
                    params.insert(name.clone(), rest);
                    if child.routes.is_empty() {
                        params.remove(name);
                    } else {
                        return Some((&child.routes, params.values().cloned().collect()));
                    }
                }
            }
        }

        None
    }

    fn collect_methods(&self, segments: &[&str], params: &mut HashMap<String, String>) -> Vec<String> {
        if segments.is_empty() {
            return self.routes.keys().cloned().collect();
        }

        let seg = segments[0];
        let mut methods = Vec::new();

        for child in &self.children {
            match &child.kind {
                NodeKind::Literal(lit) => {
                    if lit == seg {
                        methods.extend(child.collect_methods(&segments[1..], params));
                    }
                }
                NodeKind::Param(name) => {
                    params.insert(name.clone(), seg.to_string());
                    methods.extend(child.collect_methods(&segments[1..], params));
                    params.remove(name);
                }
                NodeKind::Wildcard(name) => {
                    params.insert(name.clone(), segments.join("/"));
                    methods.extend(child.routes.keys().cloned().collect::<Vec<_>>());
                    params.remove(name);
                }
            }
        }

        methods
    }
}

pub struct Router {
    root: TrieNode,
}

impl Router {
    pub fn new() -> Self {
        Router {
            root: TrieNode::new(NodeKind::Literal(String::new())),
        }
    }

    pub fn add_route(&mut self, method: Method, pattern: &str, handler: HandlerFn) {
        let pattern = pattern.trim_start_matches('/');
        let segments: Vec<&str> = if pattern.is_empty() {
            vec![]
        } else {
            pattern.split('/').collect()
        };
        self.root.insert(&segments, method, handler);
    }

    pub fn lookup(&self, path: &str, method: &Method) -> RouteMatch {
        let path = path.trim_start_matches('/');
        let segments: Vec<&str> = if path.is_empty() {
            vec![]
        } else {
            path.split('/').collect()
        };

        let mut params = HashMap::new();
        match self.root.lookup(&segments, &mut params) {
            Some((routes, _)) => {
                if let Some(handler) = routes.get(method.as_str()) {
                    RouteMatch::Found {
                        handler: handler.clone(),
                        params,
                    }
                } else {
                    // Method not allowed
                    let allowed: Vec<String> = routes.keys().cloned().collect();
                    RouteMatch::MethodNotAllowed(allowed)
                }
            }
            None => {
                // Check if any method exists for this path
                let mut params2 = HashMap::new();
                let allowed = self.root.collect_methods(&segments, &mut params2);
                if !allowed.is_empty() {
                    RouteMatch::MethodNotAllowed(allowed)
                } else {
                    RouteMatch::NotFound
                }
            }
        }
    }

    pub fn route<F, Fut>(&mut self, method: Method, pattern: &str, handler: F)
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, NanowebError>> + Send + 'static,
    {
        let handler: HandlerFn = Arc::new(move |req| Box::pin(handler(req)));
        self.add_route(method, pattern, handler);
    }

    pub fn get<F, Fut>(&mut self, pattern: &str, handler: F)
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, NanowebError>> + Send + 'static,
    {
        self.route(Method::Get, pattern, handler);
    }

    pub fn post<F, Fut>(&mut self, pattern: &str, handler: F)
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response, NanowebError>> + Send + 'static,
    {
        self.route(Method::Post, pattern, handler);
    }
}

pub enum RouteMatch {
    Found {
        handler: HandlerFn,
        params: HashMap<String, String>,
    },
    NotFound,
    MethodNotAllowed(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn dummy_handler(_req: Request) -> Result<Response, NanowebError> {
        Ok(Response::text("ok"))
    }

    #[test]
    fn test_router_literal() {
        let mut router = Router::new();
        router.get("/hello", dummy_handler);

        let addr = "127.0.0.1:0".parse().unwrap();
        let req = Request::new(
            Method::Get,
            "/hello".to_string(),
            HashMap::new(),
            crate::http::types::Headers::new(),
            vec![],
            addr,
        );

        match router.lookup(&req.path, &req.method) {
            RouteMatch::Found { .. } => {}
            _ => panic!("Expected Found"),
        }
    }

    #[test]
    fn test_router_param() {
        let mut router = Router::new();
        router.get("/users/:id", dummy_handler);

        match router.lookup("/users/42", &Method::Get) {
            RouteMatch::Found { params, .. } => {
                assert_eq!(params.get("id"), Some(&"42".to_string()));
            }
            _ => panic!("Expected Found"),
        }
    }

    #[test]
    fn test_router_not_found() {
        let router = Router::new();
        match router.lookup("/nonexistent", &Method::Get) {
            RouteMatch::NotFound => {}
            _ => panic!("Expected NotFound"),
        }
    }

    #[test]
    fn test_router_method_not_allowed() {
        let mut router = Router::new();
        router.get("/hello", dummy_handler);

        match router.lookup("/hello", &Method::Post) {
            RouteMatch::MethodNotAllowed(methods) => {
                assert!(methods.contains(&"GET".to_string()));
            }
            _ => panic!("Expected MethodNotAllowed"),
        }
    }
}
