# nanoweb

A self-contained HTTP/1.1 web framework for Rust. No external HTTP parser dependencies — every byte is parsed by hand.

## Features

- Custom HTTP/1.1 request parser (byte-level, no hyper/http crates)
- Trie-based router with path params (`:id`), wildcards (`*path`), and per-method dispatch
- Middleware stack: logging, gzip compression, rate limiting, CORS, CSRF, basic auth
- Body parsers: JSON, URL-encoded, multipart/form-data
- TLS via rustls (behind `tls` cargo feature, enabled by default)
- Graceful shutdown on SIGINT/SIGTERM
- Prometheus metrics endpoint
- Session cookies with HMAC-SHA256 signing
- Mustache-like template engine
- Health endpoints `/healthz` and `/readyz`
- Static file serving with ETag/304 support and directory traversal prevention
- TOML config file + CLI flag overrides
- Structured per-request logging with `X-Request-Id` headers

## Quickstart

```toml
# Cargo.toml
[dependencies]
nanoweb = { path = "." }
```

```rust
use std::sync::Arc;
use nanoweb::router::Router;
use nanoweb::server::{start_http, Server};
use nanoweb::http::Response;
use nanoweb::health::HealthState;

#[tokio::main]
async fn main() {
    let health = Arc::new(HealthState::new());
    let mut router = Router::new();

    router.get("/hello/:name", |req| Box::pin(async move {
        let name = req.params.get("name").map(|s| s.as_str()).unwrap_or("world");
        let mut resp = Response::new(200);
        resp.body = format!("Hello, {}!", name).into_bytes();
        Ok(resp)
    }));

    let server = Arc::new(Server::new(router, health, 5));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let handle = start_http("127.0.0.1:8080", server, rx).await.unwrap();
    handle.await.unwrap();
}
```

Or run the binary directly:

```
cargo run -- --listen 127.0.0.1:8080 --log-level info
```

## CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `--listen` | `127.0.0.1:8080` | Bind address |
| `--log-level` | `info` | Tracing log level |
| `--config` | (none) | Path to TOML config file |
| `--static-dir` | (none) | Directory for static file serving |
| `--tls-cert` | (none) | PEM certificate file |
| `--tls-key` | (none) | PEM private key file |

CLI flags override values in the TOML config file.

## TOML Config

```toml
listen = "0.0.0.0:8080"
log_level = "info"
static_dir = "./public"
graceful_timeout_s = 10
healthz_path = "/healthz"
readyz_path = "/readyz"
metrics_path = "/metrics"

[middleware]
logging = true
compression = true
csrf = false

[middleware.cors]
allowed_origins = ["https://example.com"]
allowed_methods = ["GET", "POST"]
allowed_headers = ["Content-Type", "Authorization"]
allow_credentials = true

[middleware.rate_limit]
max_requests = 100
refill_rate = 10

[middleware.basic_auth]
realm = "Protected"
username = "admin"
password = "secret"
```

## Routing Primer

Routes are stored in a trie. Lookup is O(path segments) with backtracking for parameter/wildcard fallback.

```rust
// Exact match
router.get("/api/status", handler);

// Path parameter → req.params["id"]
router.get("/users/:id", handler);

// Wildcard → req.params["path"]
router.get("/static/*path", handler);

// Multiple methods on the same path
router.post("/users", create_user_handler);
router.get("/users/:id", get_user_handler);
router.delete("/users/:id", delete_user_handler);

// HEAD falls back to GET with body suppressed
// OPTIONS is handled automatically (returns Allow header)

// Per-route middleware (applied after global middleware)
router.get_with("/admin", admin_handler, vec![Arc::new(AuthMiddleware)]);
```

## Middleware

Register middleware globally on the router before adding routes:

```rust
use nanoweb::middleware::{
    LoggingMiddleware, CompressionMiddleware, RateLimitMiddleware,
    CorsMiddleware, CsrfMiddleware, BasicAuthMiddleware,
};

router.use_middleware(LoggingMiddleware);
router.use_middleware(CompressionMiddleware::new());        // gzip, min 1 KB
router.use_middleware(RateLimitMiddleware::new(100, 10));   // 100 req burst, 10/s refill
router.use_middleware(CorsMiddleware::new(CorsConfig {
    allowed_origins: vec!["https://example.com".into()],
    ..Default::default()
}));
router.use_middleware(CsrfMiddleware::new());               // double-submit cookie
router.use_middleware(BasicAuthMiddleware::new("Realm", |u, p| u == "admin" && p == "pass"));
```

Middleware runs outermost-first (LIFO registration, FIFO execution). Each middleware calls `next.run(req).await` to pass through.

## TLS Setup

TLS is enabled by default via the `tls` cargo feature. Provide PEM-encoded cert and key:

```
cargo run -- --tls-cert cert.pem --tls-key key.pem
```

Or in TOML:

```toml
tls_cert = "/etc/ssl/cert.pem"
tls_key  = "/etc/ssl/key.pem"
```

When TLS is configured, the HTTPS server binds on `HTTP_PORT + 1`. Both HTTP and HTTPS servers run concurrently.

Both PKCS#8 and RSA (PKCS#1) private keys are supported. Certificate chains (multiple PEM blocks) are loaded automatically.

