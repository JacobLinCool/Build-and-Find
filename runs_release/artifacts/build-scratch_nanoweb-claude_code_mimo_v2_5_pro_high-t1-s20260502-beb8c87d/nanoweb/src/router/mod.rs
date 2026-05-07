use crate::http::types::{Method, Request, Response};
use crate::middleware::Middleware;
use std::collections::HashMap;

pub type Handler = Box<
    dyn Fn(Request) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
        + Send
        + Sync,
>;

pub struct RouteEntry {
    pub handler: Handler,
    pub middleware: Vec<Box<dyn Middleware>>,
}

struct TrieNode {
    children: HashMap<String, TrieNode>,
    param_child: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard_child: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    routes: HashMap<Method, RouteEntry>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            param_child: None,
            param_name: None,
            wildcard_child: None,
            wildcard_name: None,
            routes: HashMap::new(),
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

    pub fn add_route(&mut self, method: Method, path: &str, entry: RouteEntry) {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut node = &mut self.root;

        for seg in segments {
            if seg.starts_with(':') {
                let name = seg[1..].to_string();
                if node.param_child.is_none() {
                    node.param_child = Some(Box::new(TrieNode::new()));
                    node.param_name = Some(name.clone());
                }
                node = node.param_child.as_mut().unwrap();
            } else if seg.starts_with('*') {
                let name = seg[1..].to_string();
                if node.wildcard_child.is_none() {
                    node.wildcard_child = Some(Box::new(TrieNode::new()));
                    node.wildcard_name = Some(name.clone());
                }
                node = node.wildcard_child.as_mut().unwrap();
            } else {
                node = node
                    .children
                    .entry(seg.to_string())
                    .or_insert_with(TrieNode::new);
            }
        }

        node.routes.insert(method, entry);
    }

    pub fn route(
        &self,
        method: Method,
        path: &str,
    ) -> Option<(&RouteEntry, HashMap<String, String>)> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        self.lookup(&self.root, &segments, 0, &mut params)
            .and_then(|node| {
                // Try exact method first
                if let Some(entry) = node.routes.get(&method) {
                    return Some((entry, params));
                }
                // HEAD falls back to GET
                if method == Method::Head {
                    if let Some(entry) = node.routes.get(&Method::Get) {
                        return Some((entry, params));
                    }
                }
                None
            })
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<Method> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        if let Some(node) = self.lookup(&self.root, &segments, 0, &mut params) {
            node.routes.keys().copied().collect()
        } else {
            Vec::new()
        }
    }

    fn lookup<'a>(
        &'a self,
        node: &'a TrieNode,
        segments: &[&str],
        idx: usize,
        params: &mut HashMap<String, String>,
    ) -> Option<&'a TrieNode> {
        if idx == segments.len() {
            return Some(node);
        }

        let seg = segments[idx];

        // Literal match
        if let Some(child) = node.children.get(seg) {
            if let Some(result) = self.lookup(child, segments, idx + 1, params) {
                return Some(result);
            }
        }

        // Parameter match
        if let Some(ref child) = node.param_child {
            if let Some(ref name) = node.param_name {
                let old = params.remove(name);
                params.insert(name.clone(), percent_decode(seg));
                if let Some(result) = self.lookup(child, segments, idx + 1, params) {
                    return Some(result);
                }
                params.remove(name);
                if let Some(v) = old {
                    params.insert(name.clone(), v);
                }
            }
        }

        // Wildcard match - consumes all remaining segments
        if let Some(ref child) = node.wildcard_child {
            if let Some(ref name) = node.wildcard_name {
                let remaining: Vec<&str> = segments[idx..].to_vec();
                let wildcard_value = remaining.join("/");
                params.insert(name.clone(), percent_decode(&wildcard_value));
                return Some(child.as_ref());
            }
        }

        None
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                result.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(result).unwrap_or_default()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler() -> Handler {
        Box::new(|_req| Box::pin(async { Response::ok() }))
    }

    #[test]
    fn test_literal_route() {
        let mut router = Router::new();
        router.add_route(
            Method::Get,
            "/hello",
            RouteEntry {
                handler: make_handler(),
                middleware: vec![],
            },
        );
        assert!(router.route(Method::Get, "/hello").is_some());
        assert!(router.route(Method::Get, "/other").is_none());
    }

    #[test]
    fn test_param_route() {
        let mut router = Router::new();
        router.add_route(
            Method::Get,
            "/users/:id",
            RouteEntry {
                handler: make_handler(),
                middleware: vec![],
            },
        );
        let (_, params) = router.route(Method::Get, "/users/42").unwrap();
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_wildcard_route() {
        let mut router = Router::new();
        router.add_route(
            Method::Get,
            "/static/*path",
            RouteEntry {
                handler: make_handler(),
                middleware: vec![],
            },
        );
        let (_, params) = router.route(Method::Get, "/static/css/style.css").unwrap();
        assert_eq!(params.get("path").unwrap(), "css/style.css");
    }

    #[test]
    fn test_head_fallback_to_get() {
        let mut router = Router::new();
        router.add_route(
            Method::Get,
            "/page",
            RouteEntry {
                handler: make_handler(),
                middleware: vec![],
            },
        );
        assert!(router.route(Method::Head, "/page").is_some());
    }

    #[test]
    fn test_allowed_methods() {
        let mut router = Router::new();
        router.add_route(
            Method::Get,
            "/resource",
            RouteEntry {
                handler: make_handler(),
                middleware: vec![],
            },
        );
        router.add_route(
            Method::Post,
            "/resource",
            RouteEntry {
                handler: make_handler(),
                middleware: vec![],
            },
        );
        let methods = router.allowed_methods("/resource");
        assert!(methods.contains(&Method::Get));
        assert!(methods.contains(&Method::Post));
    }
}
