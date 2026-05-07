//! Trie-based router with three node kinds: literal, `:param`, and
//! `*wildcard`.
//!
//! * Registration: `router.add(Method::Get, "/users/:id", handler)`
//! * HEAD falls back to the GET handler if no explicit HEAD was
//!   registered.
//! * OPTIONS on a known path returns a response listing the methods
//!   registered for it.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, StatusCode};

/// Asynchronous boxed handler.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The signature of a user-supplied route handler.
pub type Handler =
    Arc<dyn Fn(Request) -> BoxFuture<'static, Result<Response, NanowebError>> + Send + Sync>;

/// Build a `Handler` from an async closure / fn.
pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, NanowebError>> + Send + 'static,
{
    Arc::new(move |req| Box::pin(f(req)))
}

/// What kind of trie node a segment represents.
#[derive(Debug)]
enum Segment {
    Literal(String),
    Param(String),
    Wildcard(String),
}

impl Segment {
    fn parse(raw: &str) -> Segment {
        if let Some(rest) = raw.strip_prefix(':') {
            Segment::Param(rest.to_string())
        } else if let Some(rest) = raw.strip_prefix('*') {
            Segment::Wildcard(rest.to_string())
        } else {
            Segment::Literal(raw.to_string())
        }
    }
}

/// A trie node.
#[derive(Default)]
struct Node {
    literals: HashMap<String, Node>,
    /// A single `:name` child. Only one per node — collisions error on
    /// registration.
    param: Option<(String, Box<Node>)>,
    /// Terminal `*name` catch-all.
    wildcard: Option<(String, Handlers)>,
    /// Handlers registered at this exact path.
    handlers: Handlers,
    /// The full pattern registered at this node, for metrics labels.
    pattern: Option<String>,
}

#[derive(Default)]
struct Handlers {
    map: HashMap<Method, Handler>,
}

impl Handlers {
    fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn allowed_methods(&self) -> Vec<Method> {
        self.map.keys().copied().collect()
    }
}

/// The router.
#[derive(Default)]
pub struct Router {
    root: Node,
}

/// Result of matching a request path against the trie.
pub struct Match {
    pub handler: Handler,
    pub params: HashMap<String, String>,
    pub pattern: String,
}

/// Why a lookup failed.
#[derive(Debug)]
pub enum NoMatch {
    /// Path did not correspond to any registered route.
    NotFound,
    /// Path matched but the method is not allowed. Contains the set of
    /// methods registered for the path.
    MethodNotAllowed(Vec<Method>),
}

impl Router {
    pub fn new() -> Self {
        Router::default()
    }

    /// Register a handler for `method` at `pattern`.
    pub fn add(
        &mut self,
        method: Method,
        pattern: &str,
        h: Handler,
    ) -> Result<(), NanowebError> {
        let segs: Vec<Segment> = split_path(pattern).map(Segment::parse).collect();
        let mut node: &mut Node = &mut self.root;
        let mut idx = 0;
        while idx < segs.len() {
            let seg = &segs[idx];
            match seg {
                Segment::Literal(s) => {
                    node = node.literals.entry(s.clone()).or_default();
                }
                Segment::Param(name) => match node.param {
                    Some((ref existing, _)) if existing != name => {
                        return Err(NanowebError::Routing(format!(
                            "conflicting param names ':{}' vs ':{}'",
                            existing, name
                        )))
                    }
                    Some(_) => {
                        node = node.param.as_mut().unwrap().1.as_mut();
                    }
                    None => {
                        node.param = Some((name.clone(), Box::new(Node::default())));
                        node = node.param.as_mut().unwrap().1.as_mut();
                    }
                },
                Segment::Wildcard(name) => {
                    if idx != segs.len() - 1 {
                        return Err(NanowebError::Routing(
                            "wildcard must be the final segment".into(),
                        ));
                    }
                    node.wildcard = Some((name.clone(), Handlers::default()));
                    let w = &mut node.wildcard.as_mut().unwrap().1;
                    if w.map.contains_key(&method) {
                        return Err(NanowebError::Routing(format!(
                            "duplicate wildcard handler for {} {}",
                            method, pattern
                        )));
                    }
                    w.map.insert(method, h);
                    return Ok(());
                }
            }
            idx += 1;
        }
        if node.handlers.map.contains_key(&method) {
            return Err(NanowebError::Routing(format!(
                "duplicate handler for {} {}",
                method, pattern
            )));
        }
        node.handlers.map.insert(method, h);
        node.pattern = Some(pattern.to_string());
        Ok(())
    }

