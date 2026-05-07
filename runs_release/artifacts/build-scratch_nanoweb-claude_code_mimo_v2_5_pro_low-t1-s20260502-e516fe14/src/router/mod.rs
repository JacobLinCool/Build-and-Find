use crate::http::{Method, Request, Response};
use std::collections::HashMap;

/// A boxed async handler function.
pub type Handler = Box<dyn Fn(Request) -> HandlerFuture + Send + Sync>;
pub type HandlerFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>;

/// A trie node for route matching.
#[derive(Default)]
struct TrieNode {
    /// Literal children: segment -> child node.
    children: HashMap<String, TrieNode>,
    /// Parameter child (e.g., :id).
    param_child: Option<(String, Box<TrieNode>)>,
    /// Wildcard child (e.g., *path).
    wildcard_child: Option<(String, Box<TrieNode>)>,
    /// Handlers by method.
    handlers: HashMap<Method, Handler>,
}

/// The router with trie-based path matching.
pub struct Router {
    root: TrieNode,
}

impl Router {
    pub fn new() -> Self {
        Self {
            root: TrieNode::default(),
        }
    }

    /// Register a handler for a method and path.
    pub fn route<F, Fut>(&mut self, method: Method, path: &str, handler: F)
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Response> + Send + 'static,
    {
        let handler: Handler = Box::new(move |req| Box::pin(handler(req)));
        self.add_route(method, path, handler);
    }

    fn add_route(&mut self, method: Method, path: &str, handler: Handler) {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut node = &mut self.root;

        for seg in segments {
            if seg.starts_with('*') {
                // Wildcard segment
                let name = seg[1..].to_string();
                if node.wildcard_child.is_none() {
                    node.wildcard_child = Some((name, Box::new(TrieNode::default())));
                }
                node = node.wildcard_child.as_mut().unwrap().1.as_mut();
                break; // Wildcard consumes the rest
            } else if seg.starts_with(':') {
                // Parameter segment
                let name = seg[1..].to_string();
                if node.param_child.is_none() {
                    node.param_child = Some((name, Box::new(TrieNode::default())));
                } else {
                    // Update name if different
                    node.param_child.as_mut().unwrap().0 = name;
                }
                node = node.param_child.as_mut().unwrap().1.as_mut();
            } else {
                // Literal segment
                node = node
                    .children
                    .entry(seg.to_string())
                    .or_insert_with(TrieNode::default);
            }
        }

        node.handlers.insert(method, handler);
    }

    /// Look up a route and return the handler + extracted parameters.
    pub fn find(&self, method: Method, path: &str) -> Option<(&Handler, HashMap<String, String>)> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();

        if let Some(handler) = self.find_recursive(&self.root, &segments, 0, method, &mut params) {
            Some((handler, params))
        } else {
            None
        }
    }

    fn find_recursive<'a>(
        &'a self,
        node: &'a TrieNode,
        segments: &[&str],
        idx: usize,
        method: Method,
        params: &mut HashMap<String, String>,
    ) -> Option<&'a Handler> {
        if idx == segments.len() {
            return node.handlers.get(&method);
        }

        let seg = segments[idx];

        // Try literal match first
        if let Some(child) = node.children.get(seg) {
            if let Some(handler) = self.find_recursive(child, segments, idx + 1, method, params) {
                return Some(handler);
            }
        }

        // Try parameter match
        if let Some((ref name, ref child)) = node.param_child {
            let old = params.remove(name);
            params.insert(name.clone(), seg.to_string());
            if let Some(handler) = self.find_recursive(child, segments, idx + 1, method, params) {
                return Some(handler);
            }
            params.remove(name);
            if let Some(v) = old {
                params.insert(name.clone(), v);
            }
        }

        // Try wildcard match
        if let Some((ref name, ref child)) = node.wildcard_child {
            let rest = segments[idx..].join("/");
            params.insert(name.clone(), rest);
            if let Some(handler) = child.handlers.get(&method) {
                return Some(handler);
            }
            params.remove(name);
        }

        None
    }

    /// Get all registered methods for a given path (for OPTIONS and 405).
    pub fn allowed_methods(&self, path: &str) -> Vec<Method> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut methods = Vec::new();
        self.collect_methods(&self.root, &segments, 0, &mut methods);
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

        if let Some((_, ref child)) = node.param_child {
            self.collect_methods(child, segments, idx + 1, methods);
        }

        if let Some((_, ref child)) = node.wildcard_child {
            methods.extend(child.handlers.keys().copied());
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
    use crate::http::{Response, StatusCode};

    fn ok_response(
        _req: Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
        Box::pin(async { Response::ok() })
    }

    #[tokio::test]
    async fn test_literal_route() {
        let mut router = Router::new();
        router.route(Method::Get, "/hello", ok_response);

        let (handler, params) = router.find(Method::Get, "/hello").unwrap();
        assert!(params.is_empty());
        let resp = handler(Request {
            method: Method::Get,
            target: "/hello".into(),
            path: "/hello".into(),
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        })
        .await;
        assert_eq!(resp.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_parameter_route() {
        let mut router = Router::new();
        router.route(Method::Get, "/users/:id", ok_response);

        let (_, params) = router.find(Method::Get, "/users/42").unwrap();
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[tokio::test]
    async fn test_wildcard_route() {
        let mut router = Router::new();
        router.route(Method::Get, "/static/*path", ok_response);

        let (_, params) = router.find(Method::Get, "/static/css/style.css").unwrap();
        assert_eq!(params.get("path").unwrap(), "css/style.css");
    }

    #[tokio::test]
    async fn test_method_dispatch() {
        let mut router = Router::new();
        router.route(Method::Get, "/items", ok_response);
        router.route(Method::Post, "/items", ok_response);

        assert!(router.find(Method::Get, "/items").is_some());
        assert!(router.find(Method::Post, "/items").is_some());
        assert!(router.find(Method::Delete, "/items").is_none());
    }

    #[test]
    fn test_allowed_methods() {
        let mut router = Router::new();
        router.route(Method::Get, "/items", ok_response);
        router.route(Method::Post, "/items", ok_response);
        router.route(Method::Put, "/items", ok_response);

        let methods = router.allowed_methods("/items");
        assert!(methods.contains(&Method::Get));
        assert!(methods.contains(&Method::Post));
        assert!(methods.contains(&Method::Put));
    }

    #[tokio::test]
    async fn test_no_match() {
        let router = Router::new();
        assert!(router.find(Method::Get, "/nothing").is_none());
    }

    #[tokio::test]
    async fn test_multiple_params() {
        let mut router = Router::new();
        router.route(Method::Get, "/users/:uid/posts/:pid", ok_response);

        let (_, params) = router.find(Method::Get, "/users/1/posts/99").unwrap();
        assert_eq!(params.get("uid").unwrap(), "1");
        assert_eq!(params.get("pid").unwrap(), "99");
    }
}
