use std::collections::HashMap;
use std::sync::Arc;

use crate::http::Method;
use crate::middleware::Middleware;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Literal(String),
    Param(String),
    Wildcard(String),
}

#[derive(Clone)]
pub struct RouteEntry {
    pub segments: Vec<Segment>,
    pub methods: Vec<Method>,
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub name: String,
}

#[derive(Debug, Default, Clone)]
pub struct TrieNode {
    children: HashMap<String, TrieNode>,
    param_child: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard_child: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    route_index: Option<usize>,
}

pub struct Router {
    trie: TrieNode,
    routes: Vec<RouteEntry>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            trie: TrieNode::default(),
            routes: Vec::new(),
        }
    }

    pub fn add_route(
        &mut self,
        path: &str,
        methods: Vec<Method>,
        middleware: Vec<Arc<dyn Middleware>>,
        name: &str,
    ) {
        let segments = parse_segments(path);
        let route_index = self.routes.len();
        self.routes.push(RouteEntry {
            segments,
            methods,
            middleware,
            name: name.to_string(),
        });

        let segments = self.routes[route_index].segments.clone();
        let mut current = &mut self.trie;
        for segment in &segments {
            match segment {
                Segment::Literal(lit) => {
                    current = current
                        .children
                        .entry(lit.clone())
                        .or_insert_with(TrieNode::default);
                }
                Segment::Param(name) => {
                    if current.param_child.is_none() {
                        current.param_child = Some(Box::new(TrieNode::default()));
                    }
                    current.param_name = Some(name.clone());
                    current = current.param_child.as_mut().unwrap();
                }
                Segment::Wildcard(name) => {
                    if current.wildcard_child.is_none() {
                        current.wildcard_child = Some(Box::new(TrieNode::default()));
                    }
                    current.wildcard_name = Some(name.clone());
                    current = current.wildcard_child.as_mut().unwrap();
                }
            }
        }
        current.route_index = Some(route_index);
    }

    pub fn lookup(
        &self,
        method: &Method,
        path: &str,
    ) -> Option<(usize, HashMap<String, String>, Vec<Arc<dyn Middleware>>)> {
        let segments: Vec<&str> = path
            .trim_start_matches('/')
            .trim_end_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        let mut params = HashMap::new();
        let mut current = &self.trie;
        let mut seg_idx = 0;

        while seg_idx < segments.len() {
            let seg = segments[seg_idx];
            if let Some(child) = current.children.get(seg) {
                current = child;
                seg_idx += 1;
            } else if let Some(ref param_child) = current.param_child {
                if let Some(ref name) = current.param_name {
                    params.insert(name.clone(), seg.to_string());
                }
                current = param_child;
                seg_idx += 1;
            } else if let Some(ref wildcard_child) = current.wildcard_child {
                if let Some(ref name) = current.wildcard_name {
                    let rest: Vec<&str> = segments[seg_idx..].to_vec();
                    params.insert(name.clone(), rest.join("/"));
                }
                current = wildcard_child;
                // Wildcard consumed all remaining segments
                seg_idx = segments.len();
            } else {
                return None;
            }
        }

        let route_idx = current.route_index?;
        let entry = &self.routes[route_idx];
        if !entry.methods.is_empty() && !entry.methods.contains(method) {
            return None;
        }

        Some((route_idx, params, entry.middleware.clone()))
    }

    pub fn route_name(&self, idx: usize) -> &str {
        &self.routes[idx].name
    }

    pub fn route_methods(&self, idx: usize) -> &[Method] {
        &self.routes[idx].methods
    }

    pub fn all_routes(&self) -> Vec<(&str, &[Method])> {
        self.routes
            .iter()
            .map(|r| (r.name.as_str(), r.methods.as_slice()))
            .collect()
    }
}

fn parse_segments(path: &str) -> Vec<Segment> {
    path.trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with(':') {
                Segment::Param(s[1..].to_string())
            } else if s.starts_with('*') {
                Segment::Wildcard(s[1..].to_string())
            } else {
                Segment::Literal(s.to_string())
            }
        })
        .collect()
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_route() {
        let mut router = Router::new();
        router.add_route("/users", vec![Method::Get], vec![], "get_users");
        let result = router.lookup(&Method::Get, "/users");
        assert!(result.is_some());
        assert_eq!(router.route_name(result.unwrap().0), "get_users");
    }

    #[test]
    fn param_route() {
        let mut router = Router::new();
        router.add_route("/users/:id", vec![Method::Get], vec![], "get_user");
        let result = router.lookup(&Method::Get, "/users/42");
        assert!(result.is_some());
        let (idx, params, _) = result.unwrap();
        assert_eq!(router.route_name(idx), "get_user");
        assert_eq!(params.get("id"), Some(&"42".to_string()));
    }

    #[test]
    fn wildcard_route() {
        let mut router = Router::new();
        router.add_route("/static/*path", vec![Method::Get], vec![], "static");
        let result = router.lookup(&Method::Get, "/static/css/style.css");
        assert!(result.is_some());
        let (_, params, _) = result.unwrap();
        assert_eq!(params.get("path"), Some(&"css/style.css".to_string()));
    }

    #[test]
    fn method_not_allowed() {
        let mut router = Router::new();
        router.add_route("/users", vec![Method::Get], vec![], "get_users");
        let result = router.lookup(&Method::Post, "/users");
        assert!(result.is_none());
    }

    #[test]
    fn not_found() {
        let mut router = Router::new();
        router.add_route("/users", vec![Method::Get], vec![], "get_users");
        let result = router.lookup(&Method::Get, "/nonexistent");
        assert!(result.is_none());
    }
}
