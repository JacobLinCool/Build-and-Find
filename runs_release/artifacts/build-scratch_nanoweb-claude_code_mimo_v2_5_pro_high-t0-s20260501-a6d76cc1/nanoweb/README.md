# nanoweb

A self-contained HTTP/1.1 web framework and server written in Rust. Works as a standalone binary or as a library.

## Quick Start

```bash
# Build
cargo build --release

# Run with default settings (127.0.0.1:8080, reads ./nanoweb.toml)
./target/release/nanoweb

# Run with custom settings
./target/release/nanoweb --listen 0.0.0.0:3000 --config my_app.toml --log-level debug
```

## Configuration

Create a `nanoweb.toml` file:

```toml
listen = "127.0.0.1:8080"
log_level = "info"
graceful_timeout_s = 30
session_secret = "your-secret-key-here"

[[routes]]
path = "/hello"
method = "GET"
handler = "hello"
body = "Hello, World!"
content_type = "text/plain; charset=utf-8"
status = 200

[[routes]]
path = "/page"
method = "GET"
handler = "page"
file = "./templates/page.html"
content_type = "text/html; charset=utf-8"

[middleware]
logging = true
gzip = true

[middleware.cors]
allowed_origins = ["*"]

[middleware.rate_limit]
capacity = 100
refill_rate = 10.0
```

### CLI Flags

Every CLI flag overrides the corresponding TOML key:

| Flag | Default | Description |
|------|---------|-------------|
| `--listen` | `127.0.0.1:8080` | Address and port to bind |
| `--config` | `./nanoweb.toml` | Path to TOML config file |
| `--tls-cert` | - | TLS certificate (PEM) |
| `--tls-key` | - | TLS private key (PEM) |
| `--metrics` | `/metrics` | Prometheus metrics endpoint |
| `--static-dir` | - | Static files directory |
| `--log-level` | `info` | Log level (trace/debug/info/warn/error) |
| `--help` | - | Print help |
| `--version` | - | Print version |

## Routing

Routes are configured in the TOML file. The router supports:

- **Literal segments**: `/users/list`
- **Path parameters**: `/users/:id` (accessible via `req.params`)
- **Wildcard segments**: `/static/*path` (captures remaining path)
- **Method dispatch**: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS

HEAD requests automatically fall back to the matching GET handler with the body suppressed. OPTIONS requests return the list of allowed methods.

## Middleware

Configure middleware in the `[middleware]` section of your config:

### Built-in Middleware

| Middleware | Config Key | Description |
|-----------|------------|-------------|
| **Logging** | `logging = true` | Structured request logging via `tracing` |
| **Gzip** | `gzip = true` | Response compression (respects Accept-Encoding) |
| **Rate Limiting** | `[middleware.rate_limit]` | Token-bucket per client IP |
| **CORS** | `[middleware.cors]` | Configurable allowed origins |
| **CSRF** | `csrf = true` | Double-submit cookie protection |
| **Basic Auth** | `[middleware.basic_auth]` | HTTP Basic Authentication guard |

### Implementing Custom Middleware

```rust
use nanoweb::middleware::Middleware;
use nanoweb::http::{Request, Response};
use async_trait::async_trait;

struct MyMiddleware;

#[async_trait]
impl Middleware for MyMiddleware {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        // Return Some(Response) to short-circuit
        None
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        // Observe or mutate the response
    }
}
```

## Body Parsers

The framework provides typed body parsing via `req.body_as::<T>()`:

- `application/json` - parsed via `serde_json`
- `application/x-www-form-urlencoded` - parsed via `serde_urlencoded`
- `multipart/form-data` - built-in parser with field extraction

## TLS Setup

Enable TLS with rustls (enabled by default via the `tls` feature):

```bash
./target/release/nanoweb \
  --tls-cert /path/to/cert.pem \
  --tls-key /path/to/key.pem \
  --listen 0.0.0.0:443
```

To disable TLS support: `cargo build --no-default-features`

## Metrics

A Prometheus text-exposition endpoint is available at `/metrics` (configurable). It reports:

- `http_requests_total` - counter by method, status class, and route
- `http_request_duration_seconds` - histogram of request durations

## Health Endpoints

- `/healthz` - returns 200 OK when the server is running
- `/readyz` - returns 200 OK when ready to accept traffic (503 during shutdown)

## Template Engine

A minimal mustache-like template engine is included:

```rust
use nanoweb::template::Renderer;
use serde_json::json;

let renderer = Renderer::new();
let html = renderer.render(
    "<h1>{{title}}</h1><ul>{{#items}}<li>{{name}}</li>{{/items}}</ul>",
    &json!({"title": "Hello", "items": [{"name": "World"}]})
)?;
```

Supported syntax:
- `{{variable}}` - HTML-escaped variable interpolation
- `{{path.to.nested}}` - dotted path access
- `{{#section}}...{{/section}}` - truthy section / array iteration
- `{{^inverted}}...{{/inverted}}` - rendered when value is absent or falsy

## Session Management

Sessions use HMAC-SHA256 signed cookies. The in-memory store is included; implement the `SessionStore` trait for custom backends.

## Architecture

```
src/
  http/        - HTTP/1.1 parser and types (Request, Response, Method)
  router/      - Trie-based router with params and wildcards
  middleware/  - Middleware trait + standard middleware
  body_parser/ - JSON, urlencoded, and multipart parsers
  static_files/- Static file serving with ETag and 304 support
  tls/         - TLS configuration via rustls
  session/     - Session management with signed cookies
  metrics/     - Prometheus metrics registry
  health/      - Health and readiness endpoints
  template/    - Mustache-like template engine
  config/      - TOML + CLI configuration merging
  cli/         - CLI argument parsing via clap
  server/      - Accept loop, connection handling, graceful shutdown
  error/       - Crate-wide error types
```

## License

MIT
