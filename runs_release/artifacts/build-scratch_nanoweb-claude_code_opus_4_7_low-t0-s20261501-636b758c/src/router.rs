//! Trie-based router with path parameters and a trailing wildcard.

use crate::http::{Method, Request, Response};
use crate::NanowebError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn call(&self, req: Request) -> Result<Response, NanowebError>;
}

pub type ArcHandler = Arc<dyn Handler>;

#[async_trait]
impl<F, Fut> Handler for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Response, NanowebError>> + Send + 'static,
{
    async fn call(&self, req: Request) -> Result<Response, NanowebError> {
        (self)(req).await
    }
}

#[derive(Default)]
struct Node {
    literals: HashMap<String, Box<Node>>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<(String, Box<Node>)>,
    handlers: HashMap<Method, ArcHandler>,
}

#[derive(Default)]
pub struct Router {
    root: Node,
}

pub struct Match {
    pub handler: ArcHandler,
    pub method: Method,
    pub head_fallback: bool,
    pub allowed: Vec<Method>,
    pub params: HashMap<String, String>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route(&mut self, method: Method, pattern: &str, handler: ArcHandler) {
        let mut node = &mut self.root;
        for seg in pattern.trim_start_matches('/').split('/') {
            if seg.is_empty() {
                continue;
            }
            if let Some(rest) = seg.strip_prefix('*') {
                let name = rest.to_string();
                if node.wildcard.is_none() {
                    node.wildcard = Some((name, Box::new(Node::default())));
                }
                node = &mut node.wildcard.as_mut().unwrap().1;
                break;
            } else if let Some(rest) = seg.strip_prefix(':') {
                let name = rest.to_string();
                if node.param.is_none() {
                    node.param = Some((name, Box::new(Node::default())));
                }
                node = &mut node.param.as_mut().unwrap().1;
            } else {
                node = node
                    .literals
                    .entry(seg.to_string())
                    .or_insert_with(|| Box::new(Node::default()));
            }
        }
        node.handlers.insert(method, handler);
    }

    pub fn lookup(&self, method: Method, path: &str) -> Option<Match> {
        let segments: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut params = HashMap::new();
        let leaf = match_node(&self.root, &segments, 0, &mut params)?;
        let allowed: Vec<Method> = leaf.handlers.keys().copied().collect();
        if let Some(h) = leaf.handlers.get(&method) {
            return Some(Match {
                handler: h.clone(),
                method,
                head_fallback: false,
                allowed,
                params,
            });
        }
        if method == Method::Head {
            if let Some(h) = leaf.handlers.get(&Method::Get) {
                return Some(Match {
                    handler: h.clone(),
                    method: Method::Get,
                    head_fallback: true,
                    allowed,
                    params,
                });
            }
        }
        // Method not allowed but path matched: signal via empty handler? Use OPTIONS list.
        // For simplicity, return None and let server check by calling allowed_for.
        None
    }

    pub fn allowed_for(&self, path: &str) -> Vec<Method> {
        let segments: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut params = HashMap::new();
        if let Some(leaf) = match_node(&self.root, &segments, 0, &mut params) {
            let mut v: Vec<Method> = leaf.handlers.keys().copied().collect();
            if v.contains(&Method::Get) && !v.contains(&Method::Head) {
                v.push(Method::Head);
            }
            v
        } else {
            Vec::new()
        }
    }
}

fn match_node<'a>(
    node: &'a Node,
    segments: &[&str],
    idx: usize,
    params: &mut HashMap<String, String>,
) -> Option<&'a Node> {
    if idx == segments.len() {
        if !node.handlers.is_empty() {
            return Some(node);
        }
        // wildcard with empty match
        if let Some((name, child)) = &node.wildcard {
            params.insert(name.clone(), String::new());
            if !child.handlers.is_empty() {
                return Some(child);
            }
            params.remove(name);
        }
        return None;
    }
    let seg = segments[idx];
    if let Some(child) = node.literals.get(seg) {
        if let Some(m) = match_node(child, segments, idx + 1, params) {
            return Some(m);
        }
    }
    if let Some((name, child)) = &node.param {
        params.insert(name.clone(), seg.to_string());
        if let Some(m) = match_node(child, segments, idx + 1, params) {
            return Some(m);
        }
        params.remove(name);
    }
    if let Some((name, child)) = &node.wildcard {
        let rest = segments[idx..].join("/");
        params.insert(name.clone(), rest);
        if !child.handlers.is_empty() {
            return Some(child);
        }
        params.remove(name);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::StatusCode;

    fn h() -> ArcHandler {
        Arc::new(|_req: Request| async { Ok(Response::new(StatusCode::OK)) })
    }

    #[test]
    fn literal_match() {
        let mut r = Router::new();
        r.route(Method::Get, "/foo/bar", h());
        assert!(r.lookup(Method::Get, "/foo/bar").is_some());
        assert!(r.lookup(Method::Get, "/foo").is_none());
    }

    #[test]
    fn param_match() {
        let mut r = Router::new();
        r.route(Method::Get, "/users/:id", h());
        let m = r.lookup(Method::Get, "/users/42").unwrap();
        assert_eq!(m.params.get("id"), Some(&"42".to_string()));
    }

    #[test]
    fn wildcard_match() {
        let mut r = Router::new();
        r.route(Method::Get, "/static/*path", h());
        let m = r.lookup(Method::Get, "/static/a/b/c").unwrap();
        assert_eq!(m.params.get("path"), Some(&"a/b/c".to_string()));
    }

    #[test]
    fn head_falls_back_to_get() {
        let mut r = Router::new();
        r.route(Method::Get, "/x", h());
        let m = r.lookup(Method::Head, "/x").unwrap();
        assert!(m.head_fallback);
    }

    #[test]
    fn allowed_for_path() {
        let mut r = Router::new();
        r.route(Method::Get, "/x", h());
        r.route(Method::Post, "/x", h());
        let allowed = r.allowed_for("/x");
        assert!(allowed.contains(&Method::Get));
        assert!(allowed.contains(&Method::Post));
        assert!(allowed.contains(&Method::Head));
    }
}
