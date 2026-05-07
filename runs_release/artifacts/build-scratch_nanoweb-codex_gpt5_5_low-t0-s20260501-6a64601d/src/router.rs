use crate::http::{Method, Request, Response, StatusCode};
use crate::middleware::{Middleware, Next};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub type Params = HashMap<String, String>;

#[async_trait]
pub trait Handler: Send + Sync {
    async fn call(&self, req: Request) -> Response;
}

#[async_trait]
impl<F, Fut> Handler for F
where
    F: Send + Sync + Fn(Request) -> Fut,
    Fut: std::future::Future<Output = Response> + Send,
{
    async fn call(&self, req: Request) -> Response {
        (self)(req).await
    }
}

pub type HandlerFn = Arc<dyn Handler>;

#[derive(Clone)]
pub struct RouteMatch {
    pub handler: HandlerFn,
    pub params: Params,
    pub route: String,
    pub suppress_body: bool,
}

#[derive(Default)]
struct Node {
    literal: HashMap<String, Node>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<(String, HandlerMap)>,
    handlers: HandlerMap,
}

#[derive(Default, Clone)]
struct HandlerMap {
    handlers: HashMap<Method, HandlerFn>,
    pattern: String,
}

#[derive(Default)]
pub struct Router {
    root: Node,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<H>(&mut self, method: Method, path: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.add_arc(method, path, Arc::new(handler));
    }

    pub fn add_arc(&mut self, method: Method, path: &str, handler: HandlerFn) {
        let mut node = &mut self.root;
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if segments.is_empty() {
            node.handlers.handlers.insert(method, handler);
            node.handlers.pattern = path.to_string();
            return;
        }
        for (i, seg) in segments.iter().enumerate() {
            if let Some(name) = seg.strip_prefix('*') {
                node.wildcard
                    .get_or_insert_with(|| (name.to_string(), HandlerMap::default()))
                    .1
                    .handlers
                    .insert(method, handler);
                node.wildcard.as_mut().unwrap().1.pattern = path.to_string();
                return;
            }
            if let Some(name) = seg.strip_prefix(':') {
                node = &mut node
                    .param
                    .get_or_insert_with(|| (name.to_string(), Box::new(Node::default())))
                    .1;
            } else {
                node = node.literal.entry((*seg).to_string()).or_default();
            }
            if i == segments.len() - 1 {
                node.handlers
                    .handlers
                    .insert(method.clone(), handler.clone());
                node.handlers.pattern = path.to_string();
            }
        }
    }

    pub fn lookup(&self, method: &Method, path: &str) -> Lookup {
        let mut params = HashMap::new();
        let segments: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        match lookup_node(&self.root, &segments, 0, &mut params) {
            Some(map) => dispatch(map, method, params),
            None => Lookup::NotFound,
        }
    }
}

fn lookup_node<'a>(
    node: &'a Node,
    segments: &[&str],
    idx: usize,
    params: &mut Params,
) -> Option<&'a HandlerMap> {
    if idx == segments.len() {
        return (!node.handlers.handlers.is_empty()).then_some(&node.handlers);
    }
    let seg = segments[idx];
    if let Some(child) = node.literal.get(seg) {
        if let Some(found) = lookup_node(child, segments, idx + 1, params) {
            return Some(found);
        }
    }
    if let Some((name, child)) = &node.param {
        params.insert(name.clone(), seg.to_string());
        if let Some(found) = lookup_node(child, segments, idx + 1, params) {
            return Some(found);
        }
        params.remove(name);
    }
    if let Some((name, map)) = &node.wildcard {
        params.insert(name.clone(), segments[idx..].join("/"));
        return Some(map);
    }
    None
}

fn dispatch(map: &HandlerMap, method: &Method, params: Params) -> Lookup {
    if *method == Method::Options {
        return Lookup::Options(allowed(&map.handlers));
    }
    if *method == Method::Head {
        if let Some(handler) = map
            .handlers
            .get(&Method::Head)
            .or_else(|| map.handlers.get(&Method::Get))
        {
            return Lookup::Found(RouteMatch {
                handler: handler.clone(),
                params,
                route: map.pattern.clone(),
                suppress_body: true,
            });
        }
    }
    match map.handlers.get(method) {
        Some(handler) => Lookup::Found(RouteMatch {
            handler: handler.clone(),
            params,
            route: map.pattern.clone(),
            suppress_body: false,
        }),
        None => Lookup::MethodNotAllowed(allowed(&map.handlers)),
    }
}

fn allowed(map: &HashMap<Method, HandlerFn>) -> String {
    let mut set: HashSet<&str> = map.keys().map(Method::as_str).collect();
    if set.contains("GET") {
        set.insert("HEAD");
    }
    set.insert("OPTIONS");
    let mut v: Vec<&str> = set.into_iter().collect();
    v.sort_unstable();
    v.join(", ")
}

pub enum Lookup {
    Found(RouteMatch),
    Options(String),
    MethodNotAllowed(String),
    NotFound,
}

#[derive(Clone, Default)]
pub struct App {
    router: Arc<Router>,
    middleware: Arc<Vec<Arc<dyn Middleware>>>,
}

impl App {
    pub fn new(router: Router) -> Self {
        Self {
            router: Arc::new(router),
            middleware: Arc::new(Vec::new()),
        }
    }

    pub fn with_middleware(mut self, middleware: Vec<Arc<dyn Middleware>>) -> Self {
        self.middleware = Arc::new(middleware);
        self
    }

    pub async fn handle(&self, mut req: Request) -> (Response, String, bool) {
        match self.router.lookup(&req.method, &req.path) {
            Lookup::Found(matched) => {
                req.params = matched.params.clone();
                let next = Next::new(self.middleware.clone(), matched.handler.clone());
                let res = next.run(req).await;
                (res, matched.route, matched.suppress_body)
            }
            Lookup::Options(allow) => {
                let mut res = Response::new(StatusCode::NoContent);
                res.header("Allow", allow);
                (res, "<options>".into(), false)
            }
            Lookup::MethodNotAllowed(allow) => {
                let mut res = Response::text(StatusCode::MethodNotAllowed, "method not allowed");
                res.header("Allow", allow);
                (res, "<method-not-allowed>".into(), false)
            }
            Lookup::NotFound => (
                Response::text(StatusCode::NotFound, "not found"),
                "<not-found>".into(),
                false,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn matches_param_and_head() {
        let mut r = Router::new();
        r.add(Method::Get, "/users/:id", |req: Request| async move {
            Response::text(StatusCode::Ok, req.params["id"].clone())
        });
        let l = r.lookup(&Method::Head, "/users/42");
        assert!(matches!(l, Lookup::Found(_)));
    }
}
