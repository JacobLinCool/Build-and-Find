# nanoweb

A self-contained HTTP/1.1 web framework and server in Rust. `nanoweb` ships
its own HTTP/1.1 parser, a trie-based router, a middleware stack, body
parsers (JSON / urlencoded / multipart), TLS via `rustls`, graceful
shutdown, Prometheus metrics, a TOML-driven CLI, and a small mustache-style
template engine — usable as a single binary or as a library.

## Quickstart

```sh
cargo build --release
./target/release/nanoweb --listen 127.0.0.1:8080
```

Then:

```sh
curl http://127.0.0.1:8080/
curl http://127.0.0.1:8080/hello/world
curl http://127.0.0.1:8080/healthz
curl http://127.0.0.1:8080/metrics
```

## Library use

```rust
use nanoweb::{server::App, http::{Method, Response}, NanowebError, config::Config};
use nanoweb::middleware::logging::RequestLogger;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), NanowebError> {
    let mut app = App::new(Config::default());
    app.route(Method::Get, "/", Arc::new(|_req| async {
        Ok::<_, NanowebError>(Response::text("hello"))
    }));
    let app = app.with_middleware(Arc::new(RequestLogger));
    app.run().await
}
```

## Routing

The router supports literal segments, named parameters and a trailing
wildcard. Lookup is `O(path-length)` over a trie and only allocates a
parameter map per request.

```rust
app.route(Method::Get,  "/users/:id",   handler);  // path parameter
app.route(Method::Get,  "/static/*path", handler); // trailing wildcard
app.route(Method::Post, "/users",        handler);
```

`HEAD` falls back to the matching `GET` handler with the body suppressed.
`OPTIONS` returns `204` with an `Allow:` header that auto-lists the route's
methods.

## Middleware

Each middleware implements the `Middleware` trait and may run logic
*before* the handler (optionally short-circuiting with a response) and
*after* the handler. Middleware run in the order they are registered, and
their `after` hooks run in reverse order.

The standard library lives under `nanoweb::middleware`:

| Module                   | Type                | Purpose                                  |
|--------------------------|---------------------|------------------------------------------|
| `logging`                | `RequestLogger`     | Structured request/response logging      |
| `compression`            | `GzipCompression`   | Gzip responses based on `Accept-Encoding`|
| `rate_limit`             | `RateLimit`         | Token-bucket per client IP               |
| `cors`                   | `Cors`              | Configurable allowed origins             |
| `csrf`                   | `Csrf`              | Double-submit cookie protection          |
| `basic_auth`             | `BasicAuth`         | RFC 7617 Basic authentication            |

## Body parsing

```rust
#[derive(serde::Deserialize)]
struct Msg { hello: String }

let m: Msg = req.body_as()?;
```

`Request::body_as::<T>` dispatches on `Content-Type` and supports
`application/json`, `application/x-www-form-urlencoded`, and
`multipart/form-data`.

## Static files

A configured `static_dir` is served automatically when no route matches.
Files carry a weak ETag derived from `mtime+size`; `If-None-Match` returns
`304` when appropriate. Path traversal (`..`) is rejected with `403`.

## TLS

TLS is provided by `rustls` and gated by the `tls` Cargo feature (enabled
by default).

```sh
nanoweb --listen 127.0.0.1:8080 \
        --tls-cert ./certs/server.pem --tls-key ./certs/server.key
# TLS port defaults to 127.0.0.1:8443 (override with NANOWEB_TLS_LISTEN)
```

If `tls` is disabled at compile time, supplying a TLS flag produces a
clear error instead of silently degrading.

## Metrics

A Prometheus text-exposition endpoint is mounted at `/metrics`
(configurable). Exposes:

- `nanoweb_http_requests_total{method,route,status}` (counter)
- `nanoweb_http_request_duration_seconds_*` (histogram)

## Health

- `GET /healthz` → `200` while the process is alive.
- `GET /readyz`  → `200` once the server is accepting traffic; flips to
  `500` during graceful shutdown.

## Templates

A small mustache-flavored engine supports `{{name}}`, sections
(`{{#list}}…{{/list}}`) and inverted sections (`{{^empty}}…{{/empty}}`):

```rust
let r = nanoweb::template::Renderer::new();
let html = r.render("Hello {{name}}!", &serde_json::json!({"name":"world"}))?;
```

Variables are HTML-escaped by default.

## Sessions

`SessionStore` is a trait; an in-memory default ships with the framework.
Cookies are signed with HMAC-SHA256 using `session_secret` from the TOML
file.

## Configuration

CLI flags override TOML keys. Every flag has a matching key.

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_dir = "./public"
log_level = "info"
graceful_timeout_s = 30
session_secret = "change-me"
# tls_cert = "./certs/server.pem"
# tls_key = "./certs/server.key"
```

```
nanoweb --listen ADDR:PORT \
        --config PATH \
        --tls-cert PATH --tls-key PATH \
        --metrics /metrics \
        --static-dir DIR \
        --log-level info|debug|...
```

## Graceful shutdown

On `SIGINT`/`SIGTERM` the server stops accepting new connections, lets
in-flight requests complete (bounded by `graceful_timeout_s`, default
30 s), and exits cleanly with code `0`.

## Architectural overview

- `http::parser` — byte-level HTTP/1.1 request parser; depends only on
  `http::*` types so it can be exercised in isolation.
- `router` — trie router; literal, parameter, and wildcard segments.
- `middleware::*` — `Middleware` trait + standard library.
- `body` — typed body decoders.
- `static_files` — file serving with ETag and traversal protection.
- `tls` — rustls-backed TLS acceptor (feature-gated).
- `session` — session store trait and HMAC cookie signing.
- `metrics`, `health` — Prometheus + liveness/readiness probes.
- `template` — mustache-lite engine.
- `config`, `cli` — CLI parsing and TOML overlay producing one `Config`.
- `server` — accept loop, request lifecycle, graceful shutdown.
- `error` — single `NanowebError` enum at the public boundary.

## Testing

```sh
cargo test
cargo build --no-default-features    # TLS-disabled build
```

## License

Dual-licensed under MIT or Apache-2.0.
