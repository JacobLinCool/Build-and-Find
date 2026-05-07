use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{NanowebError, Result};
use crate::http::{Method, Request, Response};

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, request: Request) -> Result<Response>;
}

#[async_trait]
impl<F, Fut> Handler for F
where
    F: Send + Sync + Fn(Request) -> Fut,
    Fut: std::future::Future<Output = Result<Response>> + Send,
{
    async fn handle(&self, request: Request) -> Result<Response> {
        (self)(request).await
    }
}

#[derive(Clone)]
pub struct RouteHandle {
    pub pattern: String,
    pub handlers: HashMap<Method, Arc<dyn Handler>>,
}

pub struct RouteMatch<'a> {
    pub pattern: &'a str,
    pub handlers: &'a HashMap<Method, Arc<dyn Handler>>,
    pub params: HashMap<String, String>,
}

#[derive(Default)]
pub struct Router {
    root: TrieNode,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_route(
        &mut self,
        method: Method,
        pattern: impl Into<String>,
        handler: Arc<dyn Handler>,
    ) -> Result<()> {
        let pattern = pattern.into();
        self.root.insert(&pattern, method, handler)
    }

    pub fn lookup<'a>(&'a self, path: &'a str) -> Option<RouteMatch<'a>> {
        let mut params = HashMap::new();
        let normalized = path.trim_matches('/');
        self.root.lookup(normalized, 0, &mut params)
    }
}

#[derive(Default)]
struct TrieNode {
    literals: HashMap<String, TrieNode>,
    param: Option<Box<TrieNode>>,
    param_name: Option<String>,
    wildcard: Option<Box<TrieNode>>,
    wildcard_name: Option<String>,
    handlers: HashMap<Method, Arc<dyn Handler>>,
    pattern: Option<String>,
}

impl TrieNode {
    fn insert(&mut self, pattern: &str, method: Method, handler: Arc<dyn Handler>) -> Result<()> {
        let normalized = normalize_path(pattern);
        if normalized.is_empty() {
            self.handlers.insert(method, handler);
            self.pattern = Some(pattern.to_string());
            return Ok(());
        }

        let mut node = self;
        let mut start = 0;
        while let Some((segment, next_start)) = next_segment(&normalized, start) {
            let is_last = next_start >= normalized.len();
            if let Some(name) = segment.strip_prefix(':') {
                let child = node.param.get_or_insert_with(|| {
                    let mut child = TrieNode::default();
                    child.param_name = Some(name.to_string());
                    Box::new(child)
                });
                if child.param_name.as_deref() != Some(name) {
                    return Err(NanowebError::Router(format!(
                        "conflicting parameter names for pattern {pattern}"
                    )));
                }
                node = child.as_mut();
            } else if let Some(name) = segment.strip_prefix('*') {
                if !is_last {
                    return Err(NanowebError::Router(format!(
                        "wildcard must be the final segment in pattern {pattern}"
                    )));
                }
                let child = node.wildcard.get_or_insert_with(|| {
                    let mut child = TrieNode::default();
                    child.wildcard_name = Some(name.to_string());
                    Box::new(child)
                });
                if child.wildcard_name.as_deref() != Some(name) {
                    return Err(NanowebError::Router(format!(
                        "conflicting wildcard names for pattern {pattern}"
                    )));
                }
                node = child.as_mut();
            } else {
                node = node
                    .literals
                    .entry(segment.to_string())
                    .or_insert_with(TrieNode::default);
            }
            start = next_start;
        }

        node.handlers.insert(method, handler);
        node.pattern = Some(pattern.to_string());
        Ok(())
    }

    fn lookup<'a>(
        &'a self,
        path: &'a str,
        start: usize,
        params: &mut HashMap<String, String>,
    ) -> Option<RouteMatch<'a>> {
        if start >= path.len() {
            return self.pattern_match(params.clone());
        }

        let (segment, next_start) = next_segment(path, start)?;

        if let Some(literal) = self.literals.get(segment) {
            if let Some(found) = literal.lookup(path, next_start, params) {
                return Some(found);
            }
        }

        if let Some(param) = self.param.as_ref() {
            if let Some(name) = param.param_name.as_ref() {
                params.insert(name.clone(), segment.to_string());
            }
            if let Some(found) = param.lookup(path, next_start, params) {
                return Some(found);
            }
            if let Some(name) = param.param_name.as_ref() {
                params.remove(name);
            }
        }

        if let Some(wildcard) = self.wildcard.as_ref() {
            if let Some(name) = wildcard.wildcard_name.as_ref() {
                params.insert(name.clone(), path[start..].to_string());
            }
            return wildcard.pattern_match(params.clone());
        }

        None
    }

    fn pattern_match<'a>(&'a self, params: HashMap<String, String>) -> Option<RouteMatch<'a>> {
        if self.handlers.is_empty() {
            return None;
        }
        Some(RouteMatch {
            pattern: self.pattern.as_deref().unwrap_or(""),
            handlers: &self.handlers,
            params,
        })
    }
}

fn normalize_path(path: &str) -> String {
    if path == "/" {
        String::new()
    } else {
        path.trim_matches('/').to_string()
    }
}

fn next_segment(path: &str, start: usize) -> Option<(&str, usize)> {
    if start >= path.len() {
        return None;
    }
    let bytes = path.as_bytes();
    let mut end = start;
    while end < path.len() && bytes[end] != b'/' {
        end += 1;
    }
    let next_start = if end < path.len() { end + 1 } else { end };
    Some((&path[start..end], next_start))
}

pub fn allowed_methods(handlers: &HashMap<Method, Arc<dyn Handler>>) -> Vec<Method> {
    let mut methods: Vec<Method> = handlers.keys().cloned().collect();
    if methods.contains(&Method::Get) && !methods.contains(&Method::Head) {
        methods.push(Method::Head);
    }
    methods.push(Method::Options);
    methods.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    methods.dedup();
    methods
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Response, StatusCode};

    async fn ok(_: Request) -> Result<Response> {
        Ok(Response::text(StatusCode::Ok, "ok"))
    }

    #[tokio::test]
    async fn matches_parameters() {
        let mut router = Router::new();
        router
            .add_route(Method::Get, "/users/:id", Arc::new(ok))
            .unwrap();
        let route = router.lookup("/users/42").unwrap();
        assert_eq!(route.params.get("id"), Some(&"42".to_string()));
    }

    #[tokio::test]
    async fn matches_wildcards() {
        let mut router = Router::new();
        router
            .add_route(Method::Get, "/static/*path", Arc::new(ok))
            .unwrap();
        let route = router.lookup("/static/css/app.css").unwrap();
        assert_eq!(route.params.get("path"), Some(&"css/app.css".to_string()));
    }
}
