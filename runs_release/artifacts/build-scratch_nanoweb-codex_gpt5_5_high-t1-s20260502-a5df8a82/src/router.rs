use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};

pub type RouteParams = BTreeMap<String, String>;
pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<Response>> + Send>>;

pub trait Handler: Send + Sync + 'static {
    fn call(&self, request: Request) -> HandlerFuture;
}

impl<F, Fut> Handler for F
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response>> + Send + 'static,
{
    fn call(&self, request: Request) -> HandlerFuture {
        Box::pin((self)(request))
    }
}

#[derive(Clone)]
pub struct RouteHandler {
    pub pattern: String,
    pub handler: Arc<dyn Handler>,
}

#[derive(Clone, Default)]
pub struct Router {
    root: Node,
}

#[derive(Clone, Default)]
struct Node {
    literals: BTreeMap<String, Node>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<(String, Box<Node>)>,
    handlers: BTreeMap<Method, RouteHandler>,
}

pub struct RouteMatch {
    pub pattern: String,
    pub params: RouteParams,
    pub handler: Arc<dyn Handler>,
    pub method: Method,
}

pub enum Lookup {
    Found(RouteMatch),
    MethodNotAllowed(Vec<Method>),
    NotFound,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<H>(&mut self, method: Method, pattern: &str, handler: H) -> &mut Self
    where
        H: Handler,
    {
        let segments = segments(pattern);
        let mut node = &mut self.root;
        for segment in segments {
            if let Some(name) = segment.strip_prefix(':') {
                let child = node
                    .param
                    .get_or_insert_with(|| (name.to_string(), Box::new(Node::default())));
                node = child.1.as_mut();
            } else if let Some(name) = segment.strip_prefix('*') {
                let child = node
                    .wildcard
                    .get_or_insert_with(|| (name.to_string(), Box::new(Node::default())));
                node = child.1.as_mut();
                break;
            } else {
                node = node.literals.entry(segment.to_string()).or_default();
            }
        }
        node.handlers.insert(
            method,
            RouteHandler {
                pattern: pattern.to_string(),
                handler: Arc::new(handler),
            },
        );
        self
    }

    pub fn lookup(&self, method: &Method, path: &str) -> Lookup {
        let mut params = RouteParams::new();
        let Some(node) = lookup_node(&self.root, &segments(path), 0, &mut params) else {
            return Lookup::NotFound;
        };

        if method == &Method::Options {
            let allowed = allowed_methods(node);
            if allowed.is_empty() {
                return Lookup::NotFound;
            }
            return Lookup::MethodNotAllowed(allowed);
        }

        let selected = node.handlers.get(method).or_else(|| {
            (method == &Method::Head)
                .then(|| node.handlers.get(&Method::Get))
                .flatten()
        });
        if let Some(route) = selected {
            Lookup::Found(RouteMatch {
                pattern: route.pattern.clone(),
                params,
                handler: route.handler.clone(),
                method: method.clone(),
            })
        } else {
            let allowed = allowed_methods(node);
            if allowed.is_empty() {
                Lookup::NotFound
            } else {
                Lookup::MethodNotAllowed(allowed)
            }
        }
    }

    pub async fn handle(&self, mut req: Request) -> Result<Response> {
        match self.lookup(&req.method, &req.path) {
            Lookup::Found(found) => {
                req.params = found.params;
                req.route_pattern = Some(found.pattern);
                let is_head = req.method == Method::Head;
                let response = found.handler.call(req).await?;
                Ok(if is_head {
                    response.suppress_body()
                } else {
                    response
                })
            }
            Lookup::MethodNotAllowed(methods) if req.method == Method::Options => {
                let mut response = Response::new(StatusCode::NO_CONTENT);
                response.set_header("Allow", allow_header(&methods));
                Ok(response)
            }
            Lookup::MethodNotAllowed(methods) => {
                let mut response =
                    Response::text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
                response.set_header("Allow", allow_header(&methods));
                Ok(response)
            }
            Lookup::NotFound => Ok(Response::text(StatusCode::NOT_FOUND, "not found")),
        }
    }
}

fn lookup_node<'a>(
    node: &'a Node,
    parts: &[&str],
    index: usize,
    params: &mut RouteParams,
) -> Option<&'a Node> {
    if index == parts.len() {
        return Some(node);
    }
    let part = parts[index];
    if let Some(child) = node.literals.get(part) {
        if let Some(found) = lookup_node(child, parts, index + 1, params) {
            return Some(found);
        }
    }
    if let Some((name, child)) = &node.param {
        params.insert(name.clone(), urlencoding::decode(part).ok()?.into_owned());
        if let Some(found) = lookup_node(child, parts, index + 1, params) {
            return Some(found);
        }
        params.remove(name);
    }
    if let Some((name, child)) = &node.wildcard {
        params.insert(name.clone(), parts[index..].join("/"));
        return Some(child);
    }
    None
}

fn allowed_methods(node: &Node) -> Vec<Method> {
    let mut set = BTreeSet::new();
    for method in node.handlers.keys() {
        set.insert(method.clone());
        if method == &Method::Get {
            set.insert(Method::Head);
        }
    }
    set.insert(Method::Options);
    set.into_iter().collect()
}

fn allow_header(methods: &[Method]) -> String {
    methods
        .iter()
        .map(Method::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn matches_param_and_head() {
        let mut router = Router::new();
        router.add(Method::Get, "/users/:id", |req: Request| async move {
            Ok(Response::text(
                StatusCode::OK,
                req.path_param("id").unwrap().to_string(),
            ))
        });
        let res = router
            .handle(Request::new(Method::Get, "/users/42"))
            .await
            .unwrap();
        assert_eq!(res.body, b"42");
        let res = router
            .handle(Request::new(Method::Head, "/users/42"))
            .await
            .unwrap();
        assert!(res.body.is_empty());
    }

    #[test]
    fn matches_wildcard() {
        let mut router = Router::new();
        router.add(Method::Get, "/static/*path", |_req: Request| async {
            Ok(Response::text(StatusCode::OK, "x"))
        });
        let Lookup::Found(found) = router.lookup(&Method::Get, "/static/a/b.txt") else {
            panic!("expected route");
        };
        assert_eq!(found.params.get("path").unwrap(), "a/b.txt");
    }
}
