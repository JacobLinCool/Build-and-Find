//! Trie-based router supporting literal segments, single-parameter segments
//! (`/users/:id`), and a trailing wildcard (`/static/*path`).
//!
//! Lookup is O(path length) and does not allocate beyond the parameter map.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Result;
use crate::http::{Method, Request, Response};

/// A boxed async handler.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type Handler =
    Arc<dyn for<'a> Fn(&'a mut Request) -> BoxFuture<'a, Result<Response>> + Send + Sync>;

#[derive(Default)]
struct Node {
    /// Literal children, keyed by segment.
    literal: HashMap<String, Node>,
    /// Single parameter child (e.g. `:id`). Only one allowed per node.
    param: Option<(String, Box<Node>)>,
    /// Wildcard child (e.g. `*path`). Terminal — must be the final segment.
    wildcard: Option<(String, Box<Node>)>,
    /// Method -> handler for terminal matches.
    handlers: HashMap<Method, Handler>,
}

impl Node {
    fn new() -> Self {
        Self::default()
    }
}

#[derive(Default, Clone)]
pub struct Router {
    root: Arc<std::sync::RwLock<Node>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            root: Arc::new(std::sync::RwLock::new(Node::new())),
        }
    }

    pub fn route(&self, method: Method, pattern: &str, handler: Handler) {
        let segments = split_path(pattern);
        let mut root = self.root.write().expect("router lock poisoned");
        let mut node: &mut Node = &mut root;
        for (i, seg) in segments.iter().enumerate() {
            if let Some(name) = seg.strip_prefix(':') {
                let exists = node.param.is_some();
                if !exists {
                    node.param = Some((name.to_string(), Box::new(Node::new())));
                }
                node = node.param.as_mut().expect("param node").1.as_mut();
            } else if let Some(name) = seg.strip_prefix('*') {
                if i + 1 != segments.len() {
                    panic!("wildcard segment must be last in route pattern");
                }
                if node.wildcard.is_none() {
                    node.wildcard = Some((name.to_string(), Box::new(Node::new())));
                }
                node = node.wildcard.as_mut().expect("wildcard node").1.as_mut();
            } else {
                node = node
                    .literal
                    .entry((*seg).to_string())
                    .or_insert_with(Node::new);
            }
        }
        node.handlers.insert(method, handler);
    }

    /// Look up the handler for a method/path. Returns the handler with the
    /// extracted parameters, or, if a route exists for the path but not the
    /// method, the set of allowed methods for that route. `None, None` means
    /// no route was found at all.
    pub fn lookup(
        &self,
        method: Method,
        path: &str,
    ) -> (Option<(Handler, HashMap<String, String>)>, Vec<Method>) {
        let segments = split_path(path);
        let root = self.root.read().expect("router lock poisoned");
        let mut params = HashMap::new();
        let Some(node) = walk(&root, &segments, 0, &mut params) else {
            return (None, Vec::new());
        };
        let allowed: Vec<Method> = node.handlers.keys().copied().collect();
        let handler = node.handlers.get(&method).cloned().or_else(|| {
            if method == Method::Head {
                node.handlers.get(&Method::Get).cloned()
            } else {
                None
            }
        });
        match handler {
            Some(h) => (Some((h, params)), allowed),
            None => (None, allowed),
        }
    }
}

fn walk<'a>(
    node: &'a Node,
    segs: &[&str],
    idx: usize,
    params: &mut HashMap<String, String>,
) -> Option<&'a Node> {
    if idx == segs.len() {
        return Some(node);
    }
    let seg = segs[idx];
    if let Some(child) = node.literal.get(seg) {
        if let Some(n) = walk(child, segs, idx + 1, params) {
            return Some(n);
        }
    }
    if let Some((name, child)) = &node.param {
        params.insert(name.clone(), seg.to_string());
        if let Some(n) = walk(child, segs, idx + 1, params) {
            return Some(n);
        }
        params.remove(name);
    }
    if let Some((name, child)) = &node.wildcard {
        let rest = segs[idx..].join("/");
        params.insert(name.clone(), rest);
        return Some(child);
    }
    None
}

fn split_path(p: &str) -> Vec<&str> {
    p.trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn dummy_handler() -> Handler {
        Arc::new(|_req: &mut Request| Box::pin(async move { Ok(Response::ok().text("ok")) }))
    }

    #[test]
    fn literal_match() {
        let r = Router::new();
        r.route(Method::Get, "/hello", dummy_handler());
        let (h, _) = r.lookup(Method::Get, "/hello");
        assert!(h.is_some());
        let (h, _) = r.lookup(Method::Get, "/nope");
        assert!(h.is_none());
    }

    #[test]
    fn param_match() {
        let r = Router::new();
        r.route(Method::Get, "/users/:id", dummy_handler());
        let (h, _) = r.lookup(Method::Get, "/users/42");
        let (_, params) = h.expect("matched");
        assert_eq!(params.get("id"), Some(&"42".to_string()));
    }

    #[test]
    fn wildcard_match() {
        let r = Router::new();
        r.route(Method::Get, "/static/*path", dummy_handler());
        let (h, _) = r.lookup(Method::Get, "/static/css/app.css");
        let (_, params) = h.expect("matched");
        assert_eq!(params.get("path"), Some(&"css/app.css".to_string()));
    }

    #[test]
    fn method_not_allowed_returns_methods() {
        let r = Router::new();
        r.route(Method::Get, "/x", dummy_handler());
        r.route(Method::Post, "/x", dummy_handler());
        let (h, allowed) = r.lookup(Method::Delete, "/x");
        assert!(h.is_none());
        assert!(allowed.contains(&Method::Get));
        assert!(allowed.contains(&Method::Post));
    }

    #[test]
    fn head_falls_back_to_get() {
        let r = Router::new();
        r.route(Method::Get, "/x", dummy_handler());
        let (h, _) = r.lookup(Method::Head, "/x");
        assert!(h.is_some());
    }
}
