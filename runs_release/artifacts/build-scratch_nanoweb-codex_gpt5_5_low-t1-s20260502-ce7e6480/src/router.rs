use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type BoxFutureResponse = Pin<Box<dyn Future<Output = Result<Response>> + Send>>;
pub type Handler = Arc<dyn Fn(Request) -> BoxFutureResponse + Send + Sync>;

#[derive(Default)]
pub struct Router {
    root: Node,
}

#[derive(Default)]
struct Node {
    literal: HashMap<String, Node>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<(String, HashMap<Method, Handler>)>,
    handlers: HashMap<Method, Handler>,
}

pub struct Match {
    pub handler: Handler,
    pub params: HashMap<String, String>,
    pub route: String,
    pub suppress_body: bool,
}

#[async_trait]
pub trait IntoHandler: Send + Sync + 'static {
    async fn call(&self, req: Request) -> Result<Response>;
}

#[async_trait]
impl<F, Fut> IntoHandler for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response>> + Send + 'static,
{
    async fn call(&self, req: Request) -> Result<Response> {
        self(req).await
    }
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<H, Fut>(&mut self, method: Method, path: &str, handler: H)
    where
        H: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Response>> + Send + 'static,
    {
        let handler: Handler = Arc::new(move |req| Box::pin(handler(req)));
        let mut node = &mut self.root;
        for segment in segments(path) {
            if let Some(name) = segment.strip_prefix('*') {
                node.wildcard
                    .get_or_insert_with(|| (name.to_string(), HashMap::new()))
                    .1
                    .insert(method, handler);
                return;
            }
            if let Some(name) = segment.strip_prefix(':') {
                node = node
                    .param
                    .get_or_insert_with(|| (name.to_string(), Box::default()))
                    .1
                    .as_mut();
            } else {
                node = node.literal.entry(segment.to_string()).or_default();
            }
        }
        node.handlers.insert(method, handler);
    }

    pub fn find(&self, method: &Method, path: &str) -> Option<Match> {
        let parts = segments(path);
        let mut params = HashMap::new();
        let (node, route) = find_node(&self.root, &parts, 0, &mut params, String::new())?;
        let mut suppress_body = false;
        let handler = node.handlers.get(method).cloned().or_else(|| {
            if matches!(method, Method::Head) {
                suppress_body = true;
                node.handlers.get(&Method::Get).cloned()
            } else {
                None
            }
        })?;
        Some(Match {
            handler,
            params,
            route,
            suppress_body,
        })
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<String> {
        let mut params = HashMap::new();
        if let Some((node, _)) =
            find_node(&self.root, &segments(path), 0, &mut params, String::new())
        {
            let mut v: Vec<_> = node
                .handlers
                .keys()
                .map(|m| m.as_str().to_string())
                .collect();
            if node.handlers.contains_key(&Method::Get) && !v.iter().any(|x| x == "HEAD") {
                v.push("HEAD".into());
            }
            if !v.is_empty() {
                v.push("OPTIONS".into());
            }
            v.sort();
            v.dedup();
            v
        } else {
            Vec::new()
        }
    }

    pub async fn route(&self, mut req: Request) -> Result<Response> {
        if matches!(req.method, Method::Options) {
            let allowed = self.allowed_methods(req.path());
            if allowed.is_empty() {
                return Ok(Response::new(StatusCode::NOT_FOUND, "not found"));
            }
            let mut r = Response::new(StatusCode::NO_CONTENT, Vec::new());
            r.headers.insert("allow".into(), allowed.join(", "));
            return Ok(r);
        }
        match self.find(&req.method, req.path()) {
            Some(m) => {
                req.params = m.params;
                let mut r = (m.handler)(req).await?;
                if m.suppress_body {
                    r.body.clear();
                }
                Ok(r)
            }
            None if !self.allowed_methods(req.path()).is_empty() => Ok(Response::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method not allowed",
            )),
            None => Ok(Response::new(StatusCode::NOT_FOUND, "not found")),
        }
    }
}

fn find_node<'a>(
    node: &'a Node,
    parts: &[&str],
    idx: usize,
    params: &mut HashMap<String, String>,
    route: String,
) -> Option<(&'a Node, String)> {
    if idx == parts.len() {
        return Some((node, route));
    }
    let part = parts[idx];
    if let Some(next) = node.literal.get(part) {
        if let Some(found) = find_node(next, parts, idx + 1, params, format!("{route}/{part}")) {
            return Some(found);
        }
    }
    if let Some((name, next)) = &node.param {
        params.insert(name.clone(), part.to_string());
        if let Some(found) = find_node(next, parts, idx + 1, params, format!("{route}/:{name}")) {
            return Some(found);
        }
        params.remove(name);
    }
    if let Some((name, handlers)) = &node.wildcard {
        params.insert(name.clone(), parts[idx..].join("/"));
        let synthetic = Box::leak(Box::new(Node {
            handlers: handlers.clone(),
            ..Node::default()
        }));
        return Some((synthetic, format!("{route}/*{name}")));
    }
    None
}

fn segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn matches_params() {
        let mut r = Router::new();
        r.add(Method::Get, "/users/:id", |req| async move {
            Ok(Response::text(req.params["id"].clone()))
        });
        let req = Request {
            method: Method::Get,
            target: "/users/42".into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        assert_eq!(r.route(req).await.unwrap().body, b"42");
    }
}
