use crate::http::{Method, Request, Response};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, req: Request) -> Response;
}

#[async_trait]
impl<F, Fut> Handler for F
where
    F: Fn(Request) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Response> + Send,
{
    async fn handle(&self, req: Request) -> Response {
        (self)(req).await
    }
}

pub type SharedHandler = Arc<dyn Handler>;

#[derive(Default)]
struct TrieNode {
    literals: HashMap<String, TrieNode>,
    /// Single-segment parameter child, e.g. `:id`. Stores parameter name.
    param: Option<(String, Box<TrieNode>)>,
    /// Trailing wildcard child, e.g. `*path`. Stores parameter name and handlers.
    wildcard: Option<(String, MethodHandlers)>,
    handlers: MethodHandlers,
}

#[derive(Default)]
struct MethodHandlers {
    map: HashMap<Method, SharedHandler>,
}

impl MethodHandlers {
    fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    fn insert(&mut self, m: Method, h: SharedHandler) {
        self.map.insert(m, h);
    }
    fn get(&self, m: Method) -> Option<&SharedHandler> {
        self.map.get(&m)
    }
    fn allowed(&self) -> Vec<Method> {
        let mut v: Vec<Method> = self.map.keys().copied().collect();
        v.sort_by_key(|m| m.as_str());
        if self.map.contains_key(&Method::Get) && !self.map.contains_key(&Method::Head) {
            v.push(Method::Head);
        }
        if !v.contains(&Method::Options) {
            v.push(Method::Options);
        }
        v
    }
}

#[derive(Default)]
pub struct Router {
    root: TrieNode,
}

#[derive(Debug, Clone)]
pub enum RouteOutcome {
    Found {
        params: Vec<(String, String)>,
        // method dispatch:
    },
    NotFound,
}

pub struct Match<'a> {
    pub handler: &'a SharedHandler,
    pub params: Vec<(String, String)>,
    /// Was this dispatched via HEAD->GET fallback?
    pub head_fallback: bool,
}

pub struct AllowedMatch {
    pub allowed: Vec<Method>,
    pub params: Vec<(String, String)>,
}

