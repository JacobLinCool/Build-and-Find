use std::collections::HashMap;
use std::sync::Arc;

use crate::http::types::{Method, Request, Response, StatusCode};

type HandlerFn = Arc<dyn Fn(Request) -> Response + Send + Sync>;

struct TrieNode {
    children: HashMap<String, TrieNode>,
    param_child: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard_child: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    handlers: HashMap<Method, HandlerFn>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            param_child: None,
            param_name: None,
            wildcard_child: None,
            wildcard_name: None,
            handlers: HashMap::new(),
        }
    }
}

impl Default for TrieNode {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Router {
    root: TrieNode,
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

impl Router {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
        }
    }

    pub fn route(
        &mut self,
        method: Method,
        path: &str,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) {
        let segments = parse_path(path);
        let mut node = &mut self.root;

        for seg in &segments {
            match seg.as_bytes() {
                [b':', rest @ ..] => {
                    let name = std::str::from_utf8(rest).unwrap_or("").to_string();
                    if node.param_child.is_none() {
                        node.param_child = Some(Box::new(TrieNode::new()));
                        node.param_name = Some(name.clone());
                    }
                    node = node.param_child.as_mut().unwrap();
                }
                [b'*', rest @ ..] => {
                    let name = std::str::from_utf8(rest).unwrap_or("").to_string();
                    if node.wildcard_child.is_none() {
                        node.wildcard_child = Some(Box::new(TrieNode::new()));
                        node.wildcard_name = Some(name.clone());
                    }
                    node = node.wildcard_child.as_mut().unwrap();
                    break;
                }
                _ => {
                    node.children
                        .entry(seg.clone())
                        .or_insert_with(TrieNode::new);
                    node = node.children.get_mut(seg).unwrap();
                }
            }
        }

        node.handlers.insert(method, Arc::new(handler));
    }

    pub fn lookup(&self, method: Method, path: &str) -> LookupResult {
        let segments = parse_path(path);
        self.lookup_segments(method, &segments, 0, &self.root)
    }

    fn lookup_segments(
        &self,
        method: Method,
        segments: &[String],
        depth: usize,
        node: &TrieNode,
    ) -> LookupResult {
        if depth == segments.len() {
            return self.check_handlers(method, node);
        }

        let seg = &segments[depth];

        // Try literal match first
        if let Some(child) = node.children.get(seg) {
            let result = self.lookup_segments(method, segments, depth + 1, child);
            if !matches!(result, LookupResult::NotFound) {
                return result;
            }
        }

        // Try parameter match
        if let (Some(ref param_child), Some(ref param_name)) = (&node.param_child, &node.param_name)
        {
            let result = self.lookup_segments(method, segments, depth + 1, param_child);
            if let LookupResult::Match {
                mut params,
                handler,
                allowed_methods,
            } = result
            {
                params.insert(param_name.clone(), seg.clone());
                return LookupResult::Match {
                    params,
                    handler,
                    allowed_methods,
                };
            }
        }

        // Try wildcard match
        if let (Some(ref wc_child), Some(ref wc_name)) = (&node.wildcard_child, &node.wildcard_name)
        {
            let remaining: String = segments[depth..].join("/");
            let mut params = HashMap::new();
            params.insert(wc_name.clone(), remaining);

            if let Some(handler) = wc_child.handlers.get(&method) {
                return LookupResult::Match {
                    params,
                    handler: handler.clone(),
                    allowed_methods: self.collect_methods(wc_child),
                };
            }
            if method == Method::Head {
                if let Some(handler) = wc_child.handlers.get(&Method::Get) {
                    return LookupResult::Match {
                        params,
                        handler: handler.clone(),
                        allowed_methods: self.collect_methods(wc_child),
                    };
                }
            }
        }

        LookupResult::NotFound
    }

    fn check_handlers(&self, method: Method, node: &TrieNode) -> LookupResult {
        if let Some(handler) = node.handlers.get(&method) {
            return LookupResult::Match {
                params: HashMap::new(),
                handler: handler.clone(),
                allowed_methods: self.collect_methods(node),
            };
        }

        if method == Method::Head {
            if let Some(handler) = node.handlers.get(&Method::Get) {
                return LookupResult::Match {
                    params: HashMap::new(),
                    handler: handler.clone(),
                    allowed_methods: self.collect_methods(node),
                };
            }
        }

        if node.handlers.is_empty() {
            LookupResult::NotFound
        } else {
            LookupResult::MethodNotAllowed {
                allowed_methods: self.collect_methods(node),
            }
        }
    }

    fn collect_methods(&self, node: &TrieNode) -> Vec<String> {
        node.handlers
            .keys()
            .map(|m| m.as_str().to_string())
            .collect()
    }

