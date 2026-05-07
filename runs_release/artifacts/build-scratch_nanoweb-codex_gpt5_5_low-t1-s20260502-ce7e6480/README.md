# nanoweb

`nanoweb` is a small HTTP/1.1 web framework and server implemented as a Rust binary and library. It owns its socket accept loop, request parser, trie router, middleware chain, metrics, templates, static file serving, sessions, and configuration loading.

## Quickstart

```bash
cargo run -- --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

Example `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
log_level = "info"
graceful_timeout_s = 30
session_secret = "replace-this-secret"

[[routes]]
method = "GET"
path = "/"
response = "hello from nanoweb"

[middleware]
logging = true
gzip = true
cors_origins = ["*"]
csrf = false
rate_limit_capacity = 100
rate_limit_refill_per_s = 10.0
```

CLI flags override TOML keys: `--listen`, `--config`, `--tls-cert`, `--tls-key`, `--metrics`, `--static-dir`, and `--log-level`. `--help` and `--version` are provided by the CLI parser.

## Routing

The router is backed by a trie. It supports literal segments, path parameters such as `/users/:id`, and trailing wildcards such as `/static/*path`. Supported methods are `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, and `OPTIONS`. `HEAD` falls back to `GET` and suppresses the response body. `OPTIONS` reports the allowed methods.

## Middleware

Middleware implement one object-safe trait with `before` and `after` hooks. Standard middleware includes structured request logging, gzip compression based on `Accept-Encoding`, token-bucket rate limiting per client IP, CORS, double-submit-cookie CSRF checks, and basic auth.

## Body Parsing

Handlers can call `Request::body_as::<T>()` for `serde` types. JSON, urlencoded forms, and simple multipart form fields are supported.

## Static Files

Set `static_dir` or pass `--static-dir` to serve files under `/static/*path`. Weak ETags are generated from modification time and size, `If-None-Match` can return `304`, and `..` traversal segments are rejected.

## TLS

TLS support is behind the default `tls` Cargo feature and uses rustls:

```bash
cargo run -- --tls-cert cert.pem --tls-key key.pem
```

When compiled without default features, TLS flags produce a clear runtime error.

## Metrics and Health

Prometheus text metrics are exposed at `/metrics` by default. Counters include requests by method, status class, and route; a histogram records request duration. `/healthz` reports liveness and `/readyz` reports readiness.

## Templates

`template::Renderer` provides a minimal mustache-style renderer for variables, list sections, and inverted sections:

```rust
Renderer::render("Hello {{name}}", &serde_json::json!({"name": "Ada"}))?;
```

## Architecture

The crate is split into modules for HTTP parsing and types, routing, middleware, body parsing, static files, TLS, sessions, metrics, health checks, templates, configuration, CLI, server runtime, and errors. Public fallible entry points return the crate-wide `NanowebError`.
