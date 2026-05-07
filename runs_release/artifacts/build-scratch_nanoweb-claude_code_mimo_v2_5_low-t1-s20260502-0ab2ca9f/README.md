# nanoweb

A self-contained HTTP/1.1 web framework and server for Rust. nanoweb provides its own HTTP parser, a trie-based router, middleware stack, body parsers, TLS support, and more — all in a single crate.

## Quickstart

```rust
use nanoweb::http::{Request, Response, Method};
use nanoweb::handler::simple_handler;
use nanoweb::server::Server;
use nanoweb::config::Config;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let mut router = nanoweb::router::Router::new();
    let mut handlers = std::collections::HashMap::new();

    router.add_route("/hello/:name", vec![Method::Get], vec![], "hello");
    handlers.insert(0, simple_handler(|req| {
        let name = req.params.get("name").unwrap();
        let mut resp = Response::new();
        resp.set_text(&format!("Hello, {}!", name));
        resp
    }));

    let config = Config { listen: "127.0.0.1:8080".into(), ..Config::default() };
    let mut server = Server::new(config);
    server.set_router(router, handlers);
    server.run().await.unwrap();
}
```

## Binary Usage

```bash
# Run with defaults (127.0.0.1:8080)
cargo run --release

# Run with custom config
cargo run --release -- --config myapp.toml --listen 0.0.0.0:3000

# Run with TLS
cargo run --release -- --tls-cert cert.pem --tls-key key.pem

# Show help
cargo run --release -- --help
```

## Configuration

Create a `nanoweb.toml`:

```toml
listen = "0.0.0.0:8080"
log_level = "info"
graceful_timeout_s = 30
session_secret = "your-secret-here"

[middleware]
logging = true
gzip = true
rate_limit = 100.0  # requests per second per IP
cors = true
csrf = false

[cors]
allowed_origins = ["*"]
allow_credentials = false

[[routes]]
path = "/users"
methods = ["GET", "POST"]

[[routes]]
path = "/users/:id"
methods = ["GET", "PUT", "DELETE"]
```

CLI flags override TOML values.

## Routing

nanoweb uses a trie-based router with O(path-length) lookup:

- **Literal segments**: `/users/list`
- **Path parameters**: `/users/:id` — captured as `req.params["id"]`
- **Wildcards**: `/static/*path` — captures the rest of the path

Supported HTTP methods: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS.

- **HEAD** falls back to the matching GET handler with the body suppressed.
- **OPTIONS** auto-lists allowed methods for the route.

## Middleware

Middleware implements the `Middleware` trait with `before` and `after` hooks:

```rust
pub trait Middleware: Send + Sync {
    fn before(&self, req: &mut Request) -> Option<Response> { None }
    fn after(&self, _req: &Request, _res: &mut Response) {}
}
```

Standard middleware:
- **LoggingMiddleware** — structured request/response logging via `tracing`
- **GzipMiddleware** — compresses responses when client accepts gzip
- **RateLimitMiddleware** — token bucket rate limiting per client IP
- **CorsMiddleware** — configurable CORS headers
- **CsrfMiddleware** — double-submit cookie CSRF protection
- **BasicAuthMiddleware** — HTTP Basic authentication

## Body Parsers

Parse request bodies with typed deserialization:

```rust
#[derive(serde::Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

let user: CreateUser = req.body_as()?;
```

Supported content types:
- `application/json`
- `application/x-www-form-urlencoded`
- `multipart/form-data`

## Static Files

Serve static files from a directory with ETag-based caching:

```toml
static_dir = "./public"
```

Directory traversal is rejected. Files are served with correct MIME types and `304 Not Modified` support via `If-None-Match`.

## TLS

Enable TLS with rustls:

```bash
cargo run --release -- --tls-cert cert.pem --tls-key key.pem
```

TLS is enabled by default via the `tls` Cargo feature. Disable it with:

```bash
cargo build --no-default-features
```

## Sessions

```rust
use nanoweb::session::{SessionManager, InMemorySessionStore};
use std::sync::Arc;

let store = Arc::new(InMemorySessionStore::new());
let manager = SessionManager::new("secret-key", store);

// Sign a session ID
let signed = manager.sign("session-123");

// Verify and retrieve
if let Some(session) = manager.get_session(&signed) {
    println!("User: {}", session.get("user").unwrap());
}
```

## Metrics

Prometheus-compatible metrics are exposed at `/metrics` (configurable):

- `http_requests_total` — counter by method, status class, and route
- `http_request_duration_seconds` — histogram by route
- `process_uptime_seconds` — uptime gauge

## Template Engine

Minimal mustache-style templates:

```rust
use nanoweb::template::Renderer;
use serde_json::json;

let renderer = Renderer::new();
let output = renderer.render(
    "Hello, {{name}}! {{#items}}{{.}} {{/items}}",
    &json!({"name": "World", "items": ["a", "b", "c"]}),
);
// "Hello, World! a b c "
```

Features: `{{variable}}`, `{{#section}}...{{/section}}`, `{{^inverted}}...{{/inverted}}`, dotted paths (`{{user.name}}`).

## Health Endpoints

- `/healthz` — returns 200 OK if the server is running
- `/readyz` — returns 200 OK if ready to accept traffic, 503 during startup/shutdown

## Graceful Shutdown

On SIGINT/SIGTERM, the server stops accepting new connections, waits for in-flight requests to complete (bounded by `graceful_timeout_s`, default 30s), and exits cleanly.

## Architecture

```
src/
├── lib.rs          # Module declarations
├── main.rs         # Binary entry point
├── error.rs        # NanowebError enum
├── config.rs       # TOML config + CLI args
├── http/
│   ├── mod.rs      # Re-exports
│   ├── types.rs    # Request, Response, Headers, Method, StatusCode
│   └── parser.rs   # HTTP/1.1 request parser (byte-level)
├── router/
│   └── mod.rs      # Trie-based router
├── handler.rs      # HandlerFn type alias
├── middleware/
│   ├── mod.rs      # Middleware trait
│   ├── logging.rs  # Request logging
│   ├── gzip.rs     # Gzip compression
│   ├── rate_limit.rs # Token bucket rate limiting
│   ├── cors.rs     # CORS headers
│   ├── csrf.rs     # CSRF double-submit cookie
│   └── basic_auth.rs # HTTP Basic auth
├── body/
│   └── mod.rs      # JSON, form, multipart parsers
├── static_files.rs # Static file serving with ETag
├── tls.rs          # TLS via rustls
├── session.rs      # Session management with HMAC-SHA256
├── metrics.rs      # Prometheus metrics
├── health.rs       # Health/readyz endpoints
├── template/
│   └── mod.rs      # Mustache-style template engine
└── server.rs       # Async TCP server with graceful shutdown
```

## License

MIT
