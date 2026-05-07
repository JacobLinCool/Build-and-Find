# nanoweb

A self-contained HTTP/1.1 web framework and server written in Rust. Usable as a single binary or as a library.

## Quickstart

```bash
# Build
cargo build --release

# Run with defaults (listens on 127.0.0.1:8080)
./target/release/nanoweb

# Run with custom config
./target/release/nanoweb --config ./nanoweb.toml --listen 0.0.0.0:3000

# Run with TLS
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

## Routing

Routes are defined in a TOML config file or registered programmatically:

```toml
[[routes]]
method = "GET"
path = "/users/:id"
handler = "get_user"

[[routes]]
method = "POST"
path = "/users"
handler = "create_user"

[[routes]]
method = "GET"
path = "/static/*path"
handler = "serve_static"
```

**Path parameters**: `/users/:id` captures `id` as a parameter.
**Wildcards**: `/static/*path` captures the rest of the path.
**Methods**: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS.

HEAD requests fall back to the matching GET handler with the body suppressed.
OPTIONS requests automatically list the allowed methods for the route.

### Programmatic routing

```rust
use nanoweb::{Router, Method, Response, StatusCode};

let mut router = Router::new();
router.route(Method::Get, "/hello", |_req| {
    Box::pin(async { Response::text(StatusCode::OK, "Hello!") })
});
```

## Middleware

Middleware implements the `MiddlewareDyn` trait with `before` and `after` hooks:

```rust
use nanoweb::middleware::MiddlewareDyn;

struct MyMiddleware;

impl MiddlewareDyn for MyMiddleware {
    fn before(&self, req: Request) -> ... { /* pre-handler logic */ }
    fn after(&self, req: &Request, resp: Response) -> ... { /* post-handler logic */ }
}
```

### Built-in middleware

| Middleware | Description |
|---|---|
| `logging` | Structured request/response logging via tracing |
| `compression` | Gzip response compression (controlled by Accept-Encoding) |
| `rate_limit` | Token bucket rate limiting per client IP |
| `cors` | CORS with configurable allowed origins |
| `csrf` | CSRF protection via double-submit cookie |
| `basic_auth` | HTTP Basic Authentication guard |

### TOML configuration

```toml
[[middleware]]
name = "logging"

[[middleware]]
name = "compression"

[[middleware]]
name = "rate_limit"
[middleware.options]
max_requests = 100
window_seconds = 60

[[middleware]]
name = "cors"
[middleware.options]
allowed_origins = ["https://example.com"]

[[middleware]]
name = "basic_auth"
[middleware.options]
username = "admin"
password = "secret"
```

## Body Parsers

Supports `application/json`, `application/x-www-form-urlencoded`, and `multipart/form-data`:

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

router.route(Method::Post, "/users", |req| {
    Box::pin(async move {
        let body: CreateUser = req.body_as().unwrap();
        Response::text(StatusCode::CREATED, format!("Created: {}", body.name))
    })
});
```

## Static Files

Serve static files from a configured directory with ETag and If-None-Match support:

```bash
./target/release/nanoweb --static-dir ./public
```

- Weak ETags based on mtime + size
- 304 Not Modified when If-None-Match matches
- Directory traversal is rejected
- Automatic content-type detection

## TLS

TLS termination via rustls (requires the `tls` feature, enabled by default):

```bash
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

Both plain HTTP and HTTPS can run simultaneously. When `tls` feature is disabled, `--tls-cert` produces a clear error.

## Metrics

A Prometheus text-exposition endpoint at `/metrics` (configurable):

```
# HELP nanoweb_http_requests_total Total HTTP requests
# TYPE nanoweb_http_requests_total counter
nanoweb_http_requests_total{method="GET",status_class="2xx",route="GET:/hello"} 5

# HELP nanoweb_request_duration_seconds Request duration in seconds
# TYPE nanoweb_request_duration_seconds histogram
nanoweb_request_duration_seconds_bucket{le="0.001"} 2
nanoweb_request_duration_seconds_bucket{le="0.005"} 4
nanoweb_request_duration_seconds_sum 0.023
nanoweb_request_duration_seconds_count 5
```

## Health Endpoints

- `/healthz` - Returns 200 OK if the server is up
- `/readyz` - Returns 200 OK if the server is ready to accept traffic (post-startup, pre-shutdown)

## Template Engine

A minimal mustache-like template engine:

```rust
use nanoweb::Renderer;
use serde_json::json;

let html = Renderer::render(
    "<h1>{{title}}</h1><ul>{{#items}}<li>{{name}}</li>{{/items}}</ul>",
    &json!({"title": "Users", "items": [{"name": "Alice"}, {"name": "Bob"}]})
).unwrap();
```

**Syntax**:
- `{{name}}` - HTML-escaped variable
- `{{{name}}}` - Unescaped variable
- `{{#list}}...{{/list}}` - Section (rendered for arrays or truthy values)
- `{{^absent}}...{{/absent}}` - Inverted section (rendered for missing/falsy/empty)
- `{{! comment }}` - Comment

## Sessions

Session cookies with HMAC-SHA256 signing:

```toml
session_secret = "your-secret-key"
```

```rust
use nanoweb::session::{InMemorySessionStore, sign_session, verify_session};
```

## CLI Flags

| Flag | Default | Description |
|---|---|---|
| `--listen` | `127.0.0.1:8080` | Address and port to listen on |
| `--config` | `./nanoweb.toml` | Path to TOML configuration file |
| `--tls-cert` | - | TLS certificate file (PEM) |
| `--tls-key` | - | TLS private key file (PEM) |
| `--metrics` | `/metrics` | Metrics endpoint path |
| `--static-dir` | - | Static files directory |
| `--log-level` | `info` | Log level (trace, debug, info, warn, error) |
| `--version` | - | Print version and exit |
| `--help` | - | Print help and exit |

Every CLI flag has a matching TOML key. CLI flags override TOML values.

## Architecture

```
src/
  http/          - HTTP/1.1 parser and types (Request, Response, Method, StatusCode)
  router/        - Trie-based router with path parameters and wildcards
  middleware/    - Middleware trait and standard middleware implementations
    logging.rs   - Structured request logging
    compression.rs - Gzip compression
    rate_limit.rs - Token bucket rate limiter
    cors.rs      - CORS headers
    csrf.rs      - CSRF double-submit cookie
    basic_auth.rs - HTTP Basic Auth
  body/          - Body parsers (JSON, urlencoded, multipart)
  static_files/  - Static file serving with ETag support
  tls/           - TLS via rustls (optional feature)
  session/       - Session store with HMAC-SHA256 cookies
  metrics/       - Prometheus metrics
  health/        - Health/readiness endpoints
  template/      - Mustache-like template engine
  config/        - TOML configuration with CLI override
  cli/           - Command-line argument parsing
  server/        - TCP accept loop, connection handling, graceful shutdown
  error/         - Crate-wide error type
```

## License

MIT
