use crate::http::{Method, Request, Response};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

#[derive(Default)]
pub struct Router {
    root: Node,
}

#[derive(Default)]
struct Node {
    literal: HashMap<String, Node>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<String>,
    handlers: HashMap<Method, Handler>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<F>(&mut self, method: Method, path: &str, handler: F)
    where
        F: Fn(Request) -> Response + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        let mut node = &mut self.root;
        let segments = split_path(path);
        for seg in segments {
            if seg.starts_with(':') {
                let name = seg.trim_start_matches(':').to_string();
                node.param
                    .get_or_insert_with(|| (name, Box::new(Node::default())));
                node = node.param.as_mut().unwrap().1.as_mut();
            } else if seg.starts_with('*') {
                node.wildcard = Some(seg.trim_start_matches('*').to_string());
                break;
            } else {
                node = node.literal.entry(seg.to_string()).or_default();
            }
        }
        node.handlers.insert(method, handler);
    }

    pub fn resolve(
        &self,
        method: &Method,
        path: &str,
    ) -> Option<(Handler, HashMap<String, String>, Vec<Method>)> {
        let mut params = HashMap::new();
        let mut methods = HashSet::new();
        let node = self
            .root
            .resolve(split_path(path).as_slice(), &mut params, &mut methods)?;
        if let Some(h) = node.handlers.get(method) {
            return Some((h.clone(), params, methods.into_iter().collect()));
        }
        if matches!(method, Method::Head) {
            if let Some(h) = node.handlers.get(&Method::Get) {
                return Some((h.clone(), params, methods.into_iter().collect()));
            }
        }
        if matches!(method, Method::Options) && !methods.is_empty() {
            return Some((
                Arc::new(|_| Response::new(204, Vec::<u8>::new())),
                params,
                methods.into_iter().collect(),
            ));
        }
        None
    }
}

impl Node {
    fn resolve<'a>(
        &'a self,
        segments: &[String],
        params: &mut HashMap<String, String>,
        methods: &mut HashSet<Method>,
    ) -> Option<&'a Node> {
        methods.extend(self.handlers.keys().cloned());
        if segments.is_empty() {
            return Some(self);
        }
        if let Some(next) = self.literal.get(&segments[0]) {
            if let Some(found) = next.resolve(&segments[1..], params, methods) {
                return Some(found);
            }
        }
        if let Some((name, child)) = &self.param {
            params.insert(name.clone(), segments[0].clone());
            if let Some(found) = child.resolve(&segments[1..], params, methods) {
                return Some(found);
            }
            params.remove(name);
        }
        if let Some(name) = &self.wildcard {
            params.insert(name.clone(), segments.join("/"));
            return Some(self);
        }
        None
    }
}

fn split_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;
    #[test]
    fn routes_params() {
        let mut r = Router::new();
        r.add(Method::Get, "/users/:id", |_| Response::text(200, "ok"));
        let (_, params, _) = r.resolve(&Method::Get, "/users/7").unwrap();
        assert_eq!(params.get("id").unwrap(), "7");
    }
}
