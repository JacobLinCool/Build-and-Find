# nanoweb

A self-contained HTTP/1.1 web framework and server written in Rust from scratch.

## Features

- Custom HTTP/1.1 parser (no hyper/axum)
- Trie-based router with path parameters and wildcards
- Composable middleware stack
- Body parsers: JSON, URL-encoded, multipart
- Static file serving with ETag/304 caching
- Mustache-like template engine
- Prometheus metrics endpoint
- Health check endpoints (`/healthz`, `/readyz`)
- Session cookies with HMAC-SHA256 signing
- TLS support via rustls (feature-gated)
- Structured logging with request IDs
- Graceful shutdown on SIGINT/SIGTERM
- TOML configuration file

## Quickstart

```bash
cargo build --release
./target/release/nanoweb --listen 0.0.0.0:8080
```

With a config file:

```bash
./target/release/nanoweb --config nanoweb.toml
```

Example `nanoweb.toml`:

```toml
listen = "0.0.0.0:8080"
log_level = "info"

[[middleware]]
type = "Logging"

[[middleware]]
type = "Gzip"
min_size = 1024

[[static_files]]
route = "/static"
dir = "./public"
```

## Routing

The router supports exact paths, named parameters (`:name`), and wildcards (`*name`).

```rust
use nanoweb::{Router, Request, Response};
use nanoweb::middleware::FnHandler;
use std::sync::Arc;

let mut router = Router::new();

// Exact path
router.get("/hello", Arc::new(FnHandler::new(|_req: Request| async {
    Ok(Response::text("Hello, World!"))
})));

// Named parameter
router.get("/users/:id", Arc::new(FnHandler::new(|req: Request| async move {
    let id = req.params.get("id").cloned().unwrap_or_default();
    Ok(Response::text(format!("User: {}", id)))
})));

// Wildcard
router.get("/files/*path", Arc::new(FnHandler::new(|req: Request| async move {
    let path = req.params.get("path").cloned().unwrap_or_default();
    Ok(Response::text(format!("File: {}", path)))
})));

// POST with JSON body
router.post("/api/data", Arc::new(FnHandler::new(|req: Request| async move {
    let body = String::from_utf8_lossy(&req.body).to_string();
    Response::json(&serde_json::json!({"received": body}))
})));

// Query parameters
router.get("/search", Arc::new(FnHandler::new(|req: Request| async move {
    let params = req.query_params();
    let q = params.get("q").cloned().unwrap_or_default();
    Ok(Response::text(format!("Search: {}", q)))
})));
```

HTTP methods: `get`, `post`, `put`, `delete`, `patch`. Use `route(method, path, handler, middleware)` for other methods or per-route middleware.

## Middleware

Middleware wraps request handling and can inspect/modify requests and responses. Add global middleware via `router.add_global_middleware()`.

### Logging

```rust
use nanoweb::middleware::logging::LoggingMiddleware;

router.add_global_middleware(Arc::new(LoggingMiddleware));
```

Logs method, path, status, duration, remote address, and request ID using `tracing`.

### Gzip Compression

```rust
use nanoweb::middleware::gzip::GzipMiddleware;

router.add_global_middleware(Arc::new(GzipMiddleware::with_min_size(1024)));
```

Compresses responses when the client sends `Accept-Encoding: gzip` and the body exceeds `min_size` bytes.

### CORS

```rust
use nanoweb::middleware::cors::{CorsConfig, CorsMiddleware};

let config = CorsConfig {
    allowed_origins: vec!["https://example.com".into()],
    allow_credentials: true,
    ..CorsConfig::default()
};
router.add_global_middleware(Arc::new(CorsMiddleware::new(config)));
```

Handles preflight OPTIONS requests and adds `Access-Control-*` headers.

### Rate Limiting

```rust
use nanoweb::middleware::rate_limit::RateLimitMiddleware;

// Token bucket: 100 requests/second capacity, 1000 burst
router.add_global_middleware(Arc::new(RateLimitMiddleware::with_capacity(100.0, 1000.0)));
```

Per-IP token bucket. Returns 429 when exhausted.

### Basic Auth

```rust
use nanoweb::middleware::basic_auth::BasicAuthMiddleware;

router.add_global_middleware(Arc::new(BasicAuthMiddleware::single(
    "My Realm", "admin", "secret",
)));
```

Returns 401 with `WWW-Authenticate` header if credentials are missing or invalid.

### CSRF Protection

```rust
use nanoweb::middleware::csrf::CsrfMiddleware;

router.add_global_middleware(Arc::new(CsrfMiddleware::new()));
```

Double-submit cookie pattern. Safe methods (GET, HEAD, OPTIONS) bypass the check. State-changing requests must include `x-csrf-token` header matching the `csrf_token` cookie.

## TLS Setup

TLS requires the `tls` feature (enabled by default):

```toml
# nanoweb.toml
[tls]
listen = "0.0.0.0:8443"
cert = "./cert.pem"
key = "./key.pem"
```

Generate a self-signed certificate for testing:

```bash
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes
```

Build without TLS:

```bash
cargo build --release --no-default-features
```

## Metrics

The Prometheus metrics endpoint is available at `/metrics` by default (configurable via `metrics_path` in config or `--metrics` CLI flag).

Exposed metrics:

- `http_requests_total{method, status_class, route}` — request count by method, status class (2xx/4xx/5xx), and route
- `http_request_duration_seconds{method, route}` — request latency histogram

```toml
# nanoweb.toml
metrics_path = "/metrics"
```

## Health Endpoints