To compile without TLS:

```
cargo build --no-default-features
```

## Prometheus Metrics

The `/metrics` endpoint exposes Prometheus text format (content-type `text/plain; version=0.0.4`).

Available metrics:

| Metric | Type | Labels |
|--------|------|--------|
| `nanoweb_requests_total` | Counter | `method`, `path`, `status` |
| `nanoweb_request_duration_seconds` | Histogram | `method`, `path` |
| `nanoweb_active_connections` | Gauge | — |

```
curl http://localhost:8080/metrics
```

## Template Engine

Minimal mustache-style renderer. Supports variable substitution with HTML escaping, sections, inverted sections, and lists.

```rust
use nanoweb::template::{Renderer, Context, Value};

let r = Renderer::new();
let mut ctx = Context::new();
ctx.set("name", "Alice");
ctx.set("show_greeting", true);
ctx.set("items", Value::List(vec![
    Context::new().with("item", "foo"),
    Context::new().with("item", "bar"),
]));

let html = r.render(r#"
  {{#show_greeting}}Hello, {{name}}!{{/show_greeting}}
  <ul>{{#items}}<li>{{item}}</li>{{/items}}</ul>
  {{^items}}No items.{{/items}}
"#, &ctx).unwrap();
```

- `{{var}}` — substitutes variable, HTML-escaped
- `{{#section}}...{{/section}}` — renders if truthy/non-empty list; iterates over lists
- `{{^section}}...{{/section}}` — renders if absent/false/empty list

## Sessions

```rust
use nanoweb::session::{SessionManager, MemorySessionStore};
use std::sync::Arc;

let store = Arc::new(MemorySessionStore::new());
let mgr = SessionManager::new(store, b"your-secret-key");

// In a handler:
let session_id = mgr.create().await?;
mgr.set(&session_id, "user_id", "42").await?;

let mut resp = Response::new(200);
mgr.set_session_cookie(&mut resp, &session_id);
```

Session cookies are signed with HMAC-SHA256. The format is `session_id.hex_signature`. Tampered cookies are rejected.

## Health Endpoints

```
GET /healthz  → 200 {"status":"ok"} or 503
GET /readyz   → 200 {"status":"ready"} or 503
```

Control liveness/readiness programmatically:

```rust
health.set_live(false);   // → healthz returns 503
health.set_ready(false);  // → readyz returns 503
```

## Static Files

```rust
use nanoweb::static_files::StaticFileHandler;

let handler = Arc::new(StaticFileHandler::new("./public"));
router.get("/static/*path", move |req| {
    let h = handler.clone();
    Box::pin(async move { h.handle(&req).await })
});
```

- Weak ETags (`W/"mtime-size"`) with `If-None-Match` → 304
- Directory index: serves `index.html` if present
- Directory traversal: rejected at both path-component and canonicalized-path levels
- Content-Type guessed from extension (html, css, js, json, png, jpg, gif, svg, wasm, …)

## Architecture

```
src/
├── http/
│   ├── parser.rs      — byte-level HTTP/1.1 request parser
│   ├── types.rs       — Method, Version, Headers, StatusCode
│   ├── request.rs     — Request struct (params, cookies, query, extensions)
│   └── response.rs    — Response builder, cookie helpers, into_bytes()
├── router/
│   ├── trie.rs        — Trie<V> with literal/param/wildcard segments
│   └── mod.rs         — Router: route registration, middleware chain, HEAD/OPTIONS
├── middleware/
│   ├── mod.rs         — Middleware trait, Next<'a>, MiddlewareChain
│   ├── logging.rs     — Request/response logging with tracing
│   ├── compression.rs — Gzip compression (flate2)
│   ├── rate_limit.rs  — Token bucket per IP
│   ├── cors.rs        — CORS preflight + headers
│   ├── csrf.rs        — Double-submit cookie CSRF protection
│   └── basic_auth.rs  — HTTP Basic Authentication
├── body/
│   ├── mod.rs         — FormField, MultipartForm types
│   └── multipart.rs   — Multipart/form-data parser (binary-safe)
├── session/mod.rs     — HMAC-signed session cookies, MemorySessionStore
├── metrics/mod.rs     — Prometheus metrics (OnceLock singleton)
├── health/mod.rs      — HealthState with atomic live/ready flags
├── template/mod.rs    — Mustache-like template renderer
├── static_files/mod.rs — Static file serving with ETag support
├── tls/mod.rs         — rustls TLS acceptor (feature-gated)
├── config.rs          — TOML config + override map
├── cli.rs             — clap CLI definition
├── server.rs          — Connection loop, keep-alive, graceful shutdown
├── error.rs           — NanowebError enum
└── main.rs            — Wiring: middleware → routes → server start
```

The HTTP parser (`src/http/parser.rs`) has zero imports from router/middleware/body — it is entirely self-contained. The trie router and middleware chain are also independent of each other; the router holds middleware as `Arc<dyn Middleware>` trait objects.

## Running Tests

```
cargo test                        # all unit + integration tests
cargo test --test integration     # integration tests only
cargo test -- --nocapture         # show println output
```

115 tests total: 90 unit tests across all modules, 24 integration tests, 1 doc test.
