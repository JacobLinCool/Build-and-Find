# nanoweb

A self-contained HTTP/1.1 web framework and server written in Rust. Can be used as a standalone binary or as a library.

## Features

- **Custom HTTP/1.1 parser** — handles request line, headers, chunked transfer encoding, and Content-Length bodies
- **Trie-based router** — supports path parameters (`:id`), wildcards (`*path`), and method dispatch for GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS
- **Middleware stack** — deterministic ordering with before/after hooks
- **Built-in middleware** — logging, gzip compression, rate limiting, CORS, CSRF protection, basic auth
- **Body parsers** — JSON, URL-encoded forms, multipart/form-data
- **Static file serving** — with ETag (weak) and If-None-Match/304 support
- **TLS** — via rustls (optional `tls` feature)
- **Session management** — HMAC-SHA256 signed cookies, pluggable store
- **Prometheus metrics** — request counters by method/status/route, duration histogram
- **Health endpoints** — `/healthz` and `/readyz`
- **Template engine** — mustache-like (`{{var}}`, `{{#list}}`, `{{^absent}}`)
- **Structured logging** — via `tracing` with per-request IDs
- **Graceful shutdown** — SIGINT/SIGTERM handling with configurable timeout
- **TOML + CLI configuration** — CLI flags override TOML values

## Quickstart

```bash
# Build
cargo build --release

# Run with default config
./target/release/nanoweb

# Run with custom settings
./target/release/nanoweb --listen 0.0.0.0:3000 --log-level debug

# Run with TLS
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

## Configuration

Create a `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
log_level = "info"
graceful_timeout_s = 30
session_secret = "your-secret-key"
rate_limit = 100.0
allowed_origins = ["https://example.com"]
metrics = "/metrics"

[tls]
cert = "cert.pem"
key = "key.pem"
```

CLI flags override TOML values:

```
--listen <ADDR:PORT>     Address to listen on (default: 127.0.0.1:8080)
--config <PATH>          TOML config file (default: ./nanoweb.toml)
--tls-cert <PATH>        TLS certificate (PEM)
--tls-key <PATH>         TLS private key (PEM)
--metrics <PATH>         Prometheus metrics endpoint (default: /metrics)
--static-dir <PATH>      Static file directory
--log-level <LEVEL>      Log level: trace, debug, info, warn, error
```

## Routing

Routes are registered with method and path:

```rust
use nanoweb::{Router, Method, Response, RouteEntry};

let mut router = Router::new();

// Literal path
router.add_route(Method::Get, "/hello", RouteEntry {
    handler: Box::new(|_req| Box::pin(async { Response::text(200, "Hello!") })),
    middleware: vec![],
});

// Path parameter
router.add_route(Method::Get, "/users/:id", RouteEntry {
    handler: Box::new(|req| Box::pin(async move {
        let id = req.param("id").unwrap_or("unknown");
        Response::text(200, &format!("User: {id}"))
    })),
    middleware: vec![],
});

// Wildcard
router.add_route(Method::Get, "/static/*path", RouteEntry {
    handler: Box::new(|req| Box::pin(async move {
        let path = req.param("path").unwrap_or("");
        Response::text(200, &format!("File: {path}"))
    })),
    middleware: vec![],
});
```

- `HEAD` requests automatically fall back to the matching `GET` handler (body suppressed)
- `OPTIONS` requests auto-list allowed methods for the route

## Middleware

Implement the `Middleware` trait:

```rust
use nanoweb::{Middleware, Request, Response};

struct MyMiddleware;

impl Middleware for MyMiddleware {
    fn name(&self) -> &str { "my_middleware" }

    fn before(&self, req: &mut Request) -> Option<Response> {
        // Return Some(Response) to short-circuit
        None
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        // Observe or mutate the response
    }
}
```

Built-in middleware:
- **LoggingMiddleware** — structured request logging with `tracing`
- **CompressionMiddleware** — gzip compression (respects Accept-Encoding)
- **RateLimitMiddleware** — token bucket per client IP
- **CorsMiddleware** — configurable allowed origins
- **CsrfMiddleware** — double-submit cookie protection
- **BasicAuthMiddleware** — HTTP Basic authentication

## Body Parsing

```rust
use nanoweb::body;
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

// In a handler:
let user: CreateUser = body::parse_body(&req)?;
```

Supports `application/json`, `application/x-www-form-urlencoded`, and `multipart/form-data`.

## Template Engine

```rust
use nanoweb::{Renderer, TemplateValue};
use std::collections::HashMap;

let mut ctx = HashMap::new();
ctx.insert("name".to_string(), TemplateValue::String("World".to_string()));

let items = vec![
    TemplateValue::Map({
        let mut m = HashMap::new();
        m.insert("item".to_string(), TemplateValue::String("A".to_string()));
        m
    }),
];
ctx.insert("items".to_string(), TemplateValue::List(items));

let html = Renderer::render(
    "<h1>Hello, {{name}}!</h1><ul>{{#items}}<li>{{item}}</li>{{/items}}</ul>",
    &ctx,
)?;
```

Supports:
- `{{variable}}` — HTML-escaped variable interpolation
- `{{#section}}...{{/section}}` — truthy/iterable sections
- `{{^inverted}}...{{/inverted}}` — falsy/empty sections

## Metrics

Prometheus text exposition format at `/metrics` (configurable):

```
# HELP nanoweb_http_requests_total Total HTTP requests
# TYPE nanoweb_http_requests_total counter
nanoweb_http_requests_total{method="GET",status_class="2xx",route="/hello"} 42

# HELP nanoweb_request_duration_seconds Request duration in seconds
# TYPE nanoweb_request_duration_seconds histogram
nanoweb_request_duration_seconds_bucket{le="0.01"} 38
nanoweb_request_duration_seconds_bucket{le="+Inf"} 42
nanoweb_request_duration_seconds_sum 1.23
nanoweb_request_duration_seconds_count 42
```

## Architecture

```
src/
├── main.rs              # CLI entry point
├── lib.rs               # Public API re-exports
├── error.rs             # NanowebError enum
├── config.rs            # TOML + CLI config merging
├── cli.rs               # clap argument definitions
├── server.rs            # Accept loop, connection handling, graceful shutdown
├── http/
│   ├── types.rs         # Method, Request, Response
│   └── parser.rs        # HTTP/1.1 byte-level parser
├── router/
│   └── mod.rs           # Trie-based router
├── middleware/
│   ├── mod.rs           # Middleware trait + chain runner
│   ├── logging.rs       # Structured request logging
│   ├── compression.rs   # Gzip compression
│   ├── rate_limit.rs    # Token bucket rate limiter
│   ├── cors.rs          # CORS headers
│   ├── csrf.rs          # CSRF double-submit cookie
│   └── basic_auth.rs    # HTTP Basic auth guard
├── body/
│   ├── mod.rs           # Unified body parser
│   ├── json.rs          # JSON parser
│   ├── urlencoded.rs    # Form URL-encoded parser
│   └── multipart.rs     # Multipart form parser
├── static_files.rs      # Static file serving with ETag
├── tls.rs               # TLS via rustls
├── session.rs           # Session store + HMAC-signed cookies
├── metrics.rs           # Prometheus metrics collector
├── health.rs            # Health/readiness endpoints
└── template.rs          # Mustache-like template engine
```

## License

MIT