pub enum Lookup<'a> {
    Found(Match<'a>),
    MethodNotAllowed(AllowedMatch),
    NotFound,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route<H>(&mut self, method: Method, path: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.add(method, path, Arc::new(handler));
    }

    pub fn get<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Get, path, handler);
    }
    pub fn post<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Post, path, handler);
    }
    pub fn put<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Put, path, handler);
    }
    pub fn delete<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Delete, path, handler);
    }
    pub fn patch<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Patch, path, handler);
    }
    pub fn options<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Options, path, handler);
    }
    pub fn head<H: Handler + 'static>(&mut self, path: &str, handler: H) {
        self.route(Method::Head, path, handler);
    }

    pub fn add(&mut self, method: Method, path: &str, handler: SharedHandler) {
        let segments: Vec<&str> = split_path(path);
        let mut node = &mut self.root;
        let mut i = 0;
        while i < segments.len() {
            let seg = segments[i];
            if let Some(rest) = seg.strip_prefix('*') {
                // wildcard MUST be the last segment
                assert!(
                    i == segments.len() - 1,
                    "wildcard segment must be last in path"
                );
                let name = rest.to_string();
                let entry = node
                    .wildcard
                    .get_or_insert_with(|| (name.clone(), MethodHandlers::default()));
                entry.0 = name;
                entry.1.insert(method, handler);
                return;
            } else if let Some(rest) = seg.strip_prefix(':') {
                let name = rest.to_string();
                if node.param.is_none() {
                    node.param = Some((name.clone(), Box::new(TrieNode::default())));
                } else if let Some((existing, _)) = &node.param {
                    assert_eq!(existing, &name, "conflicting parameter name");
                }
                let next = &mut node.param.as_mut().unwrap().1;
                node = next;
            } else {
                let entry = node
                    .literals
                    .entry(seg.to_string())
                    .or_insert_with(TrieNode::default);
                node = entry;
            }
            i += 1;
        }
        node.handlers.insert(method, handler);
    }

    pub fn lookup<'a>(&'a self, method: Method, path: &str) -> Lookup<'a> {
        let segments: Vec<&str> = split_path(path);
        let mut params: Vec<(String, String)> = Vec::new();
        return walk(&self.root, &segments, 0, &mut params, method);

        #[allow(clippy::too_many_arguments)]
        fn walk<'a>(
            node: &'a TrieNode,
            segments: &[&str],
            depth: usize,
            params: &mut Vec<(String, String)>,
            method: Method,
        ) -> Lookup<'a> {
            if depth == segments.len() {
                if let Some(h) = node.handlers.get(method) {
                    return Lookup::Found(Match {
                        handler: h,
                        params: std::mem::take(params),
                        head_fallback: false,
                    });
                }
                if method == Method::Head {
                    if let Some(h) = node.handlers.get(Method::Get) {
                        return Lookup::Found(Match {
                            handler: h,
                            params: std::mem::take(params),
                            head_fallback: true,
                        });
                    }
                }
                if !node.handlers.is_empty() {
                    return Lookup::MethodNotAllowed(AllowedMatch {
                        allowed: node.handlers.allowed(),
                        params: std::mem::take(params),
                    });
                }
                // fallback to wildcard at this node with empty path
                if let Some((name, h)) = &node.wildcard {
                    if let Some(handler) = h.get(method) {
                        params.push((name.clone(), String::new()));
                        return Lookup::Found(Match {
                            handler,
                            params: std::mem::take(params),
                            head_fallback: false,
                        });
                    }
                    if method == Method::Head {
                        if let Some(handler) = h.get(Method::Get) {
                            params.push((name.clone(), String::new()));
                            return Lookup::Found(Match {
                                handler,
                                params: std::mem::take(params),
                                head_fallback: true,
                            });
                        }
                    }
                    return Lookup::MethodNotAllowed(AllowedMatch {
                        allowed: h.allowed(),
                        params: std::mem::take(params),
                    });
                }
                return Lookup::NotFound;
            }
            let seg = segments[depth];
            // literal first
            if let Some(child) = node.literals.get(seg) {
                let saved = params.clone();
                let sub = walk(child, segments, depth + 1, params, method);
                if matches!(sub, Lookup::Found(_) | Lookup::MethodNotAllowed(_)) {
                    return sub;
                }
                *params = saved;
            }
            // param
            if let Some((name, child)) = &node.param {
                let saved = params.clone();
                params.push((name.clone(), seg.to_string()));
                let sub = walk(child, segments, depth + 1, params, method);
                if matches!(sub, Lookup::Found(_) | Lookup::MethodNotAllowed(_)) {
                    return sub;
                }
                *params = saved;
            }
            // wildcard
            if let Some((name, h)) = &node.wildcard {
                let rest = segments[depth..].join("/");
                params.push((name.clone(), rest));
                if let Some(handler) = h.get(method) {
                    return Lookup::Found(Match {
                        handler,
                        params: std::mem::take(params),
                        head_fallback: false,
                    });
                }
                if method == Method::Head {
                    if let Some(handler) = h.get(Method::Get) {
                        return Lookup::Found(Match {
                            handler,
                            params: std::mem::take(params),
                            head_fallback: true,
                        });
                    }
                }
                return Lookup::MethodNotAllowed(AllowedMatch {
                    allowed: h.allowed(),
                    params: std::mem::take(params),
                });
            }
            Lookup::NotFound
        }
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Version;

    fn dummy(s: &'static str) -> SharedHandler {
        let h = move |_req: Request| {
            let s = s;
            async move { Response::text(s) }
        };
        Arc::new(h) as SharedHandler
    }

    #[test]
    fn literal_route() {
        let mut r = Router::new();
        r.add(Method::Get, "/foo", dummy("foo"));
        let m = match r.lookup(Method::Get, "/foo") {
            Lookup::Found(m) => m,
            _ => panic!(),
        };
        assert!(m.params.is_empty());
    }

    #[test]
    fn param_route() {
        let mut r = Router::new();
        r.add(Method::Get, "/users/:id", dummy("u"));
        let m = match r.lookup(Method::Get, "/users/42") {
            Lookup::Found(m) => m,
            _ => panic!(),
        };
        assert_eq!(m.params, vec![("id".to_string(), "42".to_string())]);
    }

    #[test]
    fn wildcard_route() {
        let mut r = Router::new();
        r.add(Method::Get, "/static/*path", dummy("s"));
        let m = match r.lookup(Method::Get, "/static/a/b/c.txt") {
            Lookup::Found(m) => m,
            _ => panic!(),
        };
        assert_eq!(
            m.params,
            vec![("path".to_string(), "a/b/c.txt".to_string())]
        );
    }

    #[test]
    fn head_falls_back_to_get() {
        let mut r = Router::new();
        r.add(Method::Get, "/foo", dummy("foo"));
        let m = match r.lookup(Method::Head, "/foo") {
            Lookup::Found(m) => m,
            _ => panic!(),
        };
        assert!(m.head_fallback);
    }

    #[test]
    fn method_not_allowed() {
        let mut r = Router::new();
        r.add(Method::Get, "/foo", dummy("foo"));
        let _ = match r.lookup(Method::Post, "/foo") {
            Lookup::MethodNotAllowed(_) => {}
            _ => panic!(),
        };
    }

    #[test]
    fn not_found() {
        let r = Router::new();
        assert!(matches!(r.lookup(Method::Get, "/nope"), Lookup::NotFound));
    }

    #[tokio::test]
    async fn handler_invocation() {
        let mut r = Router::new();
        r.add(Method::Get, "/foo", dummy("hi"));
        let m = match r.lookup(Method::Get, "/foo") {
            Lookup::Found(m) => m,
            _ => panic!(),
        };
        let req = Request::new(Method::Get, "/foo".into(), Version::Http11);
        let resp = m.handler.handle(req).await;
        assert_eq!(&resp.body, b"hi");
    }
}
