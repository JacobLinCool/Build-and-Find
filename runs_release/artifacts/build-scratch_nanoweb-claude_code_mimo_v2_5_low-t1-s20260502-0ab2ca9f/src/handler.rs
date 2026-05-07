use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::http::{Request, Response};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub type HandlerFn = Arc<dyn Fn(Request) -> BoxFuture<Response> + Send + Sync>;

pub struct App {
    pub handler: HandlerFn,
}

impl App {
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        App {
            handler: Arc::new(move |req| Box::pin(handler(req))),
        }
    }
}

pub fn simple_handler<F>(handler: F) -> HandlerFn
where
    F: Fn(Request) -> Response + Send + Sync + 'static,
{
    Arc::new(move |req| {
        let resp = handler(req);
        Box::pin(async move { resp })
    })
}

pub fn async_handler<F, Fut>(handler: F) -> HandlerFn
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Response> + Send + 'static,
{
    Arc::new(move |req| Box::pin(handler(req)))
}
