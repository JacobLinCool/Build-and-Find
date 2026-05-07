use nanoweb::config::{parse_args, Config};
use nanoweb::server::Server;

fn main() {
    let args = parse_args();

    if args.version {
        println!("nanoweb {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let config = Config::load(&args).unwrap_or_else(|e| {
        eprintln!("Error loading config: {e}");
        std::process::exit(1);
    });

    // Initialize tracing
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Starting nanoweb");

    let mut server = Server::new(config);

    // Register some example routes
    server
        .router_mut()
        .route(nanoweb::Method::Get, "/", |_req| {
            nanoweb::Response::html("<h1>Welcome to nanoweb</h1>")
        });

    server
        .router_mut()
        .route(nanoweb::Method::Get, "/hello/:name", |req| {
            let name = req
                .params
                .get("name")
                .map(|s| s.as_str())
                .unwrap_or("World");
            nanoweb::Response::text(format!("Hello, {name}!"))
        });

    server
        .router_mut()
        .route(nanoweb::Method::Post, "/echo", |req| {
            let body = req.body_str().to_string();
            nanoweb::Response::text(body)
        });

    // Run the server
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(server.run()).unwrap_or_else(|e| {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    });
}
