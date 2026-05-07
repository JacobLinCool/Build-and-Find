# nanoweb

A self-contained HTTP/1.1 web framework and server for Rust. Usable as a single binary or as a library.

## Quickstart

```rust
use nanoweb::{Server, Config, Method, Response, Request};

fn main() {
    let config = Config::default();
    let mut server = Server::new(config);

    server.router_mut().route(Method::Get, "/hello/:name", |req: Request| {
        let name = req.params.get("name").map(|s| s.as_str()).unwrap_or("World");
        Response::text(format!("Hello, {name}!"))
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(server.run()).unwrap();
}
```

Or run the binary:

```bash
cargo run -- --listen 127.0.0.1:3000 --config nanoweb.toml
```

## Routing

The router is trie-based with O(path-length) lookup. Supports literal segments, path parameters, and wildcards.

```rust
router.route(Method::Get, "/users/:id", handler);        // path parameter
router.route(Method::Get, "/static/*path", handler);     // wildcard
router.route(Method::Post, "/api/data", handler);        // explicit method
```

- **Path parameters**: `:name` captures a single segment
- **Wildcards**: `*name` captures the remainder of the path
- **HEAD** falls back to GET with the body suppressed
- **OPTIONS** auto-lists allowed methods for a route

## Middleware

Middleware implements the `Middleware` trait with `before` and `after` hooks:

```rust
trait Middleware: Send + Sync {
    fn before(&self, req: &mut Request) -> Option<Response> { None }
    fn after(&self, _req: &Request, resp: Response) -> Response { resp }
    fn name(&self) -> &str;
}
```

### Built-in middleware

| Middleware | Description |
|---|---|
| `logging` | Structured request/response logging with `tracing` |
| `gzip` | Response compression (Accept-Encoding: gzip) |
| `cors` | Configurable CORS headers |
| `csrf` | Double-submit cookie CSRF protection |
| `rate-limit` | Token bucket rate limiting per client IP |
| `basic-auth` | HTTP Basic authentication guard |

Register middleware in config:

```toml
middleware = ["logging", "cors", "rate-limit"]
```

Or programmatically:

```rust
server.push_middleware(Box::new(Cors::new(cors_config)));
```

## Body Parsers

Access deserialized request bodies via the `body_as::<T>()` API:

```rust
#[derive(Deserialize)]
struct CreateUser { name: String, email: String }

let user: CreateUser = req.body_as()?;
```

Supported content types:
- `application/json`
- `application/x-www-form-urlencoded`
- `multipart/form-data`

## TLS

Enable TLS by providing PEM certificate and key files:

```bash
nanoweb --tls-cert cert.pem --tls-key key.pem
```

Or in config:

```toml
tls_cert = "cert.pem"
tls_key = "key.pem"
```

TLS is enabled by default via the `tls` Cargo feature. Disable it with `--no-default-features`.

## Static Files

Serve static files from a directory with ETag support:

```bash
nanoweb --static-dir ./public
```

- ETags are weak, based on mtime + size
- `If-None-Match` returns 304 when appropriate
- Directory traversal is rejected

## Metrics

Prometheus-compatible metrics endpoint at `/metrics` (configurable):

```
# HELP http_requests_total Total HTTP requests
# TYPE http_requests_total counter
http_requests_total 42

# HELP http_requests_by_method Requests by method
http_requests_by_method{method="GET"} 30
http_requests_by_method{method="POST"} 12

# HELP http_request_duration_seconds Request duration histogram
http_request_duration_seconds_sum 1.234
http_request_duration_seconds_count 42
http_request_duration_seconds_min 0.001
http_request_duration_seconds_max 0.5
http_request_duration_seconds_avg 0.029
```

## Template Engine

Minimal mustache-like templates with `Renderer::render()`:

```rust
use nanoweb::Renderer;
use serde_json::json;

let renderer = Renderer::new();
let ctx = json!({"name": "World", "items": ["a", "b", "c"]});

renderer.render("Hello {{name}}!", &ctx);
// => "Hello World!"

renderer.render("{{#items}}{{.}} {{/items}}", &ctx);
// => "a b c "

renderer.render("{{^missing}}default{{/missing}}", &ctx);
// => "default"
```

Features:
- `{{variable}}` - variable interpolation
- `{{#section}}...{{/section}}` - sections (arrays, booleans)
- `{{^inverted}}...{{/inverted}}` - inverted sections (show if absent/false)
- `{{.}}` - current context reference

## Sessions

HMAC-SHA256 signed session cookies:

```rust
use nanoweb::session::SessionManager;

let mgr = SessionManager::new("your-secret-key");
let session_id = mgr.create_session();
let signed = mgr.sign(&session_id);
// Use in Set-Cookie header
```

## Health Endpoints

- `/healthz` - Always returns 200 OK if the server is up
- `/readyz` - Returns 200 OK if ready, 503 if not (post-startup, pre-shutdown)

## CLI Flags

| Flag | Description | Default |
|---|---|---|
| `--listen <ADDR:PORT>` | Address to listen on | `127.0.0.1:8080` |
| `--config <PATH>` | TOML config file | `./nanoweb.toml` |
| `--tls-cert <PATH>` | TLS certificate PEM | - |
| `--tls-key <PATH>` | TLS private key PEM | - |
| `--metrics <PATH>` | Metrics endpoint path | `/metrics` |
| `--static-dir <PATH>` | Static files directory | - |
| `--log-level <LEVEL>` | Log level | `info` |
| `--version` | Print version | - |
| `--help` | Print help | - |

Every CLI flag has a matching TOML key. CLI flags override TOML values.

## Architecture

```
src/
  lib.rs          - Crate root
  main.rs         - Binary entry point
  error.rs        - NanowebError enum
  config.rs       - Configuration loading (TOML + CLI)
  http/
    mod.rs        - HTTP module
    parser.rs     - Byte-level HTTP/1.1 parser
    types.rs      - Request, Response, Method, StatusCode, Headers
  router/
    mod.rs        - Trie-based router
  middleware/
    mod.rs        - Middleware trait and stack
    logging.rs    - Request logging
    gzip.rs       - Gzip compression
    cors.rs       - CORS headers
    csrf.rs       - CSRF protection
    ratelimit.rs  - Rate limiting
    basicauth.rs  - Basic auth
  body/
    mod.rs        - Body parser dispatch
    json.rs       - JSON parser
    form.rs       - URL-encoded form parser
    multipart.rs  - Multipart form parser
  static_files.rs - Static file serving with ETag
  tls.rs          - TLS support via rustls
  session.rs      - Session management with HMAC signing
  metrics.rs      - Prometheus metrics
  health.rs       - Health/readiness endpoints
  template.rs     - Mustache-like template engine
  server.rs       - Async server with graceful shutdown
```

## Graceful Shutdown

On SIGINT/SIGTERM, the server:
1. Stops accepting new connections
2. Lets in-flight requests complete
3. Waits up to `graceful_timeout_s` (default: 30s)
4. Exits cleanly with code 0
