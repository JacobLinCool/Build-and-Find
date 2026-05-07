# nanoweb

`nanoweb` is a small HTTP/1.1 web framework and server implemented as one Rust crate. It includes its own byte-level HTTP parser, trie router, middleware stack, body parsers, static file serving, metrics, sessions, TLS support through rustls, and a tiny mustache-style renderer.

## Quickstart

```bash
cargo build --release
cargo run -- --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

Example `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_dir = "./public"
log_level = "info"
session_secret = "replace-this"
graceful_timeout_s = 30

[middleware]
logging = true
gzip = true
cors_origins = ["https://example.test"]

[[routes]]
method = "GET"
path = "/"
text = "hello from nanoweb"

[[routes]]
method = "GET"
path = "/hello/:name"
template = "<h1>Hello {{name}}</h1>"
context = { name = "world" }

[[routes]]
method = "GET"
path = "/static/*path"
static_prefix = "/static"
```

CLI flags override matching TOML keys: `--listen`, `--config`, `--tls-cert`, `--tls-key`, `--metrics`, `--static-dir`, and `--log-level`. `--help` and `--version` are provided by the CLI.

## Routing

Routes are stored in a trie with literal, parameter, and trailing wildcard segments:

```rust
use nanoweb::{Method, Request, Response, Router, StatusCode};

let mut router = Router::new();
router.add(Method::Get, "/users/:id", |req: Request| async move {
    Response::text(StatusCode::Ok, req.params["id"].clone())
});
router.add(Method::Get, "/static/*path", |req: Request| async move {
    Response::text(StatusCode::Ok, req.params["path"].clone())
});
```

GET, POST, PUT, DELETE, PATCH, HEAD, and OPTIONS are dispatched explicitly. HEAD falls back to GET with the response body suppressed. OPTIONS returns an `Allow` header for matching routes.

## Middleware

Middleware implements one object-safe trait:

```rust
#[async_trait::async_trait]
pub trait Middleware: Send + Sync {
    async fn before(&self, req: &mut Request) -> Option<Response>;
    async fn after(&self, req: &Request, res: Response) -> Response;
}
```

Built-in middleware:

- Structured request/response logging with request ids.
- Gzip response compression when `Accept-Encoding` includes `gzip`.
- Token-bucket rate limiting per client IP.
- CORS with configured allowed origins.
- Double-submit-cookie CSRF protection.
- Basic authentication.

Middleware runs in registration order before the handler and reverse order after the response.

## Bodies

`Request::body_as::<T>()` supports `application/json`, `application/x-www-form-urlencoded`, and `multipart/form-data`, where `T: serde::de::DeserializeOwned`.

## Static Files

Static routes serve files from `static_dir`, reject directory traversal, set weak ETags based on file size and mtime, and honor `If-None-Match` with `304 Not Modified`.

## TLS

TLS is enabled by default through the `tls` Cargo feature:

```bash
cargo run -- --listen 127.0.0.1:8443 --tls-cert cert.pem --tls-key key.pem
```

When built with `--no-default-features`, TLS flags return a clear runtime error.

## Metrics And Health

The metrics endpoint defaults to `/metrics` and emits Prometheus text exposition counters by method, status class, and route, plus a request-duration histogram.

Health endpoints:

- `/healthz` returns 200 while the server is up.
- `/readyz` returns 200 after startup and stops reporting ready during graceful shutdown.

## Templates

The renderer supports escaped variables, list sections, and inverted sections:

```rust
let html = nanoweb::template::Renderer.render(
    "{{#items}}{{name}}{{/items}}{{^items}}empty{{/items}}",
    &serde_json::json!({"items": [{"name": "a"}]})
)?;
```

## Architecture

Modules are separated by responsibility: `http` owns parser and types, `router` owns trie dispatch, `middleware` owns chain composition and standard middleware, `body` owns request decoding, `static_files` owns filesystem responses, `tls` owns rustls configuration, `session` owns cookie signing and stores, `metrics` owns Prometheus state, `health` owns readiness, `template` owns rendering, `config` owns TOML plus CLI override merging, `cli` owns flags, `server` owns the accept loop and connection handling, and `error` provides the crate-wide error type.
