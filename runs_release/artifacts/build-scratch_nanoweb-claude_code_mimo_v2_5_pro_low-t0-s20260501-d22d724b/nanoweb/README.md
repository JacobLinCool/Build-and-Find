# nanoweb

A self-contained HTTP/1.1 web framework and server built from scratch in Rust.

## Quickstart

```bash
# Build
cargo build --release

# Run with default config
./target/release/nanoweb

# Run with custom config
./target/release/nanoweb --config nanoweb.toml --listen 0.0.0.0:3000

# Run with TLS
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

Create a `nanoweb.toml` configuration file:

```toml
listen = "127.0.0.1:8080"
log_level = "info"
graceful_timeout_s = 30
session_secret = "your-secret-key-here"
```

## Routing

Routes support path parameters, wildcards, and all standard HTTP methods:

```rust
use nanoweb::{Router, Request, Response, Method};

let mut router = Router::new();

// Simple GET route
router.get("/hello", |_req| {
    Box::pin(async { Response::ok().with_body("Hello, World!") })
});

// Path parameters
router.get("/users/:id", |mut req| {
    Box::pin(async move {
        let id = req.params.get("id").cloned().unwrap_or_default();
        Response::ok().with_json(&serde_json::json!({"user_id": id}))
    })
});

// Wildcard routes
router.get("/static/*path", |mut req| {
    Box::pin(async move {
        let path = req.params.get("path").cloned().unwrap_or_default();
        Response::ok().with_body(format!("Serving: {path}"))
    })
});

// POST with body parsing
router.post("/users", |mut req| {
    Box::pin(async move {
        let body: serde_json::Value = req.body_as().unwrap_or_default();
        Response::new(201).with_json(&body)
    })
});
```

Supported methods: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`

- `HEAD` automatically falls back to the matching `GET` handler with body suppressed
- `OPTIONS` auto-lists the allowed methods for the route

## Middleware

Middleware implements the `Middleware` trait with `before` (can short-circuit) and `after` (observe/mutate) hooks:

```rust
use nanoweb::middleware::Middleware;

struct MyMiddleware;

impl Middleware for MyMiddleware {
    fn before(&self, req: &mut Request) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        // Return Some(Response) to short-circuit, None to continue
        Box::pin(async { None })
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        // Observe or mutate the response
    }
}
```

### Built-in Middleware

| Middleware | Description |
|---|---|
| `LoggingMiddleware` | Structured request/response logging via `tracing` |
| `GzipMiddleware` | Response compression controlled by `Accept-Encoding` |
| `RateLimitMiddleware` | Token bucket rate limiting per client IP |
| `CorsMiddleware` | Configurable CORS with preflight support |
| `CsrfMiddleware` | Double-submit cookie CSRF protection |
| `BasicAuthMiddleware` | HTTP Basic Authentication guard |

## Body Parsers

Typed access to request bodies:

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

router.post("/users", |mut req| {
    Box::pin(async move {
        let user: CreateUser = req.body_as().unwrap();
        Response::new(201).with_json(&user)
    })
});
```

Supported content types:
- `application/json`
- `application/x-www-form-urlencoded`
- `multipart/form-data` (via `MultipartParser`)

## Static Files

Serve static files with ETag support and 304 responses:

```rust
let server = StaticFileServer::new("./public");
```

- Generates weak ETags based on mtime + size
- Returns `304 Not Modified` for matching `If-None-Match` headers
- Rejects directory traversal attempts
- Serves `index.html` for directory requests

## TLS

TLS is available via the `tls` feature (enabled by default) using rustls:

```bash
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

Or in config:

```toml
tls_cert = "cert.pem"
tls_key = "key.pem"
tls_listen = "0.0.0.0:8443"
```

## Sessions

Per-session data with HMAC-SHA256 signed cookies:

```rust
use nanoweb::session::{InMemorySessionStore, SessionStore};

let store = InMemorySessionStore::new("secret-key");
let session_id = store.create();

let mut data = HashMap::new();
data.insert("user".to_string(), "alice".to_string());
store.set(&session_id, data);
```

## Metrics

Prometheus text exposition format at `/metrics` (configurable):

```
# HELP http_requests_total Total HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="GET",status="2xx",route="/api/test"} 42

# HELP http_request_duration_seconds Request duration in seconds
# TYPE http_request_duration_seconds histogram
http_request_duration_seconds_bucket{le="0.1"} 38
```

## Health Endpoints

- `/healthz` - Returns 200 OK if the server is up
- `/readyz` - Returns 200 OK if ready to accept traffic (post-startup, pre-shutdown)

## Template Engine

Minimal mustache-like templates:

```rust
use nanoweb::template::Renderer;
use serde_json::Value;

let renderer = Renderer::new();
let mut ctx = HashMap::new();
ctx.insert("name".to_string(), Value::String("World".to_string()));

let html = renderer.render("Hello, {{name}}!", &ctx).unwrap();
// "Hello, World!"
```

Features:
- `{{variable}}` - Variable substitution with HTML escaping
- `{{#list}}...{{/list}}` - Section iteration
- `{{^absent}}...{{/absent}}` - Inverted sections (render when value is absent/false/empty)

## CLI Flags

| Flag | Default | Description |
|---|---|---|
| `--listen` | `127.0.0.1:8080` | Address and port |
| `--config` | `./nanoweb.toml` | Config file path |
| `--tls-cert` | - | TLS certificate (PEM) |
| `--tls-key` | - | TLS private key (PEM) |
| `--metrics` | `/metrics` | Metrics endpoint path |
| `--static-dir` | - | Static files directory |
| `--log-level` | `info` | Log level |
| `--version` | - | Show version |
| `--help` | - | Show help |

Every CLI flag has a matching TOML key; CLI flags override TOML values.

## Architecture

```
nanoweb/
  src/
    http/         # HTTP/1.1 parser and types
      parser.rs   # Byte-level request parser (chunked, content-length)
      types.rs    # Request, Response, Method, StatusCodeClass
    router.rs     # Trie-based router with params and wildcards
    middleware/   # Middleware trait and standard library
      logging.rs  # Structured request logging
      gzip.rs     # Response compression
      rate_limit.rs # Token bucket rate limiting
      cors.rs     # CORS headers
      csrf.rs     # CSRF double-submit cookie
      basic_auth.rs # HTTP Basic Auth
    body/         # Body parsers
      json.rs     # application/json
      urlencoded.rs # application/x-www-form-urlencoded
      multipart.rs # multipart/form-data
    static_files.rs # Static file serving with ETag
    tls.rs        # TLS via rustls
    session.rs    # Session store with HMAC-signed cookies
    metrics.rs    # Prometheus metrics
    health.rs     # Health/readiness endpoints
    template.rs   # Mustache-like template engine
    config.rs     # TOML + CLI configuration
    cli.rs        # CLI argument parsing
    server.rs     # TCP accept loop and connection handling
    error.rs      # Crate-wide error enum
    lib.rs        # Public API
    main.rs       # Binary entry point
```

## License

MIT
