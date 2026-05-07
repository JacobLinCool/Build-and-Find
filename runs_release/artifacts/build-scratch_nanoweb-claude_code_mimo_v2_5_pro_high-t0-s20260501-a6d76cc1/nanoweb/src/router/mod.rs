use crate::http::types::{Method, Request, Response};
use std::collections::HashMap;

pub type Handler = Box<
    dyn Fn(Request) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
        + Send
        + Sync,
>;

pub struct RouteEntry {
    pub handler: Handler,
    pub method: Method,
}

struct TrieNode {
    children: HashMap<String, TrieNode>,
    param_child: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    handlers: HashMap<Method, Handler>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            param_child: None,
            param_name: None,
            wildcard: None,
            wildcard_name: None,
            handlers: HashMap::new(),
        }
    }
}

pub struct Router {
    root: TrieNode,
}

impl Router {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
        }
    }

    pub fn add(&mut self, method: Method, path: &str, handler: Handler) {
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut node = &mut self.root;

        for seg in &segments {
            if seg.starts_with(':') {
                let name = seg[1..].to_string();
                if node.param_child.is_none() {
                    node.param_child = Some(Box::new(TrieNode::new()));
                    node.param_name = Some(name.clone());
                }
                node = node.param_child.as_mut().unwrap();
            } else if seg.starts_with('*') {
                let name = seg[1..].to_string();
                if node.wildcard.is_none() {
                    node.wildcard = Some(Box::new(TrieNode::new()));
                    node.wildcard_name = Some(name.clone());
                }
                node = node.wildcard.as_mut().unwrap();
                break; // wildcard consumes the rest
            } else {
                let entry = node
                    .children
                    .entry(seg.to_string())
                    .or_insert_with(TrieNode::new);
                node = entry;
            }
        }

        node.handlers.insert(method, handler);
    }

    pub fn find(&self, method: Method, path: &str) -> Option<(&Handler, HashMap<String, String>)> {
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        self.find_recursive(&self.root, &segments, 0, &mut HashMap::new(), method)
    }

    fn find_recursive<'a>(
        &'a self,
        node: &'a TrieNode,
        segments: &[&str],
        idx: usize,
        params: &mut HashMap<String, String>,
        method: Method,
    ) -> Option<(&'a Handler, HashMap<String, String>)> {
        if idx == segments.len() {
            return node.handlers.get(&method).map(|h| (h, params.clone()));
        }

        let seg = segments[idx];

        // Try literal match
        if let Some(child) = node.children.get(seg) {
            if let Some(result) = self.find_recursive(child, segments, idx + 1, params, method) {
                return Some(result);
            }
        }

        // Try param match
        if let Some(ref child) = node.param_child {
            if let Some(ref name) = node.param_name {
                params.insert(name.clone(), seg.to_string());
                if let Some(result) = self.find_recursive(child, segments, idx + 1, params, method)
                {
                    return Some(result);
                }
                params.remove(name);
            }
        }

        // Try wildcard match - consumes remaining segments
        if let Some(ref child) = node.wildcard {
            if let Some(ref name) = node.wildcard_name {
                let rest = segments[idx..].join("/");
                params.insert(name.clone(), rest);
                // Wildcard node stores handler at its own level; check directly
                let result = child.handlers.get(&method).map(|h| (h, params.clone()));
                if result.is_some() {
                    return result;
                }
                params.remove(name);
            }
        }

        None
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<Method> {
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut methods = Vec::new();
        self.collect_methods(&self.root, &segments, 0, &mut methods);
        methods.sort_by_key(|m| m.as_str().to_string());
        methods.dedup();
        methods
    }

    fn collect_methods(
        &self,
        node: &TrieNode,
        segments: &[&str],
        idx: usize,
        methods: &mut Vec<Method>,
    ) {
        if idx == segments.len() {
            methods.extend(node.handlers.keys().copied());
            return;
        }

        let seg = segments[idx];

        if let Some(child) = node.children.get(seg) {
            self.collect_methods(child, segments, idx + 1, methods);
        }

        if let Some(ref child) = node.param_child {
            self.collect_methods(child, segments, idx + 1, methods);
        }

        if node.wildcard.is_some() {
            if let Some(ref child) = node.wildcard {
                self.collect_methods(child, &[], segments.len(), methods);
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

    fn dummy_handler() -> Handler {
        Box::new(|_req: Request| Box::pin(async { Response::ok() }))
    }

    #[test]
    fn test_literal_route() {
        let mut router = Router::new();
        router.add(Method::Get, "/hello", dummy_handler());
        let result = router.find(Method::Get, "/hello");
        assert!(result.is_some());
        assert!(router.find(Method::Get, "/other").is_none());
    }

    #[test]
    fn test_param_route() {
        let mut router = Router::new();
        router.add(Method::Get, "/users/:id", dummy_handler());
        let (handler, params) = router.find(Method::Get, "/users/42").unwrap();
        assert_eq!(params.get("id").map(|s| s.as_str()), Some("42"));
        let _ = handler;
    }

    #[test]
    fn test_wildcard_route() {
        let mut router = Router::new();
        router.add(Method::Get, "/static/*path", dummy_handler());
        let (_, params) = router.find(Method::Get, "/static/css/main.css").unwrap();
        assert_eq!(params.get("path").map(|s| s.as_str()), Some("css/main.css"));
    }

    #[test]
    fn test_method_dispatch() {
        let mut router = Router::new();
        router.add(Method::Get, "/items", dummy_handler());
        router.add(Method::Post, "/items", dummy_handler());
        assert!(router.find(Method::Get, "/items").is_some());
        assert!(router.find(Method::Post, "/items").is_some());
        assert!(router.find(Method::Delete, "/items").is_none());
    }

    #[test]
    fn test_allowed_methods() {
        let mut router = Router::new();
        router.add(Method::Get, "/items", dummy_handler());
        router.add(Method::Post, "/items", dummy_handler());
        router.add(Method::Delete, "/items/:id", dummy_handler());
        let methods = router.allowed_methods("/items");
        assert!(methods.contains(&Method::Get));
        assert!(methods.contains(&Method::Post));
    }
}
