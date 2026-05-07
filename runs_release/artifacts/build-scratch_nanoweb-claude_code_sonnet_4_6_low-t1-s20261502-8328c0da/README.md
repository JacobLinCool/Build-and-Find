# nanoweb

A lightweight, async HTTP/1.1 web framework written in Rust.

## Quickstart

```bash
# Run with defaults (127.0.0.1:8080, nanoweb.toml)
nanoweb

# Custom address and config
nanoweb --listen 0.0.0.0:9090 --config myapp.toml

# With TLS
nanoweb --tls-cert cert.pem --tls-key key.pem

# With static files
nanoweb --static-dir ./public
```

## Configuration (nanoweb.toml)

```toml
listen = "127.0.0.1:8080"
log_level = "info"
metrics = "/metrics"
graceful_timeout_s = 30
session_secret = "change-me-in-production"

[cors]
allowed_origins = ["https://example.com"]
allow_credentials = false

[rate_limit]
enabled = true
requests_per_second = 10.0
burst = 20.0

[[routes]]
method = "GET"
path = "/hello"
response = "Hello, World!"
status = 200
content_type = "text/plain"
```

## Routing

Routes support literal paths, path parameters (`:name`), and wildcards (`*path`):

```rust
use nanoweb::{Method, Response, router::Router};

let mut router = Router::new();

// Literal path
router.get("/hello", |_req| async move {
    Ok(Response::text("Hello!"))
});

// Path parameter
router.get("/users/:id", |req| async move {
    let id = req.params.get("id").cloned().unwrap_or_default();
    Ok(Response::text(format!("User: {}", id)))
});

// Wildcard
router.get("/static/*path", |req| async move {
    let path = req.params.get("path").cloned().unwrap_or_default();
    Ok(Response::text(format!("File: {}", path)))
});

// POST
router.post("/api/data", |req| async move {
    let val: serde_json::Value = serde_json::from_slice(&req.body)?;
    Response::json(&val)
});
```

### HTTP Method Handling

- **HEAD**: falls back to GET handler with body suppressed
- **OPTIONS**: auto-responds with allowed methods for the path
- Unsupported methods return 405 Method Not Allowed with `Allow` header

## Middleware

Middleware is applied in registration order:

```rust
use nanoweb::middleware::{
    MiddlewareStack,
    logging::RequestLogger,
    compression::GzipCompression,
    cors::CorsMiddleware,
    rate_limit::RateLimiter,
    auth::BasicAuth,
    csrf::CsrfMiddleware,
};

let mut middleware = MiddlewareStack::new();
middleware.add(RequestLogger);
middleware.add(GzipCompression::default());
middleware.add(CorsMiddleware::new(vec!["https://example.com".to_string()]));
middleware.add(RateLimiter::new(10.0, 20.0));
middleware.add(BasicAuth::new("user", "password"));
middleware.add(CsrfMiddleware::new());
```

### Standard Middleware

| Middleware | Description |
|-----------|-------------|
| `RequestLogger` | Structured request logging with duration |
| `GzipCompression` | Gzip responses when client sends `Accept-Encoding: gzip` |
| `RateLimiter` | Token bucket rate limiting per IP address |
| `CorsMiddleware` | CORS headers with configurable allowed origins |
| `CsrfMiddleware` | Double-submit cookie CSRF protection |
| `BasicAuth` | HTTP Basic Authentication guard |

## Body Parsing

Use `BodyParser` trait for automatic content-type detection:

```rust
use nanoweb::body::BodyParser;

router.post("/api/users", |req| async move {
    // Auto-detects content type
    let user: MyStruct = req.body_as()?;
    
    // Explicit JSON
    let json: serde_json::Value = req.body_as_json()?;
    
    // Form data
    let form: HashMap<String, String> = req.body_as_form()?;
    
    Ok(Response::text("ok"))
});
```

For multipart/form-data:

```rust
use nanoweb::body::parse_multipart;

router.post("/upload", |req| async move {
    let fields = parse_multipart(&req).await?;
    for field in fields {
        println!("Field: {} ({:?})", field.name, field.filename);
    }
    Ok(Response::text("uploaded"))
});
```

## TLS

TLS support requires the `tls` feature (enabled by default). Provide PEM files:

```bash
nanoweb --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
```

Or in `nanoweb.toml`:

```toml
tls_cert = "/path/to/cert.pem"
tls_key = "/path/to/key.pem"
```

To disable TLS support at compile time:

```bash
cargo build --no-default-features
```

## Metrics

Prometheus metrics are served at `/metrics` (configurable):

```bash
curl http://localhost:8080/metrics
```

