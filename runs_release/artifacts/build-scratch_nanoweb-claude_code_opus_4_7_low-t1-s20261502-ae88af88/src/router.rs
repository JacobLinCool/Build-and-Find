use crate::http::{Method, Request, Response};
use async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type HandlerFut = Pin<Box<dyn Future<Output = Response> + Send>>;

#[async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn call(&self, req: Request) -> Response;
}

pub struct FnHandler<F>(pub F);

#[async_trait]
impl<F, Fut> Handler for FnHandler<F>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Response> + Send + 'static,
{
    async fn call(&self, req: Request) -> Response {
        (self.0)(req).await
    }
}

pub fn handler<F, Fut>(f: F) -> Arc<dyn Handler>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Response> + Send + 'static,
{
    Arc::new(FnHandler(f))
}

#[derive(Default)]
struct Node {
    /// Static children keyed by literal segment.
    literal: HashMap<String, Node>,
    /// Single named-parameter child like `:id`.
    param: Option<(String, Box<Node>)>,
    /// Trailing wildcard like `*path`. Captures everything remaining.
    wildcard: Option<String>,
    /// Method handlers attached at this node.
    handlers: HashMap<Method, Arc<dyn Handler>>,
}

#[derive(Default)]
pub struct Router {
    root: Node,
}

pub struct Match {
    pub handler: Arc<dyn Handler>,
    pub params: HashMap<String, String>,
    pub method_used: Method,
    pub head_fallback: bool,
}

pub enum RouteResolution {
    Found(Match),
    MethodNotAllowed { allowed: Vec<Method> },
    NotFound,
}

impl Router {
    pub fn new() -> Self {
        Self {
            root: Node::default(),
        }
    }

    pub fn route(&mut self, method: Method, path: &str, h: Arc<dyn Handler>) {
        let node = self.insert(path);
        node.handlers.insert(method, h);
    }

    pub fn get(&mut self, path: &str, h: Arc<dyn Handler>) {
        self.route(Method::Get, path, h);
    }
    pub fn post(&mut self, path: &str, h: Arc<dyn Handler>) {
        self.route(Method::Post, path, h);
    }
    pub fn put(&mut self, path: &str, h: Arc<dyn Handler>) {
        self.route(Method::Put, path, h);
    }
    pub fn delete(&mut self, path: &str, h: Arc<dyn Handler>) {
        self.route(Method::Delete, path, h);
    }
    pub fn patch(&mut self, path: &str, h: Arc<dyn Handler>) {
        self.route(Method::Patch, path, h);
    }

    fn insert(&mut self, path: &str) -> &mut Node {
        let segs = split_path(path);
        let mut cur = &mut self.root;
        for seg in segs {
            if let Some(name) = seg.strip_prefix('*') {
                cur.wildcard = Some(name.to_string());
                // wildcard terminates the path
                return cur;
            } else if let Some(name) = seg.strip_prefix(':') {
                if cur.param.is_none() {
                    cur.param = Some((name.to_string(), Box::new(Node::default())));
                }
                let (_, ref mut next) = cur.param.as_mut().unwrap();
                cur = next.as_mut();
            } else {
                cur = cur.literal.entry(seg.to_string()).or_default();
            }
        }
        cur
    }

    pub fn resolve(&self, method: Method, path: &str) -> RouteResolution {
        let segs = split_path(path);
        let mut params: HashMap<String, String> = HashMap::new();
        let node = match find_node(&self.root, &segs, 0, &mut params) {
            Some(n) => n,
            None => return RouteResolution::NotFound,
        };
        if let Some(h) = node.handlers.get(&method) {
            return RouteResolution::Found(Match {
                handler: h.clone(),
                params,
                method_used: method,
                head_fallback: false,
            });
        }
        if method == Method::Head {
            if let Some(h) = node.handlers.get(&Method::Get) {
                return RouteResolution::Found(Match {
                    handler: h.clone(),
                    params,
                    method_used: Method::Head,
                    head_fallback: true,
                });
            }
        }
        let allowed: Vec<Method> = node.handlers.keys().copied().collect();
        if allowed.is_empty() {
            RouteResolution::NotFound
        } else {
            RouteResolution::MethodNotAllowed { allowed }
        }
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<Method> {
        let segs = split_path(path);
        let mut params = HashMap::new();
        if let Some(node) = find_node(&self.root, &segs, 0, &mut params) {
            let mut v: Vec<Method> = node.handlers.keys().copied().collect();
            if v.contains(&Method::Get) && !v.contains(&Method::Head) {
                v.push(Method::Head);
            }
            v.push(Method::Options);
            v
        } else {
            Vec::new()
        }
    }
}

fn find_node<'a>(
    node: &'a Node,
    segs: &[&str],
    i: usize,
    params: &mut HashMap<String, String>,
) -> Option<&'a Node> {
    if i == segs.len() {
        return Some(node);
    }
    let seg = segs[i];
    if let Some(child) = node.literal.get(seg) {
        if let Some(found) = find_node(child, segs, i + 1, params) {
            return Some(found);
        }
    }
    if let Some((name, child)) = &node.param {
        params.insert(name.clone(), seg.to_string());
        if let Some(found) = find_node(child, segs, i + 1, params) {
            return Some(found);
        }
        params.remove(name);
    }
    if let Some(name) = &node.wildcard {
        let rest = segs[i..].join("/");
        params.insert(name.clone(), rest);
        return Some(node);
    }
    None
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn build_options_response(allowed: &[Method]) -> Response {
    let mut s: Vec<&str> = allowed.iter().map(|m| m.as_str()).collect();
    s.sort();
    s.dedup();
    let mut r = Response::new(crate::http::StatusCode::NO_CONTENT);
    r.headers.insert("Allow", s.join(", "));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> Arc<dyn Handler> {
        handler(|_r| async { Response::ok() })
    }

    #[test]
    fn literal_lookup() {
        let mut r = Router::new();
        r.get("/users", dummy());
        match r.resolve(Method::Get, "/users") {
            RouteResolution::Found(_) => {}
            _ => panic!(),
        }
    }

    #[test]
    fn param_lookup() {
        let mut r = Router::new();
        r.get("/users/:id", dummy());
        match r.resolve(Method::Get, "/users/42") {
            RouteResolution::Found(m) => assert_eq!(m.params.get("id").unwrap(), "42"),
            _ => panic!(),
        }
    }

    #[test]
    fn wildcard_lookup() {
        let mut r = Router::new();
        r.get("/static/*path", dummy());
        match r.resolve(Method::Get, "/static/a/b/c") {
            RouteResolution::Found(m) => assert_eq!(m.params.get("path").unwrap(), "a/b/c"),
            _ => panic!(),
        }
    }

    #[test]
    fn head_fallback() {
        let mut r = Router::new();
        r.get("/x", dummy());
        match r.resolve(Method::Head, "/x") {
            RouteResolution::Found(m) => assert!(m.head_fallback),
            _ => panic!(),
        }
    }

    #[test]
    fn method_not_allowed() {
        let mut r = Router::new();
        r.get("/x", dummy());
        match r.resolve(Method::Post, "/x") {
            RouteResolution::MethodNotAllowed { .. } => {}
            _ => panic!(),
        }
    }
}