    /// Look up a request. Applies HEAD-falls-back-to-GET and reports
    /// `MethodNotAllowed` with the real list of registered methods.
    pub fn find(&self, method: Method, path: &str) -> Result<Match, NoMatch> {
        let segs: Vec<&str> = split_path(path).collect();
        let mut params: HashMap<String, String> = HashMap::new();
        let hit = walk(&self.root, &segs, 0, &mut params);
        let Some(result) = hit else {
            return Err(NoMatch::NotFound);
        };
        let allowed = result.handlers.allowed_methods();
        let pattern = result
            .pattern
            .clone()
            .unwrap_or_else(|| path.to_string());
        if let Some(h) = result.handlers.map.get(&method).cloned() {
            return Ok(Match {
                handler: h,
                params,
                pattern,
            });
        }
        if method == Method::Head {
            if let Some(h) = result.handlers.map.get(&Method::Get).cloned() {
                return Ok(Match {
                    handler: h,
                    params,
                    pattern,
                });
            }
        }
        Err(NoMatch::MethodNotAllowed(allowed))
    }

    /// Produce the OPTIONS response body for a path: a 204 with an
    /// `Allow` header when the path is known, else 404.
    pub fn options_response(&self, path: &str) -> Response {
        let mut params = HashMap::new();
        let segs: Vec<&str> = split_path(path).collect();
        let hit = walk(&self.root, &segs, 0, &mut params);
        match hit {
            Some(r) => {
                let mut methods = r.handlers.allowed_methods();
                if methods.contains(&Method::Get) && !methods.contains(&Method::Head) {
                    methods.push(Method::Head);
                }
                methods.push(Method::Options);
                methods.sort_by_key(|m| m.as_str());
                methods.dedup();
                let allow = methods
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut resp = Response::status(StatusCode::NO_CONTENT);
                resp.headers.set("allow", allow);
                resp
            }
            None => Response::status(StatusCode::NOT_FOUND),
        }
    }
}

struct WalkResult<'a> {
    handlers: &'a Handlers,
    pattern: &'a Option<String>,
}

fn walk<'a>(
    node: &'a Node,
    segs: &[&str],
    depth: usize,
    params: &mut HashMap<String, String>,
) -> Option<WalkResult<'a>> {
    if depth == segs.len() {
        if node.handlers.is_empty() {
            return None;
        }
        return Some(WalkResult {
            handlers: &node.handlers,
            pattern: &node.pattern,
        });
    }
    let seg = segs[depth];
    if let Some(child) = node.literals.get(seg) {
        if let Some(hit) = walk(child, segs, depth + 1, params) {
            return Some(hit);
        }
    }
    if let Some((name, child)) = node.param.as_ref() {
        params.insert(name.clone(), seg.to_string());
        if let Some(hit) = walk(child, segs, depth + 1, params) {
            return Some(hit);
        }
        params.remove(name);
    }
    if let Some((name, handlers)) = node.wildcard.as_ref() {
        params.insert(name.clone(), segs[depth..].join("/"));
        return Some(WalkResult {
            handlers,
            pattern: &node.pattern,
        });
    }
    None
}

fn split_path(path: &str) -> impl Iterator<Item = &str> {
    path.trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body(req: Request) -> Result<Response, NanowebError> {
        Ok(Response::text(format!("hit {}", req.path)))
    }

    fn make_handler() -> Handler {
        handler(|r| async move { body(r).await })
    }

    #[test]
    fn literal_and_param_match() {
        let mut r = Router::new();
        r.add(Method::Get, "/users/:id/profile", make_handler())
            .unwrap();
        let m = r.find(Method::Get, "/users/42/profile").unwrap();
        assert_eq!(m.params.get("id"), Some(&"42".to_string()));
        assert_eq!(m.pattern, "/users/:id/profile");
    }

    #[test]
    fn wildcard_must_be_last() {
        let mut r = Router::new();
        assert!(r
            .add(Method::Get, "/files/*path/tail", make_handler())
            .is_err());
    }

    #[test]
    fn wildcard_captures_rest() {
        let mut r = Router::new();
        r.add(Method::Get, "/files/*rest", make_handler()).unwrap();
        let m = r.find(Method::Get, "/files/a/b/c.txt").unwrap();
        assert_eq!(m.params.get("rest"), Some(&"a/b/c.txt".to_string()));
    }

    #[test]
    fn head_falls_back_to_get() {
        let mut r = Router::new();
        r.add(Method::Get, "/x", make_handler()).unwrap();
        assert!(r.find(Method::Head, "/x").is_ok());
    }

    #[test]
    fn method_not_allowed() {
        let mut r = Router::new();
        r.add(Method::Get, "/x", make_handler()).unwrap();
        match r.find(Method::Post, "/x") {
            Err(NoMatch::MethodNotAllowed(methods)) => {
                assert!(methods.contains(&Method::Get))
            }
            Err(NoMatch::NotFound) => panic!("expected MNA, got NotFound"),
            Ok(_) => panic!("expected MNA, got match"),
        }
    }

    #[test]
    fn options_lists_methods() {
        let mut r = Router::new();
        r.add(Method::Get, "/x", make_handler()).unwrap();
        r.add(Method::Post, "/x", make_handler()).unwrap();
        let resp = r.options_response("/x");
        let allow = resp.headers.get("allow").unwrap();
        assert!(allow.contains("GET"));
        assert!(allow.contains("POST"));
        assert!(allow.contains("OPTIONS"));
        assert!(allow.contains("HEAD"));
    }
}
