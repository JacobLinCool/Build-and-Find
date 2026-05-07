use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub type HandlerFn = Arc<dyn Handler>;

#[async_trait]
pub trait Handler: Send + Sync {
    async fn call(&self, req: Request) -> Result<Response, NanowebError>;
}

#[async_trait]
impl<F, Fut> Handler for F
where
    F: Fn(Request) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<Response, NanowebError>> + Send,
{
    async fn call(&self, req: Request) -> Result<Response, NanowebError> {
        self(req).await
    }
}

#[derive(Clone)]
enum NodeKind {
    Literal(String),
    Param(String),
    Wildcard(String),
}

struct TrieNode {
    kind: NodeKind,
    handlers: HashMap<String, HandlerFn>, // method -> handler
    children: Vec<TrieNode>,
}

impl TrieNode {
    fn new_root() -> Self {
        TrieNode {
            kind: NodeKind::Literal(String::new()),
            handlers: HashMap::new(),
            children: Vec::new(),
        }
    }

    fn new(kind: NodeKind) -> Self {
        TrieNode {
            kind,
            handlers: HashMap::new(),
            children: Vec::new(),
        }
    }
}

pub struct Router {
    root: TrieNode,
}

pub struct RouteMatch {
    pub handler: HandlerFn,
    pub params: HashMap<String, String>,
    pub allowed_methods: Vec<String>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            root: TrieNode::new_root(),
        }
    }

    pub fn add_route(&mut self, method: Method, path: &str, handler: impl Handler + 'static) {
        let segments = split_path(path);
        insert_route(
            &mut self.root,
            &segments,
            method.as_str().to_string(),
            Arc::new(handler),
        );
    }

    pub fn lookup(&self, method: &Method, path: &str) -> Option<RouteMatch> {
        let segments = split_path(path);
        let mut params = HashMap::new();
        let node = find_node(&self.root, &segments, &mut params)?;

        let allowed: Vec<String> = node.handlers.keys().cloned().collect();

        // HEAD falls back to GET
        let method_str = method.as_str();
        let handler = if let Some(h) = node.handlers.get(method_str) {
            h.clone()
        } else if method == &Method::HEAD {
            node.handlers.get("GET")?.clone()
        } else {
            return None;
        };

        Some(RouteMatch {
            handler,
            params,
            allowed_methods: allowed,
        })
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<String> {
        let segments = split_path(path);
        let mut params = HashMap::new();
        if let Some(node) = find_node(&self.root, &segments, &mut params) {
            node.handlers.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }
}

impl Default for Router {
    fn default() -> Self {
        Router::new()
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

fn insert_route(node: &mut TrieNode, segments: &[&str], method: String, handler: HandlerFn) {
    if segments.is_empty() {
        node.handlers.insert(method, handler);
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

    // Find existing child or create
    let child_idx = node.children.iter().position(|c| {
        matches!((&c.kind, &kind),
            (NodeKind::Literal(a), NodeKind::Literal(b)) if a == b,
        ) || matches!(
            (&c.kind, &kind),
            (NodeKind::Param(_), NodeKind::Param(_))
                | (NodeKind::Wildcard(_), NodeKind::Wildcard(_))
        )
    });

    if let Some(idx) = child_idx {
        insert_route(&mut node.children[idx], &segments[1..], method, handler);
    } else {
        let mut child = TrieNode::new(kind);
        insert_route(&mut child, &segments[1..], method, handler);
        node.children.push(child);
    }
}

fn find_node<'a>(
    node: &'a TrieNode,
    segments: &[&str],
    params: &mut HashMap<String, String>,
) -> Option<&'a TrieNode> {
    if segments.is_empty() {
        return Some(node);
    }

    let seg = segments[0];

    // Try literal match first
    for child in &node.children {
        if let NodeKind::Literal(lit) = &child.kind {
            if lit == seg {
                if let Some(n) = find_node(child, &segments[1..], params) {
                    return Some(n);
                }
            }
        }
    }

    // Try param match
    for child in &node.children {
        if let NodeKind::Param(name) = &child.kind {
            let saved = params.get(name).cloned();
            params.insert(name.clone(), seg.to_string());
            if let Some(n) = find_node(child, &segments[1..], params) {
                return Some(n);
            }
            // Backtrack
            if let Some(v) = saved {
                params.insert(name.clone(), v);
            } else {
                params.remove(name);
            }
        }
    }

    // Try wildcard match
    for child in &node.children {
        if let NodeKind::Wildcard(name) = &child.kind {
            // Wildcard consumes all remaining segments
            params.insert(name.clone(), segments.join("/"));
            return Some(child);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::Response;

    async fn dummy_handler(_req: Request) -> Result<Response, NanowebError> {
        Ok(Response::ok().with_text("ok"))
    }

    #[tokio::test]
    async fn test_basic_route() {
        let mut router = Router::new();
        router.add_route(Method::GET, "/hello", dummy_handler);

        let m = router.lookup(&Method::GET, "/hello").unwrap();
        let req = Request::new(Method::GET, "/hello".to_string());
        let resp = m.handler.call(req).await.unwrap();
        assert_eq!(resp.status.code(), 200);
    }

    #[tokio::test]
    async fn test_param_route() {
        let mut router = Router::new();
        router.add_route(Method::GET, "/users/:id", dummy_handler);

        let m = router.lookup(&Method::GET, "/users/42").unwrap();
        assert_eq!(m.params.get("id"), Some(&"42".to_string()));
    }

    #[tokio::test]
    async fn test_wildcard_route() {
        let mut router = Router::new();
        router.add_route(Method::GET, "/files/*path", dummy_handler);

        let m = router.lookup(&Method::GET, "/files/a/b/c").unwrap();
        assert_eq!(m.params.get("path"), Some(&"a/b/c".to_string()));
    }

    #[test]
    fn test_not_found() {
        let router = Router::new();
        assert!(router.lookup(&Method::GET, "/missing").is_none());
    }

    #[tokio::test]
    async fn test_head_falls_back_to_get() {
        let mut router = Router::new();
        router.add_route(Method::GET, "/ping", dummy_handler);

        let m = router.lookup(&Method::HEAD, "/ping");
        assert!(m.is_some());
    }
}
