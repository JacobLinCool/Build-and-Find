# nanoweb

A self-contained HTTP/1.1 web framework and server written in Rust. nanoweb
ships its own HTTP/1.1 parser, a trie-based router, a middleware stack, body
parsers (JSON / urlencoded / multipart), TLS via rustls, graceful shutdown,
Prometheus metrics, a TOML-configured CLI, and a tiny mustache-style template
engine.

## Quickstart

```bash
cargo build --release
./target/release/nanoweb --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

A minimal `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
log_level = "info"
graceful_timeout_s = 30
session_secret = "change-me"

[[routes]]
method = "GET"
path = "/"
kind = "html"
body = "<h1>Hello from nanoweb</h1>"
```

Visit `http://127.0.0.1:8080/`.

## CLI

| Flag           | Description                                            |
|----------------|--------------------------------------------------------|
| `--listen`     | Address (e.g. `127.0.0.1:8080`)                         |
| `--config`     | Path to TOML configuration                             |
| `--tls-cert`   | TLS certificate (PEM)                                  |
| `--tls-key`    | TLS private key (PEM)                                  |
| `--metrics`    | Path of the Prometheus endpoint                        |
| `--static-dir` | Directory served at `/static/*`                         |
| `--log-level`  | `error`, `warn`, `info`, `debug`, `trace`               |
| `--version`    | Print the version                                      |
| `--help`       | Print help                                             |

CLI flags override matching keys in TOML.

## Routing primer

```rust
use nanoweb::{server::App, http::Response, router::handler};

let mut app = App::new();
app.router.get("/users/:id", handler(|req| async move {
    Response::text(format!("user {}", req.param("id").unwrap()))
}));
app.router.get("/static/*path", handler(|req| async move {
    Response::text(format!("path {}", req.param("path").unwrap()))
}));
```

* Path parameters: `:name`
* Wildcard segment: `*name` (must be the trailing segment)
* `HEAD` automatically falls back to the matching `GET` handler with the
  body suppressed.
* `OPTIONS` automatically returns the allowed methods for a route.

## Middleware

Implement the single `Middleware` trait. `before` may short-circuit by
returning `Some(Response)`. `after` observes/mutates the final response.
Middleware is registered in order and runs outer-to-inner.

Standard library:

* `Logging` — structured per-request logs
* `Gzip` — response compression based on `Accept-Encoding`
* `RateLimit` — token-bucket per client IP
* `Cors` — configurable allowed origins / methods / headers
* `Csrf` — double-submit cookie protection on unsafe methods
* `BasicAuth` — `Authorization: Basic ...` guard

## Body parsers

Handlers may call `req.body_as::<T>()` where `T: DeserializeOwned`. The parser
selects based on `Content-Type`:

* `application/json`
* `application/x-www-form-urlencoded`
* `multipart/form-data`

## TLS

TLS termination is enabled when both `--tls-cert` and `--tls-key` are
provided. The plain HTTP and HTTPS listeners run independently. TLS lives
behind the Cargo feature `tls` (enabled by default); when disabled, supplying
TLS flags produces a clear error.

## Metrics

A Prometheus text-exposition endpoint is mounted at `/metrics` (configurable).
Counters: HTTP requests labelled by method, status class, and route.
Histogram: `nanoweb_http_request_duration_seconds`.

## Health endpoints

* `/healthz` — `200 OK` while the process is up
* `/readyz`  — `200 OK` once startup completes; flips to `503` during
  graceful shutdown.

## Template engine

A tiny mustache-like renderer:

```rust
use nanoweb::template::Renderer;
let r = Renderer::new();
let html = r.render("Hi {{name}}", &serde_json::json!({"name":"bob"}));
```

Supported tags: `{{var}}`, `{{#section}}…{{/section}}`,
`{{^inverse}}…{{/inverse}}`. Variable values are HTML-escaped.

## Sessions

A pluggable `SessionStore` trait with an in-memory default. Cookies are signed
with HMAC-SHA256 using the secret from the TOML config.

## Architecture

* `http::parser` – byte-level HTTP/1.1 parsing (no router/middleware/body
  dependencies; safe to fuzz in isolation).
* `router` – trie of literal/parameter/wildcard segments; lookup is
  O(path-length) and only allocates the parameter map.
* `middleware` – the `Middleware` trait plus standard middleware modules.
* `body` – JSON / urlencoded / multipart parsers exposed via
  `Request::body_as`.
* `server` – accept loop split from per-connection handling so graceful
  shutdown can be reasoned about locally.
* `metrics`, `health`, `static_files`, `session`, `template`, `tls`,
  `config`, `cli`, `error` – self-explanatory.

The entire crate compiles on stable Rust. Run the suite with `cargo test`.
