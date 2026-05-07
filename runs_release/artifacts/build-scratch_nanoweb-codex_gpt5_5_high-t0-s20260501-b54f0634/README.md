# nanoweb

`nanoweb` is a small self-contained HTTP/1.1 framework and server written in Rust. It includes its own byte-level request parser, a trie router, middleware, body parsers, static files, sessions, metrics, TLS support through `rustls`, and a tiny mustache-style renderer.

## Quickstart

```bash
cargo run -- --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

Example `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_dir = "./public"
session_secret = "replace-this-secret"
graceful_timeout_s = 30

[[routes]]
method = "GET"
path = "/hello/:name"
status = 200
body = "hello from nanoweb"
content_type = "text/plain; charset=utf-8"

[middleware]
logging = true
gzip = true
cors_allowed_origins = ["https://example.test"]
rate_limit_capacity = 100
rate_limit_refill_per_sec = 10.0
```

CLI flags override TOML: `--listen`, `--config`, `--tls-cert`, `--tls-key`, `--metrics`, `--static-dir`, `--log-level`, `--version`, and `--help`.

## Routing

Routes are stored in a trie and dispatch by method. Literal segments, parameters, and trailing wildcards are supported:

```rust
app.add_route(Method::Get, "/users/:id", |req: Request| async move {
    Response::text(200, req.params["id"].clone())
});
app.add_route(Method::Get, "/static/*path", |req: Request| async move {
    Response::text(200, req.params["path"].clone())
});
```

`HEAD` falls back to `GET` and suppresses the response body. `OPTIONS` automatically returns an `Allow` header for matching paths.

## Middleware

Middleware implements one object-safe `Middleware` trait with `before` and `after` hooks. Registered order is preserved for request entry; response hooks run on the way back out.

Included middleware:

- Structured request logging with `tracing`
- Gzip response compression controlled by `Accept-Encoding`
- Per-client token bucket rate limiting
- Configurable CORS
- Double-submit cookie CSRF protection
- Basic authentication

## Body Parsing

Handlers can call `Request::body_as::<T>()` for:

- `application/json`
- `application/x-www-form-urlencoded`
- `multipart/form-data`

`T` must implement `serde::de::DeserializeOwned`.

## TLS

TLS is behind the default `tls` Cargo feature and uses PEM certificate and key files:

```bash
cargo run -- --listen 127.0.0.1:8443 --tls-cert cert.pem --tls-key key.pem
```

Build without TLS support:

```bash
cargo build --no-default-features
```

When TLS is disabled, supplying TLS configuration returns a clear startup error.

## Metrics And Health

`/metrics` exposes Prometheus text metrics by default. The endpoint is configurable with the `metrics` key or `--metrics`.

Counters include requests by method, status class, and route. A request duration histogram is also exported. `/healthz` returns server liveness; `/readyz` returns readiness and flips during graceful shutdown.

## Template Engine

The renderer supports escaped variables, sections, and inverted sections:

```rust
let html = Renderer::new().render(
    "<h1>{{name}}</h1>{{#items}}{{name}}{{/items}}{{^items}}empty{{/items}}",
    &serde_json::json!({"name": "nanoweb", "items": [{"name": "a"}]}),
);
```

Supported tags are `{{name}}`, `{{#list}}...{{/list}}`, and `{{^absent}}...{{/absent}}`.

## Architecture

The crate is split by responsibility:

- `http`: request/response types and isolated HTTP/1.1 parser
- `router`: trie router and application dispatch
- `middleware`: trait and standard middleware modules
- `body`: JSON, form, and multipart parsing
- `static_files`: ETag-aware static file serving with traversal rejection
- `tls`: feature-gated rustls setup
- `session`: signed session cookies and pluggable stores
- `metrics`: the sole owner of Prometheus exposition data
- `health`, `template`, `config`, `cli`, `server`, and `error`

The server uses Tokio. Accept loops and per-connection handling are separate functions so graceful shutdown can stop accepting new connections while bounded in-flight work completes.