- `GET /healthz` — liveness probe (200 if live, 503 if not)
- `GET /readyz` — readiness probe (200 if ready, 503 if not)

## Template Engine

A Mustache-like renderer supporting variable substitution, conditional sections, and iteration.

```rust
use nanoweb::template::{Renderer, Value, Context};

let renderer = Renderer::new();
let mut ctx = Context::new();
ctx.insert("name".into(), Value::Str("Alice".into()));
ctx.insert("admin".into(), Value::Bool(true));

let items = vec![{
    let mut c = Context::new();
    c.insert("item".into(), Value::Str("foo".into()));
    c
}];
ctx.insert("list".into(), Value::List(items));

let output = renderer.render(
    "Hello, {{name}}! {{#admin}}You are an admin.{{/admin}} Items: {{#list}}{{item}} {{/list}}",
    &ctx,
);
```

Syntax:

| Tag | Description |
|-----|-------------|
| `{{name}}` | HTML-escaped variable substitution |
| `{{#section}}...{{/section}}` | Render if truthy (bool true, non-empty string/list) |
| `{{^section}}...{{/section}}` | Render if falsy (bool false, empty, or absent) |
| `{{#list}}...{{/list}}` | Iterate over a list; inner context is each item |

Variables are always HTML-escaped (`<`, `>`, `&`, `"`, `'`).

## Session Management

```rust
use nanoweb::session::{SessionManager, InMemorySessionStore};
use std::sync::Arc;

let store = Arc::new(InMemorySessionStore::new());
let manager = SessionManager::new(store, "my-secret-key", "session");

// In a handler:
// manager.get_session(&req) -> Option<String>   (returns session ID if valid cookie)
// manager.create_session(&mut resp, session_id) -> sets signed cookie
// manager.delete_session(&mut resp)             -> clears cookie
```

Session cookies are signed with HMAC-SHA256. The cookie value is `session_id.hex_signature`.

## Static Files

```rust
use nanoweb::static_files::StaticFileHandler;
use nanoweb::middleware::FnHandler;
use std::sync::Arc;

let handler = Arc::new(StaticFileHandler::new("./public", "/static"));
let h = handler.clone();
router.route(
    "GET",
    "/static/*path",
    Arc::new(FnHandler::new(move |req| {
        let h = h.clone();
        async move { h.serve(&req).await }
    })),
    vec![],
);
```

Features: MIME type detection, weak ETags (`W/"mtime-size"`), conditional `If-None-Match` → 304, HEAD support, directory traversal protection.

Or configure in TOML:

```toml
[[static_files]]
route = "/assets"
dir = "./assets"
```

## Architecture

```
nanoweb/
├── src/
│   ├── http/
│   │   ├── parser.rs      # HTTP/1.1 request parser (AsyncBufRead)
│   │   └── types.rs       # Request, Response, Method, HttpVersion
│   ├── router/
│   │   ├── trie.rs        # Trie-based router with params/wildcards
│   │   └── mod.rs         # Router, dispatch()
│   ├── middleware/
│   │   ├── mod.rs         # Handler, Middleware, Next, FnHandler traits
│   │   ├── logging.rs
│   │   ├── gzip.rs
│   │   ├── rate_limit.rs
│   │   ├── cors.rs
│   │   ├── csrf.rs
│   │   └── basic_auth.rs
│   ├── body/
│   │   ├── json.rs        # JSON body parsing
│   │   ├── urlencoded.rs  # URL-encoded form parsing
│   │   └── multipart.rs   # Multipart form parsing
│   ├── server/mod.rs      # Accept loop, connection handling, graceful shutdown
│   ├── config.rs          # TOML config deserialization
│   ├── cli.rs             # clap CLI argument parsing
│   ├── error.rs           # NanowebError with HTTP status mapping
│   ├── metrics/mod.rs     # Prometheus IntCounterVec + HistogramVec
│   ├── health/mod.rs      # /healthz and /readyz handlers
│   ├── session/mod.rs     # HMAC-SHA256 session cookies
│   ├── static_files/mod.rs
│   ├── template/mod.rs    # Mustache-like template renderer
│   ├── tls/mod.rs         # rustls TLS config loader (feature-gated)
│   ├── lib.rs
│   └── main.rs
└── tests/
    └── integration_test.rs
```

**Request lifecycle:**

1. `TcpStream` accepted → `handle_connection()`
2. `http::parser::read_request()` parses method, path, headers, body
3. `Router::dispatch()` walks the trie, populates `req.params`, returns `(Next, Request)`
4. `Next::run()` calls middleware chain in order, then the route handler
5. `Response::serialize()` writes status line, headers, body back to the stream
6. `X-Request-Id` header is set on every response; `Connection: close` or keep-alive per HTTP version

**Middleware chain:**

Each `Middleware::call()` receives `req` and a `Next` struct. Calling `next.run(req)` advances to the next middleware or the final handler. Middleware can short-circuit by returning a response directly without calling `next.run()`.

## CLI Reference

```
Usage: nanoweb [OPTIONS]

Options:
  -l, --listen <ADDR>       Listen address [default: 0.0.0.0:8080]
  -c, --config <FILE>       Config file path [default: ./nanoweb.toml]
      --tls-cert <FILE>     TLS certificate PEM file
      --tls-key <FILE>      TLS private key PEM file
      --metrics <PATH>      Metrics endpoint path [default: /metrics]
      --static-dir <DIR>    Serve static files from DIR at /static
      --log-level <LEVEL>   Log level [default: info]
  -h, --help                Print help
```

## Running Tests

```bash
cargo test
```
