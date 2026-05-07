use crate::error::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use std::collections::HashMap;
use std::sync::Arc;

pub type EndpointFn = Arc<dyn Fn(Request) -> Response + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Param(String),
    Wildcard(String),
}

struct TrieNode {
    children: HashMap<String, TrieNode>,
    param_child: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard_child: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    handlers: HashMap<String, EndpointFn>,
}

impl TrieNode {
    fn new() -> Self {
        TrieNode {
            children: HashMap::new(),
            param_child: None,
            param_name: None,
            wildcard_child: None,
            wildcard_name: None,
            handlers: HashMap::new(),
        }
    }
}

pub struct Router {
    root: TrieNode,
}

impl Clone for Router {
    fn clone(&self) -> Self {
        Router {
            root: clone_trie(&self.root),
        }
    }
}

fn clone_trie(node: &TrieNode) -> TrieNode {
    TrieNode {
        children: node
            .children
            .iter()
            .map(|(k, v)| (k.clone(), clone_trie(v)))
            .collect(),
        param_child: node.param_child.as_ref().map(|c| Box::new(clone_trie(c))),
        param_name: node.param_name.clone(),
        wildcard_child: node
            .wildcard_child
            .as_ref()
            .map(|c| Box::new(clone_trie(c))),
        wildcard_name: node.wildcard_name.clone(),
        handlers: node.handlers.clone(),
    }
}

impl Router {
    pub fn new() -> Self {
        Router {
            root: TrieNode::new(),
        }
    }

    pub fn add_route(&mut self, method: &str, path: &str, handler: EndpointFn) -> Result<()> {
        let segments = parse_path(path);
        let mut current = &mut self.root;

        for segment in &segments {
            match segment {
                Segment::Literal(lit) => {
                    current = current
                        .children
                        .entry(lit.clone())
                        .or_insert_with(TrieNode::new);
                }
                Segment::Param(name) => {
                    if current.param_child.is_none() {
                        current.param_child = Some(Box::new(TrieNode::new()));
                        current.param_name = Some(name.clone());
                    }
                    current = current.param_child.as_mut().unwrap();
                }
                Segment::Wildcard(name) => {
                    if current.wildcard_child.is_none() {
                        current.wildcard_child = Some(Box::new(TrieNode::new()));
                        current.wildcard_name = Some(name.clone());
                    }
                    current = current.wildcard_child.as_mut().unwrap();
                    break;
                }
            }
        }

        current.handlers.insert(method.to_uppercase(), handler);
        Ok(())
    }

    pub fn lookup(&self, method: &str, path: &str) -> Option<LookupResult> {
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        self.lookup_segments(&self.root, &segments, 0, method)
    }

    fn lookup_segments(
        &self,
        node: &TrieNode,
        segments: &[&str],
        idx: usize,
        method: &str,
    ) -> Option<LookupResult> {
        if idx == segments.len() {
            return self.try_resolve(node, method, segments);
        }

        let seg = segments[idx];

        if let Some(child) = node.children.get(seg) {
            if let Some(result) = self.lookup_segments(child, segments, idx + 1, method) {
                return Some(result);
            }
        }

        if let Some(ref param_child) = node.param_child {
            if let Some(result) = self.lookup_segments(param_child, segments, idx + 1, method) {
                return Some(result);
            }
        }

        if let Some(ref wildcard_child) = node.wildcard_child {
            let remaining = segments[idx..].join("/");
            let mut params = HashMap::new();
            if let Some(ref name) = node.wildcard_name {
                params.insert(name.clone(), remaining);
            }
            if let Some(handler) = wildcard_child.handlers.get(method) {
                return Some(LookupResult {
                    handler: handler.clone(),
                    params,
                });
            }
        }

        None
    }

    fn try_resolve(
        &self,
        node: &TrieNode,
        method: &str,
        segments: &[&str],
    ) -> Option<LookupResult> {
        let effective_method = if method == "HEAD" && !node.handlers.contains_key("HEAD") {
            "GET"
        } else {
            method
        };

        if let Some(handler) = node.handlers.get(effective_method) {
            let mut params = HashMap::new();
            self.collect_params(&self.root, segments, 0, &mut params);
            return Some(LookupResult {
                handler: handler.clone(),
                params,
            });
        }

        None
    }

    fn collect_params(
        &self,
        node: &TrieNode,
        segments: &[&str],
        idx: usize,
        params: &mut HashMap<String, String>,
    ) {
        if idx == segments.len() {
            return;
        }

        let seg = segments[idx];

        if let Some(child) = node.children.get(seg) {
            self.collect_params(child, segments, idx + 1, params);
        } else if let (Some(ref param_child), Some(ref name)) =
            (&node.param_child, &node.param_name)
        {
            params.insert(name.clone(), seg.to_string());
            self.collect_params(param_child, segments, idx + 1, params);
        }
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<String> {
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        self.allowed_methods_at(&self.root, &segments, 0)
    }

    fn allowed_methods_at(&self, node: &TrieNode, segments: &[&str], idx: usize) -> Vec<String> {
        if idx == segments.len() {
            return node.handlers.keys().cloned().collect();
        }

        let seg = segments[idx];

        if let Some(child) = node.children.get(seg) {
            return self.allowed_methods_at(child, segments, idx + 1);
        }

        if let Some(ref param_child) = node.param_child {
            return self.allowed_methods_at(param_child, segments, idx + 1);
        }

        Vec::new()
    }
}

pub struct LookupResult {
    pub handler: EndpointFn,
    pub params: HashMap<String, String>,
}

fn parse_path(path: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    for part in path.trim_matches('/').split('/') {
        if part.is_empty() {
            continue;
        }
        if let Some(name) = part.strip_prefix(':') {
            segments.push(Segment::Param(name.to_string()));
        } else if let Some(name) = part.strip_prefix('*') {
            segments.push(Segment::Wildcard(name.to_string()));
        } else {
            segments.push(Segment::Literal(part.to_string()));
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ok_handler(_: Request) -> Response {
        Response::text("ok")
    }

    #[test]
    fn test_literal_route() {
        let mut router = Router::new();
        router
            .add_route("GET", "/hello", Arc::new(ok_handler))
            .unwrap();
        let result = router.lookup("GET", "/hello");
        assert!(result.is_some());
    }

    #[test]
    fn test_param_route() {
        let mut router = Router::new();
        router
            .add_route("GET", "/users/:id", Arc::new(ok_handler))
            .unwrap();
        let result = router.lookup("GET", "/users/42");
        assert!(result.is_some());
        assert_eq!(result.unwrap().params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_wildcard_route() {
        let mut router = Router::new();
        router
            .add_route("GET", "/static/*path", Arc::new(ok_handler))
            .unwrap();
        let result = router.lookup("GET", "/static/css/style.css");
        assert!(result.is_some());
        assert_eq!(result.unwrap().params.get("path").unwrap(), "css/style.css");
    }

    #[test]
    fn test_head_fallback_to_get() {
        let mut router = Router::new();
        router
            .add_route("GET", "/test", Arc::new(ok_handler))
            .unwrap();
        let result = router.lookup("HEAD", "/test");
        assert!(result.is_some());
    }

    #[test]
    fn test_allowed_methods() {
        let mut router = Router::new();
        router
            .add_route("GET", "/item", Arc::new(ok_handler))
            .unwrap();
        router
            .add_route("POST", "/item", Arc::new(ok_handler))
            .unwrap();
        let methods = router.allowed_methods("/item");
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
    }

    #[test]
    fn test_not_found() {
        let router = Router::new();
        assert!(router.lookup("GET", "/nope").is_none());
    }
}
