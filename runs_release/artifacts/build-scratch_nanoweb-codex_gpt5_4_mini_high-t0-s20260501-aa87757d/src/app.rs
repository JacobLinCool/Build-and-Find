use crate::{
    config::{Config, HandlerConfig},
    error::Result,
    health::{HealthHandler, ReadyHandler},
    http::types::{Method, Request, Response, StatusCode},
    metrics::Metrics,
    middleware::{
        build_middleware_from_specs, build_standard_middleware, Handler, Middleware, Next,
    },
    router::Router,
    session::SessionManager,
    static_files::StaticFileHandler,
    template::Renderer,
};
use async_trait::async_trait;
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub metrics: Arc<Metrics>,
    pub sessions: Arc<SessionManager>,
    pub renderer: Renderer,
    pub ready: Arc<AtomicBool>,
}

impl AppState {
    pub fn test() -> Self {
        let config = Arc::new(Config::default());
        let metrics = Arc::new(Metrics::default());
        let sessions = Arc::new(SessionManager::default_with_secret(b"test-secret".to_vec()));
        Self {
            config,
            metrics,
            sessions,
            renderer: Renderer::new(),
            ready: Arc::new(AtomicBool::new(true)),
        }
    }
}

pub struct App {
    pub router: Router,
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub state: Arc<AppState>,
}

impl App {
    pub fn from_config(config: Config) -> Result<Self> {
        let config = Arc::new(config);
        let state = Arc::new(AppState {
            config: config.clone(),
            metrics: Arc::new(Metrics::default()),
            sessions: Arc::new(SessionManager::default_with_secret(
                config
                    .session_secret
                    .clone()
                    .unwrap_or_else(|| "nanoweb-development-secret".to_string())
                    .into_bytes(),
            )),
            renderer: Renderer::new(),
            ready: Arc::new(AtomicBool::new(false)),
        });
        let middleware = build_standard_middleware(&config);
        let mut app = Self {
            router: Router::new(),
            middleware,
            state,
        };
        app.register_builtin_routes()?;
        for route in &config.routes {
            let handler = route.handler.clone().into_handler()?;
            let route_middleware = build_middleware_from_specs(&route.middleware);
            app.router
                .add(route.method, &route.path, handler, route_middleware)?;
        }
        if let Some(dir) = &config.static_dir {
            let mount = format!("{}/", config.static_mount.trim_end_matches('/'));
            let route_path = format!("{}*path", mount);
            let handler = Arc::new(StaticFileHandler::new(dir));
            app.router
                .add(Method::GET, &route_path, handler.clone(), Vec::new())?;
            app.router
                .add(Method::HEAD, &route_path, handler, Vec::new())?;
        }
        Ok(app)
    }

    fn register_builtin_routes(&mut self) -> Result<()> {
        self.router
            .add(Method::GET, "/healthz", Arc::new(HealthHandler), Vec::new())?;
        self.router
            .add(Method::GET, "/readyz", Arc::new(ReadyHandler), Vec::new())?;
        let metrics_route = self.state.config.metrics.clone();
        self.router.add(
            Method::GET,
            &metrics_route,
            Arc::new(MetricsHandler),
            Vec::new(),
        )?;
        Ok(())
    }

    pub async fn handle(&self, mut req: Request) -> Result<Response> {
        if let Some(route) = self.router.resolve(req.method, &req.path) {
            if let Some(target) = route.target {
                req = req.with_params(route.params, Some(target.pattern.clone()));
                if req.method == Method::OPTIONS {
                    let mut response = Response::empty(StatusCode::Ok);
                    response.set_header(
                        "allow",
                        route
                            .allowed_methods
                            .iter()
                            .map(|m| m.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    response.set_header("x-request-id", req.request_id.clone());
                    return Ok(response);
                }
                let mut chain = self.middleware.clone();
                chain.extend(target.middleware.iter().cloned());
                let response = Next::new(&chain, target.handler.as_ref(), self.state.clone())
                    .run(req)
                    .await?;
                return Ok(response);
            }
        }
        Ok(Response::text(StatusCode::NotFound, "not found"))
    }
}

trait IntoHandler {
    fn into_handler(self) -> Result<Arc<dyn Handler>>;
}

impl IntoHandler for HandlerConfig {
    fn into_handler(self) -> Result<Arc<dyn Handler>> {
        match self {
            HandlerConfig::Text { body, content_type } => {
                Ok(Arc::new(TextHandler { body, content_type }))
            }
            HandlerConfig::Json { value } => Ok(Arc::new(JsonValueHandler { value })),
            HandlerConfig::Template {
                template,
                context,
                content_type,
            } => Ok(Arc::new(TemplateHandler {
                template,
                context,
                content_type,
            })),
            HandlerConfig::Echo => Ok(Arc::new(EchoHandler)),
            HandlerConfig::File { path } => Ok(Arc::new(FileHandler { path })),
        }
    }
}

struct TextHandler {
    body: String,
    content_type: Option<String>,
}

struct JsonValueHandler {
    value: serde_json::Value,
}

struct TemplateHandler {
    template: String,
    context: serde_json::Value,
    content_type: Option<String>,
}

struct EchoHandler;
struct FileHandler {
    path: std::path::PathBuf,
}
struct MetricsHandler;

#[async_trait]
impl Handler for TextHandler {
    async fn handle(&self, _req: Request, _ctx: Arc<AppState>) -> Result<Response> {
        let mut response = Response::text(StatusCode::Ok, self.body.clone());
        if let Some(content_type) = &self.content_type {
            response.set_header("content-type", content_type.clone());
        }
        Ok(response)
    }
}

#[async_trait]
impl Handler for JsonValueHandler {
    async fn handle(&self, _req: Request, _ctx: Arc<AppState>) -> Result<Response> {
        Response::json(&self.value)
    }
}

#[async_trait]
impl Handler for TemplateHandler {
    async fn handle(&self, _req: Request, ctx: Arc<AppState>) -> Result<Response> {
        let rendered = ctx.renderer.render(&self.template, &self.context)?;
        let mut response = Response::text(StatusCode::Ok, rendered);
        if let Some(content_type) = &self.content_type {
            response.set_header("content-type", content_type.clone());
        } else {
            response.set_header("content-type", "text/html; charset=utf-8");
        }
        Ok(response)
    }
}

#[async_trait]
impl Handler for EchoHandler {
    async fn handle(&self, req: Request, _ctx: Arc<AppState>) -> Result<Response> {
        Ok(Response::text(
            StatusCode::Ok,
            String::from_utf8_lossy(&req.body).to_string(),
        ))
    }
}

#[async_trait]
impl Handler for FileHandler {
    async fn handle(&self, _req: Request, _ctx: Arc<AppState>) -> Result<Response> {
        let bytes = tokio::fs::read(&self.path).await?;
        let mut response = Response::new(StatusCode::Ok);
        response.body = bytes;
        Ok(response)
    }
}

#[async_trait]
impl Handler for MetricsHandler {
    async fn handle(&self, _req: Request, ctx: Arc<AppState>) -> Result<Response> {
        let mut response = Response::text(StatusCode::Ok, ctx.metrics.render());
        response.set_header("content-type", "text/plain; version=0.0.4");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_state() {
        let state = AppState::test();
        assert!(state.ready.load(Ordering::SeqCst));
    }
}
