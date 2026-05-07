use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};
use crate::middleware::{Handler, Middleware};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
enum SegmentKind {
    Literal(String),
    Param(String),
    Wildcard(String),
}

struct TrieNode {
    handlers: HashMap<String, Arc<dyn Handler>>,
    middleware: Vec<Arc<dyn Middleware>>,
    literal_children: HashMap<String, TrieNode>,
    param_child: Option<(String, Box<TrieNode>)>,
    wildcard_child: Option<(String, Box<TrieNode>)>,
}

impl TrieNode {
    fn new() -> Self {
        TrieNode {
            handlers: HashMap::new(),
            middleware: Vec::new(),
            literal_children: HashMap::new(),
            param_child: None,
            wildcard_child: None,
        }
    }
}

pub struct Trie {
    root: TrieNode,
}

pub struct RouteMatch {
    pub handler: Arc<dyn Handler>,
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub params: HashMap<String, String>,
    pub allowed_methods: Vec<String>,
}

impl Trie {
    pub fn new() -> Self {
        Trie {
            root: TrieNode::new(),
        }
    }

    pub fn insert(
        &mut self,
        method: &str,
        path: &str,
        handler: Arc<dyn Handler>,
        middleware: Vec<Arc<dyn Middleware>>,
    ) {
        let segments = split_path(path);
        let node = self.find_or_create_node(&segments);
        node.handlers.insert(method.to_uppercase(), handler);
        if node.middleware.is_empty() {
            node.middleware = middleware;
        }
    }

    fn find_or_create_node(&mut self, segments: &[SegmentKind]) -> &mut TrieNode {
        let mut current = &mut self.root;
        for seg in segments {
            match seg {
                SegmentKind::Literal(s) => {
                    current = current
                        .literal_children
                        .entry(s.clone())
                        .or_insert_with(TrieNode::new);
                }
                SegmentKind::Param(name) => {
                    if current.param_child.is_none() {
                        current.param_child = Some((name.clone(), Box::new(TrieNode::new())));
                    }
                    current = &mut current.param_child.as_mut().unwrap().1;
                }
                SegmentKind::Wildcard(name) => {
                    if current.wildcard_child.is_none() {
                        current.wildcard_child = Some((name.clone(), Box::new(TrieNode::new())));
                    }
                    current = &mut current.wildcard_child.as_mut().unwrap().1;
                }
            }
        }
        current
    }

    pub fn lookup(&self, method: &Method, path: &str) -> Option<RouteMatch> {
        let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut params = HashMap::new();
        let node = Self::lookup_node(&self.root, &path_parts, &mut params)?;

        let method_str = method.as_str();

        // Collect allowed methods
        let allowed_methods: Vec<String> = node.handlers.keys().cloned().collect();

        // HEAD falls back to GET
        let handler = if method == &Method::Head {
            node.handlers
                .get("HEAD")
                .or_else(|| node.handlers.get("GET"))
                .cloned()
        } else if method == &Method::Options {
            // Auto-generate OPTIONS handler
            return Some(RouteMatch {
                handler: Arc::new(OptionsHandler {
                    allowed: allowed_methods.clone(),
                }),
                middleware: node.middleware.clone(),
                params,
                allowed_methods,
            });
        } else {
            node.handlers.get(method_str).cloned()
        };

        handler.map(|h| RouteMatch {
            handler: h,
            middleware: node.middleware.clone(),
            params,
            allowed_methods,
        })
    }

    pub fn lookup_node_allowed_methods(&self, path: &str) -> Vec<String> {
        let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        if let Some(node) = Self::lookup_node(&self.root, &path_parts, &mut params) {
            node.handlers.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }

    fn lookup_node<'a>(
        node: &'a TrieNode,
        segments: &[&str],
        params: &mut HashMap<String, String>,
    ) -> Option<&'a TrieNode> {
        if segments.is_empty() {
            return Some(node);
        }

        let seg = segments[0];
        let rest = &segments[1..];

        // Try literal match first
        if let Some(child) = node.literal_children.get(seg) {
            let result = Self::lookup_node(child, rest, params);
            if result.is_some() {
                return result;
            }
        }

        // Try param match
        if let Some((name, child)) = &node.param_child {
            let saved = params.get(name.as_str()).cloned();
            params.insert(name.clone(), seg.to_string());
            let result = Self::lookup_node(child, rest, params);
            if result.is_some() {
                return result;
            }
            // Restore
            if let Some(v) = saved {
                params.insert(name.clone(), v);
            } else {
                params.remove(name.as_str());
            }
        }

        // Try wildcard match (captures rest of path)
        if let Some((name, child)) = &node.wildcard_child {
            let wildcard_value = std::iter::once(seg)
                .chain(rest.iter().copied())
                .collect::<Vec<_>>()
                .join("/");
            params.insert(name.clone(), wildcard_value);
            // Wildcard node must have handlers
            if !child.handlers.is_empty() {
                return Some(child);
            }
            params.remove(name.as_str());
        }

        None
    }
}

impl Default for Trie {
    fn default() -> Self {
        Self::new()
    }
}

fn split_path(path: &str) -> Vec<SegmentKind> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                SegmentKind::Param(name.to_string())
            } else if let Some(name) = seg.strip_prefix('*') {
                SegmentKind::Wildcard(name.to_string())
            } else {
                SegmentKind::Literal(seg.to_string())
            }
        })
        .collect()
}

struct OptionsHandler {
    allowed: Vec<String>,
}

impl Handler for OptionsHandler {
    fn call(&self, _req: Request) -> crate::middleware::BoxFuture<Result<Response, NanowebError>> {
        let allowed = self.allowed.join(", ");
        Box::pin(async move {
            let mut resp = Response::new(204);
            resp.set_header("Allow", &allowed);
            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::FnHandler;

    fn dummy_handler() -> Arc<dyn Handler> {
        Arc::new(FnHandler::new(|_req: Request| async move {
            Ok(Response::text("ok"))
        }))
    }

    #[test]
    fn test_literal_route() {
        let mut trie = Trie::new();
        trie.insert("GET", "/hello", dummy_handler(), vec![]);
        let m = trie.lookup(&Method::Get, "/hello");
        assert!(m.is_some());
    }

    #[test]
    fn test_param_route() {
        let mut trie = Trie::new();
        trie.insert("GET", "/users/:id", dummy_handler(), vec![]);
        let m = trie.lookup(&Method::Get, "/users/42").unwrap();
        assert_eq!(m.params.get("id"), Some(&"42".to_string()));
    }

    #[test]
    fn test_wildcard_route() {
        let mut trie = Trie::new();
        trie.insert("GET", "/static/*path", dummy_handler(), vec![]);
        let m = trie.lookup(&Method::Get, "/static/css/style.css").unwrap();
        assert_eq!(m.params.get("path"), Some(&"css/style.css".to_string()));
    }

    #[test]
    fn test_missing_route() {
        let trie = Trie::new();
        assert!(trie.lookup(&Method::Get, "/missing").is_none());
    }

    #[test]
    fn test_head_fallback_to_get() {
        let mut trie = Trie::new();
        trie.insert("GET", "/resource", dummy_handler(), vec![]);
        let m = trie.lookup(&Method::Head, "/resource");
        assert!(m.is_some());
    }
}
