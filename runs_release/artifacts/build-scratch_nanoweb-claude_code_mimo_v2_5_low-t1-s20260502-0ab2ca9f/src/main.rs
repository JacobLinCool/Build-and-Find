use std::collections::HashMap;
use std::sync::Arc;

use nanoweb::config::Config;
use nanoweb::handler::{simple_handler, HandlerFn};
use nanoweb::http::{Method, Response};
use nanoweb::middleware::cors::CorsMiddleware;
use nanoweb::middleware::gzip::GzipMiddleware;
use nanoweb::middleware::logging::LoggingMiddleware;
use nanoweb::middleware::rate_limit::RateLimitMiddleware;
use nanoweb::router::Router;
use nanoweb::server::Server;

#[tokio::main]
async fn main() {
    let cli = nanoweb::config::CliArgs::parse();

    if cli.version {
        println!("nanoweb {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let config_path = cli.config.as_deref().unwrap_or("./nanoweb.toml");
    let config = if std::path::Path::new(config_path).exists() {
        match Config::from_file(config_path) {
            Ok(c) => c.with_cli_overrides(&cli),
            Err(e) => {
                eprintln!("Error loading config: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let mut defaults = Config::default();
        defaults = defaults.with_cli_overrides(&cli);
        defaults
    };

    // Initialize tracing
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.log_level));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Build router
    let mut router = Router::new();
    let mut handlers: HashMap<usize, HandlerFn> = HashMap::new();
    let mut route_idx = 0;

    // Add configured routes
    for route in &config.routes {
        if !route.enabled {
            continue;
        }
        let methods: Vec<Method> = route
            .methods
            .iter()
            .filter_map(|m| Method::from_str(m))
            .collect();
        if methods.is_empty() {
            continue;
        }

        let handler = create_default_handler(&route.path);
        router.add_route(&route.path, methods, vec![], &route.path);
        handlers.insert(route_idx, handler);
        route_idx += 1;
    }

    // Add default routes if none configured
    if config.routes.is_empty() {
        router.add_route("/", vec![Method::Get], vec![], "index");
        handlers.insert(
            route_idx,
            simple_handler(|_req| {
                let mut resp = Response::new();
                resp.set_html("<h1>Welcome to nanoweb</h1>");
                resp
            }),
        );
        route_idx += 1;

        router.add_route("/hello/:name", vec![Method::Get], vec![], "hello");
        handlers.insert(
            route_idx,
            simple_handler(|req| {
                let name = req
                    .params
                    .get("name")
                    .map(|s| s.as_str())
                    .unwrap_or("World");
                let mut resp = Response::new();
                resp.set_text(&format!("Hello, {}!", name));
                resp
            }),
        );
    }

    // Create server
    let mut server = Server::new(config.clone());

    // Add global middleware
    if config.middleware.logging {
        server.add_global_middleware(Arc::new(LoggingMiddleware::new()));
    }
    if config.middleware.gzip {
        server.add_global_middleware(Arc::new(GzipMiddleware::new()));
    }
    if let Some(rps) = config.middleware.rate_limit {
        server.add_global_middleware(Arc::new(RateLimitMiddleware::new(rps)));
    }
    if config.middleware.cors {
        server.add_global_middleware(Arc::new(CorsMiddleware::new(
            config.cors.allowed_origins.clone(),
            config.cors.allow_credentials,
        )));
    }

    // Static files
    if let Some(ref dir) = config.static_dir {
        server.set_static_files(dir);
    }

    server.set_router(router, handlers);

    // Run
    if let Err(e) = server.run().await {
        tracing::error!("Server error: {}", e);
        std::process::exit(1);
    }
}

fn create_default_handler(path: &str) -> HandlerFn {
    let path = path.to_string();
    simple_handler(move |_req| {
        let mut resp = Response::new();
        resp.set_text(&format!("Route: {}", path));
        resp
    })
}
