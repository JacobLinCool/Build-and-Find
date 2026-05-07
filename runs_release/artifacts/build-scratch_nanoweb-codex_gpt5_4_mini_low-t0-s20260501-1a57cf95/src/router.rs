use crate::http::types::{Method, Params, Request, Response};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

#[derive(Default)]
struct Node {
    literal: HashMap<String, Node>,
    param: Option<(String, Box<Node>)>,
    wildcard: Option<(String, HandlerMap)>,
    handlers: HandlerMap,
}

type HandlerMap = HashMap<String, Handler>;

#[derive(Default)]
pub struct Router {
    root: Node,
}

impl Router {
    pub fn add(&mut self, method: Method, path: &str, handler: Handler) {
        let segments = split_path(path);
        let mut node = &mut self.root;
        for seg in segments.iter().take(segments.len().saturating_sub(1)) {
            node = node.literal.entry(seg.to_string()).or_default();
        }
        if let Some(last) = segments.last() {
            if last.starts_with(':') {
                let key = last.trim_start_matches(':').to_string();
                node.param = Some((key, Box::new(Node::default())));
                node = node.param.as_mut().unwrap().1.as_mut();
            } else if last.starts_with('*') {
                let key = last.trim_start_matches('*').to_string();
                node.wildcard = Some((key, HashMap::new()));
                node.wildcard
                    .as_mut()
                    .unwrap()
                    .1
                    .insert(method.as_str().to_string(), handler);
                return;
            } else {
                node = node.literal.entry(last.to_string()).or_default();
            }
        }
        node.handlers.insert(method.as_str().to_string(), handler);
    }

    pub fn resolve(&self, method: &Method, path: &str) -> Option<(Handler, Params, Vec<String>)> {
        let segments = split_path(path);
        let mut params = Params::new();
        let mut allowed = BTreeSet::new();
        let mut node = &self.root;
        for (i, seg) in segments.iter().enumerate() {
            if let Some(next) = node.literal.get(*seg) {
                node = next;
            } else if let Some((name, child)) = &node.param {
                params.insert(name.clone(), seg.to_string());
                node = child;
            } else if let Some((name, handlers)) = &node.wildcard {
                params.insert(name.clone(), segments[i..].join("/"));
                let h = handlers.get(method.as_str())?;
                return Some((h.clone(), params, allowed.into_iter().collect()));
            } else {
                return None;
            }
        }
        for k in node.handlers.keys() {
            allowed.insert(k.clone());
        }
        if let Some(h) = node.handlers.get(method.as_str()) {
            return Some((h.clone(), params, allowed.into_iter().collect()));
        }
        if matches!(method, Method::Head) {
            if let Some(h) = node.handlers.get("GET") {
                return Some((h.clone(), params, allowed.into_iter().collect()));
            }
        }
        None
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn route_params() {
        let mut r = Router::default();
        r.add(
            Method::Get,
            "/users/:id",
            Arc::new(|_| Response::text(200, "ok")),
        );
        let (h, params, _) = r.resolve(&Method::Get, "/users/1").unwrap();
        assert_eq!(params["id"], "1");
        assert_eq!(
            (h)(Request {
                method: Method::Get,
                target: "/users/1".into(),
                version: "HTTP/1.1".into(),
                headers: Default::default(),
                body: vec![],
                params: Default::default(),
                peer_addr: None,
                request_id: String::new()
            })
            .status,
            200
        );
    }
}
