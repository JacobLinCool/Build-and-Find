use crate::{
    error::{NanowebError, Result},
    http::types::Method,
    middleware::{Handler, Middleware},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone)]
pub struct RouteTarget {
    pub handler: Arc<dyn Handler>,
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub pattern: String,
}

#[derive(Default)]
pub struct Router {
    root: Node,
}

#[derive(Default)]
struct Node {
    literal: HashMap<String, Node>,
    param: Option<Box<Node>>,
    param_name: Option<String>,
    wildcard: Option<Box<Node>>,
    wildcard_name: Option<String>,
    handlers: HashMap<Method, RouteTarget>,
}

pub struct RouteMatch {
    pub target: Option<RouteTarget>,
    pub allowed_methods: Vec<Method>,
    pub params: HashMap<String, String>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(
        &mut self,
        method: Method,
        path: &str,
        handler: Arc<dyn Handler>,
        middleware: Vec<Arc<dyn Middleware>>,
    ) -> Result<()> {
        let pattern = path.to_string();
        let segments = parse_pattern(path)?;
        let mut node = &mut self.root;
        for segment in segments.iter() {
            match segment {
                Segment::Literal(value) => {
                    node = node
                        .literal
                        .entry(value.clone())
                        .or_insert_with(Node::default);
                }
                Segment::Param(name) => {
                    let child = node.param.get_or_insert_with(|| Box::new(Node::default()));
                    child.param_name = Some(name.clone());
                    node = child.as_mut();
                }
                Segment::Wildcard(name) => {
                    let child = node
                        .wildcard
                        .get_or_insert_with(|| Box::new(Node::default()));
                    child.wildcard_name = Some(name.clone());
                    node = child.as_mut();
                }
            }
        }
        node.handlers.insert(
            method,
            RouteTarget {
                handler,
                middleware,
                pattern,
            },
        );
        Ok(())
    }

    pub fn resolve(&self, method: Method, path: &str) -> Option<RouteMatch> {
        let mut params = HashMap::new();
        if let Some((target, allowed_methods)) = resolve_node(&self.root, path, method, &mut params)
        {
            return Some(RouteMatch {
                target: Some(target),
                allowed_methods,
                params,
            });
        }
        None
    }
}

#[derive(Debug, Clone)]
enum Segment {
    Literal(String),
    Param(String),
    Wildcard(String),
}

fn parse_pattern(path: &str) -> Result<Vec<Segment>> {
    if !path.starts_with('/') {
        return Err(NanowebError::Config(
            "route paths must begin with /".to_string(),
        ));
    }
    let mut segments = Vec::new();
    let pieces = split_path(path);
    for (idx, piece) in pieces.iter().enumerate() {
        if let Some(name) = piece.strip_prefix(':') {
            segments.push(Segment::Param(name.to_string()));
        } else if let Some(name) = piece.strip_prefix('*') {
            if idx != pieces.len().saturating_sub(1) {
                return Err(NanowebError::Config(
                    "wildcard segments must be trailing".to_string(),
                ));
            }
            segments.push(Segment::Wildcard(name.to_string()));
        } else {
            segments.push(Segment::Literal(piece.to_string()));
        }
    }
    Ok(segments)
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn resolve_node(
    node: &Node,
    path: &str,
    method: Method,
    params: &mut HashMap<String, String>,
) -> Option<(RouteTarget, Vec<Method>)> {
    let mut current = node;
    let mut remaining = path;
    loop {
        let trimmed = remaining.trim_start_matches('/');
        if trimmed.is_empty() {
            break;
        }
        let (segment, next_remaining) = next_segment(trimmed);
        if let Some(child) = current.literal.get(segment) {
            current = child;
            remaining = next_remaining;
            continue;
        }
        if let Some(child) = current.param.as_deref() {
            if let Some(name) = &child.param_name {
                params.insert(name.clone(), segment.to_string());
            }
            current = child;
            remaining = next_remaining;
            continue;
        }
        if let Some(child) = current.wildcard.as_deref() {
            if let Some(name) = &child.wildcard_name {
                params.insert(name.clone(), trimmed.to_string());
            }
            current = child;
            break;
        }
        return None;
    }
    let allowed_methods = allowed_methods(current);
    if method == Method::OPTIONS {
        let target = select_target(current, Method::OPTIONS)
            .or_else(|| select_target(current, Method::GET))
            .or_else(|| current.handlers.values().next().cloned())?;
        return Some((target, allowed_methods));
    }
    let target = select_target(current, method).or_else(|| {
        if method == Method::HEAD {
            select_target(current, Method::GET)
        } else {
            None
        }
    })?;
    Some((target, allowed_methods))
}

fn next_segment(path: &str) -> (&str, &str) {
    if let Some((segment, rest)) = path.split_once('/') {
        (segment, rest)
    } else {
        (path, "")
    }
}

fn select_target(node: &Node, method: Method) -> Option<RouteTarget> {
    node.handlers.get(&method).cloned()
}

fn allowed_methods(node: &Node) -> Vec<Method> {
    let mut set = HashSet::new();
    for method in node.handlers.keys() {
        set.insert(*method);
    }
    if set.contains(&Method::GET) {
        set.insert(Method::HEAD);
    }
    set.insert(Method::OPTIONS);
    let mut methods: Vec<_> = set.into_iter().collect();
    methods.sort_by_key(|method| method.as_str());
    methods
}

#[cfg(test)]
mod tests {
    use super::Router;
    use crate::{
        app::AppState,
        error::Result,
        http::types::{Method, Request, Response, StatusCode},
        middleware::Handler,
    };
    use async_trait::async_trait;
    use std::sync::Arc;

    struct TestHandler;

    #[async_trait]
    impl Handler for TestHandler {
        async fn handle(&self, _req: Request, _ctx: Arc<AppState>) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "ok"))
        }
    }

    #[test]
    fn route_params() {
        let mut router = Router::new();
        router
            .add(Method::GET, "/users/:id", Arc::new(TestHandler), Vec::new())
            .unwrap();
        let route = router.resolve(Method::GET, "/users/42").unwrap();
        assert!(route.target.is_some());
    }
}