    pub fn handle(&self, mut req: Request) -> Response {
        let path = req.path.clone();
        let method = req.method;

        match self.lookup(method, &path) {
            LookupResult::Match {
                params, handler, ..
            } => {
                req.params = params;
                handler(req)
            }
            LookupResult::MethodNotAllowed { allowed_methods } => {
                let mut resp = Response::new(StatusCode::METHOD_NOT_ALLOWED);
                resp.headers.insert("allow", allowed_methods.join(", "));
                resp.body = b"Method Not Allowed".to_vec();
                resp.headers
                    .insert("content-type", "text/plain; charset=utf-8");
                resp
            }
            LookupResult::NotFound => {
                let mut resp = Response::new(StatusCode::NOT_FOUND);
                resp.body = b"Not Found".to_vec();
                resp.headers
                    .insert("content-type", "text/plain; charset=utf-8");
                resp
            }
        }
    }

    pub fn handle_options(&self, path: &str) -> Response {
        let segments = parse_path(path);
        match self.lookup_segments_for_options(&segments, 0, &self.root) {
            Some(methods) => {
                let mut resp = Response::ok();
                let mut all = methods;
                if !all.contains(&"OPTIONS".to_string()) {
                    all.push("OPTIONS".to_string());
                }
                if !all.contains(&"HEAD".to_string()) && all.contains(&"GET".to_string()) {
                    all.push("HEAD".to_string());
                }
                resp.headers.insert("allow", all.join(", "));
                resp.body = Vec::new();
                resp
            }
            None => Response::not_found(),
        }
    }

    fn lookup_segments_for_options(
        &self,
        segments: &[String],
        depth: usize,
        node: &TrieNode,
    ) -> Option<Vec<String>> {
        if depth == segments.len() {
            let methods: Vec<String> = node
                .handlers
                .keys()
                .map(|m| m.as_str().to_string())
                .collect();
            if methods.is_empty() {
                return None;
            }
            return Some(methods);
        }

        let seg = &segments[depth];

        if let Some(child) = node.children.get(seg) {
            if let Some(methods) = self.lookup_segments_for_options(segments, depth + 1, child) {
                return Some(methods);
            }
        }

        if let Some(ref param_child) = node.param_child {
            if let Some(methods) =
                self.lookup_segments_for_options(segments, depth + 1, param_child)
            {
                return Some(methods);
            }
        }

        None
    }
}

pub enum LookupResult {
    Match {
        params: HashMap<String, String>,
        handler: HandlerFn,
        allowed_methods: Vec<String>,
    },
    MethodNotAllowed {
        allowed_methods: Vec<String>,
    },
    NotFound,
}

fn parse_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_handler(_req: Request) -> Response {
        Response::text("hello")
    }

    fn user_handler(req: Request) -> Response {
        let id = req.params.get("id").map(|s| s.as_str()).unwrap_or("?");
        Response::text(format!("user {id}"))
    }

    #[test]
    fn basic_routing() {
        let mut router = Router::new();
        router.route(Method::Get, "/hello", hello_handler);
        let resp = router.handle(Request {
            method: Method::Get,
            path: "/hello".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn path_params() {
        let mut router = Router::new();
        router.route(Method::Get, "/users/:id", user_handler);
        let resp = router.handle(Request {
            method: Method::Get,
            path: "/users/42".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.body, b"user 42");
    }

    #[test]
    fn not_found() {
        let router = Router::new();
        let resp = router.handle(Request {
            method: Method::Get,
            path: "/missing".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn head_fallback_to_get() {
        let mut router = Router::new();
        router.route(Method::Get, "/hello", hello_handler);
        let resp = router.handle(Request {
            method: Method::Head,
            path: "/hello".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[test]
    fn options_lists_methods() {
        let mut router = Router::new();
        router.route(Method::Get, "/hello", hello_handler);
        router.route(Method::Post, "/hello", hello_handler);
        let resp = router.handle_options("/hello");
        assert_eq!(resp.status, StatusCode::OK);
        let allow = resp.headers.get("allow").unwrap();
        assert!(allow.contains("GET"));
        assert!(allow.contains("POST"));
        assert!(allow.contains("OPTIONS"));
    }

    #[test]
    fn method_not_allowed() {
        let mut router = Router::new();
        router.route(Method::Get, "/hello", hello_handler);
        let resp = router.handle(Request {
            method: Method::Post,
            path: "/hello".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn wildcard_routing() {
        let mut router = Router::new();
        router.route(Method::Get, "/static/*path", |_req| {
            Response::text("static file")
        });
        let resp = router.handle(Request {
            method: Method::Get,
            path: "/static/css/style.css".into(),
            ..Default::default()
        });
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.body, b"static file");
    }
}