Available metrics:
- `nanoweb_requests_total{method, status, route}` - request counter
- `nanoweb_request_duration_seconds{method, route}` - request duration histogram

## Health Endpoints

- `GET /healthz` - returns 200 if server is alive
- `GET /readyz` - returns 200 if server is ready (after startup, before graceful shutdown)

## Sessions

```rust
use nanoweb::session::{MemorySessionStore, SessionManager};
use std::sync::Arc;
use std::time::Duration;

let store = Arc::new(MemorySessionStore::new(Duration::from_secs(3600)));
let manager = SessionManager::new(store, "hmac-secret");

// In a handler:
router.get("/profile", move |req| {
    let manager = manager.clone();
    async move {
        let session = manager.get_session(req.headers.get("cookie")).await;
        // ... use session ...
        Ok(Response::text("ok"))
    }
});
```

Sessions are HMAC-SHA256 signed cookies. The signing scheme:
`HMAC-SHA256(secret, session_id + ":" + data)`

## Templates

Mustache-like template engine:

```rust
use nanoweb::template::Renderer;
use serde_json::json;

let html = Renderer::render(
    "Hello, {{name}}! You have {{count}} messages.",
    &json!({"name": "Alice", "count": 5})
)?;

// Sections (conditionals)
let html = Renderer::render(
    "{{#logged_in}}Welcome back!{{/logged_in}}{{^logged_in}}Please log in.{{/logged_in}}",
    &json!({"logged_in": true})
)?;

// Lists
let html = Renderer::render(
    "{{#items}}* {{name}}\n{{/items}}",
    &json!({"items": [{"name": "foo"}, {"name": "bar"}]})
)?;
```

Supported tags:
- `{{variable}}` - variable interpolation
- `{{#section}}...{{/section}}` - conditional/loop section
- `{{^section}}...{{/section}}` - inverted section (renders when falsy/empty)
- `{{.}}` - current item in list iteration

## CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--listen` | `127.0.0.1:8080` | Listen address |
| `--config` | `nanoweb.toml` | Config file path |
| `--tls-cert` | - | TLS certificate (PEM) |
| `--tls-key` | - | TLS private key (PEM) |
| `--metrics` | `/metrics` | Metrics endpoint path |
| `--static-dir` | - | Static files directory |
| `--log-level` | `info` | Log level |
| `--version` | - | Print version |
| `--help` | - | Print help |

CLI flags override TOML configuration.

## Architecture Overview

```
src/
├── lib.rs          - Public API surface
├── main.rs         - CLI entry point
├── error.rs        - NanowebError enum (crate-wide error type)
├── config.rs       - Config struct, TOML loading, CLI overrides
├── cli.rs          - clap CLI definition
├── http/
│   ├── mod.rs      - Re-exports
│   ├── parser.rs   - Byte-level HTTP/1.1 parser (independent of router/middleware)
│   └── types.rs    - Request, Response, Method, StatusCode, Headers
├── router/
│   └── mod.rs      - Trie-based router (O(path-length) lookup)
├── middleware/
│   ├── mod.rs      - Middleware trait + MiddlewareStack
│   ├── logging.rs  - Request logging
│   ├── compression.rs - Gzip compression
│   ├── rate_limit.rs  - Token bucket rate limiting
│   ├── cors.rs     - CORS
│   ├── csrf.rs     - CSRF double-submit
│   └── auth.rs     - Basic auth
├── body/
│   └── mod.rs      - Body parsers (JSON, form, multipart)
├── static_files/
│   └── mod.rs      - Static file serving with ETag/304 support
├── tls/
│   └── mod.rs      - TLS via rustls (feature-gated)
├── session/
│   └── mod.rs      - SessionStore trait + in-memory impl + HMAC signing
├── metrics/
│   └── mod.rs      - Prometheus metrics registry
├── health/
│   └── mod.rs      - /healthz and /readyz state
├── template/
│   └── mod.rs      - Mustache-like template renderer
└── server/
    └── mod.rs      - Accept loop, connection handling, graceful shutdown
```

### Key Design Decisions

- **HTTP Parser Independence**: `http::parser` has no dependencies on router, middleware, or body modules
- **Trie Router**: O(path-length) lookup using an enum for node types (Literal, Param, Wildcard)
- **Object-safe Middleware**: Single `Middleware` trait with `Arc<dyn Middleware>` for middleware stacks
- **Async with Tokio**: Accept loop and per-connection handling are explicitly split
- **Graceful Shutdown**: Uses `tokio::sync::watch` channel for coordinated shutdown with in-flight request tracking
- **TLS Feature**: Gated behind the `tls` Cargo feature (enabled by default)
