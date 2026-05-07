use crate::{
    http::{Method, Request, Response},
    metrics::Metrics,
    middleware::Middleware,
};
use async_trait::async_trait;
use std::{collections::HashMap, future::Future, sync::Arc, time::Instant};

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, request: Request) -> Response;
}

#[async_trait]
impl<F, Fut> Handler for F
where
    F: Fn(Request) -> Fut + Send + Sync,
    Fut: Future<Output = Response> + Send,
{
    async fn handle(&self, request: Request) -> Response {
        (self)(request).await
    }
}

#[derive(Clone)]
pub struct RouteEntry {
    pub pattern: String,
    pub handler: Arc<dyn Handler>,
}

#[derive(Default)]
struct Node {
    literals: HashMap<String, Node>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<(String, HashMap<Method, RouteEntry>)>,
    handlers: HashMap<Method, RouteEntry>,
}

#[derive(Default)]
pub struct Router {
    root: Node,
}

pub struct RouteMatch {
    pub entry: RouteEntry,
    pub params: HashMap<String, String>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<H>(&mut self, method: Method, pattern: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.add_arc(method, pattern, Arc::new(handler));
    }

    pub fn add_arc(&mut self, method: Method, pattern: &str, handler: Arc<dyn Handler>) {
        let mut node = &mut self.root;
        let segments = segments(pattern);
        for (idx, segment) in segments.iter().enumerate() {
            if let Some(name) = segment.strip_prefix('*') {
                if idx != segments.len() - 1 {
                    panic!("wildcard segment must be trailing");
                }
                let table = &mut node
                    .wildcard
                    .get_or_insert_with(|| (name.to_string(), HashMap::new()))
                    .1;
                table.insert(
                    method,
                    RouteEntry {
                        pattern: pattern.to_string(),
                        handler,
                    },
                );
                return;
            } else if let Some(name) = segment.strip_prefix(':') {
                node = node
                    .param
                    .get_or_insert_with(|| (name.to_string(), Box::<Node>::default()))
                    .1
                    .as_mut();
            } else {
                node = node.literals.entry((*segment).to_string()).or_default();
            }
        }
        node.handlers.insert(
            method,
            RouteEntry {
                pattern: pattern.to_string(),
                handler,
            },
        );
    }

    pub fn lookup(&self, method: &Method, path: &str) -> Option<RouteMatch> {
        let mut params = HashMap::new();
        self.lookup_node(&self.root, method, &segments(path), 0, &mut params)
    }

    pub fn allowed_methods(&self, path: &str) -> Vec<Method> {
        let mut out = Vec::new();
        for method in Method::routable() {
            if self.lookup(&method, path).is_some()
                || (method == Method::Head && self.lookup(&Method::Get, path).is_some())
            {
                out.push(method);
            }
        }
        if !out.contains(&Method::Options) && !out.is_empty() {
            out.push(Method::Options);
        }
        out
    }

    fn lookup_node(
        &self,
        node: &Node,
        method: &Method,
        segs: &[&str],
        pos: usize,
        params: &mut HashMap<String, String>,
    ) -> Option<RouteMatch> {
        if pos == segs.len() {
            return select_handler(&node.handlers, method).map(|entry| RouteMatch {
                entry,
                params: params.clone(),
            });
        }

        let segment = segs[pos];
        if let Some(child) = node.literals.get(segment) {
            if let Some(found) = self.lookup_node(child, method, segs, pos + 1, params) {
                return Some(found);
            }
        }
        if let Some((name, child)) = &node.param {
            params.insert(name.clone(), segment.to_string());
            if let Some(found) = self.lookup_node(child, method, segs, pos + 1, params) {
                return Some(found);
            }
            params.remove(name);
        }
        if let Some((name, handlers)) = &node.wildcard {
            params.insert(name.clone(), segs[pos..].join("/"));
            let found = select_handler(handlers, method).map(|entry| RouteMatch {
                entry,
                params: params.clone(),
            });
            params.remove(name);
            return found;
        }
        None
    }
}

fn select_handler(handlers: &HashMap<Method, RouteEntry>, method: &Method) -> Option<RouteEntry> {
    handlers
        .get(method)
        .or_else(|| {
            if method == &Method::Head {
                handlers.get(&Method::Get)
            } else {
                None
            }
        })
        .cloned()
}

fn segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

pub struct App {
    router: Router,
    middleware: Vec<Arc<dyn Middleware>>,
    metrics: Arc<Metrics>,
    ready: Arc<std::sync::atomic::AtomicBool>,
}

impl App {
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            middleware: Vec::new(),
            metrics: Arc::new(Metrics::new()),
            ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    pub fn router(&self) -> &Router {
        &self.router
    }

    pub fn router_mut(&mut self) -> &mut Router {
        &mut self.router
    }

    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }

    pub fn ready_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.ready.clone()
    }

    pub fn add_route<H>(&mut self, method: Method, path: &str, handler: H)
    where
        H: Handler + 'static,
    {
        self.router.add(method, path, handler);
    }

    pub fn add_middleware<M>(&mut self, middleware: M)
    where
        M: Middleware + 'static,
    {
        self.middleware.push(Arc::new(middleware));
    }

    pub async fn handle(&self, mut request: Request) -> Response {
        let start = Instant::now();
        let method = request.method.clone();
        let path = request.path.clone();
        let suppress_body = request.method == Method::Head;

        let mut response = if method == Method::Options {
            let allowed = self.router.allowed_methods(&path);
            if allowed.is_empty() {
                Response::text(404, "not found")
            } else {
                let mut r = Response::text(204, "");
                r.set_header(
                    "Allow",
                    allowed
                        .iter()
                        .map(Method::as_str)
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                r
            }
        } else if let Some(found) = self.router.lookup(&method, &path) {
            request.params = found.params;
            let route = found.entry.pattern.clone();
            let mut short = None;
            for mw in &self.middleware {
                if let Some(resp) = mw.before(&mut request).await {
                    short = Some(resp);
                    break;
                }
            }
            let mut response = match short {
                Some(resp) => resp,
                None => found.entry.handler.handle(request.clone()).await,
            };
            for mw in self.middleware.iter().rev() {
                mw.after(&request, &mut response).await;
            }
            self.metrics
                .observe(method.as_str(), response.status, &route, start.elapsed());
            response
        } else if !self.router.allowed_methods(&path).is_empty() {
            let mut r = Response::text(405, "method not allowed");
            r.set_header(
                "Allow",
                self.router
                    .allowed_methods(&path)
                    .iter()
                    .map(Method::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            r
        } else {
            Response::text(404, "not found")
        };

        response.set_header("X-Request-Id", request.request_id);
        if suppress_body {
            response.body.clear();
        }
        response
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trie_matches_params_and_head() {
        let mut app = App::new();
        app.add_route(Method::Get, "/users/:id", |req: Request| async move {
            Response::text(200, req.params["id"].clone())
        });
        let res = app.handle(Request::new(Method::Head, "/users/42")).await;
        assert_eq!(res.status, 200);
        assert!(res.body.is_empty());
    }

    #[test]
    fn wildcard_matches_tail() {
        let mut router = Router::new();
        router.add(Method::Get, "/static/*path", |req: Request| async move {
            Response::text(200, req.params["path"].clone())
        });
        let found = router.lookup(&Method::Get, "/static/a/b.txt").unwrap();
        assert_eq!(found.params["path"], "a/b.txt");
    }
}
