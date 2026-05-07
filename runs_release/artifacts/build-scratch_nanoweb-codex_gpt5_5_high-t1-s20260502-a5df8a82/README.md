# nanoweb

`nanoweb` is a small self-contained HTTP/1.1 framework and server written in Rust. It includes its own request parser, trie router, middleware stack, body parsers, static-file serving, sessions, metrics, health checks, TLS via rustls, and a tiny mustache-style renderer.

## Quickstart

```bash
cargo run -- --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

Example `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_dir = "./public"
log_level = "info"
graceful_timeout_s = 30
session_secret = "replace-with-a-long-random-secret"

[[routes]]
method = "GET"
path = "/"
response = "hello from nanoweb"
status = 200
content_type = "text/plain; charset=utf-8"

[middleware]
logging = true
gzip = true
csrf = false

[middleware.cors]
allowed_origins = ["https://example.com"]

[middleware.rate_limit]
capacity = 100
refill_per_second = 20
```

CLI flags override TOML: `--listen`, `--config`, `--tls-cert`, `--tls-key`, `--metrics`, `--static-dir`, `--log-level`, `--version`, and `--help`.

## Routing

Routes are stored in a trie. Literal segments, parameters, and trailing wildcards are supported:

```rust
router.add(Method::Get, "/users/:id", |req: Request| async move {
    Ok(Response::text(StatusCode::OK, req.path_param("id").unwrap()))
});

router.add(Method::Get, "/static/*path", |req: Request| async move {
    Ok(Response::text(StatusCode::OK, req.path_param("path").unwrap()))
});
```

Supported methods are GET, POST, PUT, DELETE, PATCH, HEAD, and OPTIONS. HEAD falls back to GET and suppresses the body. OPTIONS replies with an `Allow` header for matching paths.

## Middleware

Middleware implements one object-safe trait with `before` and `after` hooks. Registered order is deterministic: before hooks run in registration order and after hooks run as the stack unwinds.

Built-in middleware:

- Structured request logging with request ids.
- Gzip response compression when `Accept-Encoding` includes `gzip`.
- Token-bucket rate limiting per client IP.
- Configurable CORS.
- Double-submit-cookie CSRF protection.
- Basic authentication.

Every response includes `X-Request-Id`.

## Body Parsers

Handlers can call `Request::body_as::<T>()` for `application/json` and `application/x-www-form-urlencoded` where `T: DeserializeOwned`. Multipart form data is available with `Request::multipart()`.

## Static Files

When `static_dir` is set, nanoweb mounts `/static/*path`. Files are served with a weak ETag based on mtime and size, and `If-None-Match` returns `304 Not Modified`. Parent-directory traversal is rejected.

## TLS

TLS is enabled by default through the `tls` Cargo feature:

```bash
cargo run -- --listen 127.0.0.1:8443 --tls-cert cert.pem --tls-key key.pem
```

To build without TLS:

```bash
cargo build --no-default-features
```

When built without the feature, supplying TLS flags returns a clear runtime error.

## Metrics And Health

`/metrics` exposes Prometheus text format counters and histograms:

- `nanoweb_http_requests_total{method,status_class,route}`
- `nanoweb_http_request_duration_seconds_*{method,status_class,route}`

Health endpoints are always mounted:

- `/healthz` returns 200 while the process is up.
- `/readyz` returns 200 after startup and switches away from ready during graceful shutdown.

## Templates

The `Renderer` supports:

- Variables: `{{name}}`
- Sections: `{{#items}}...{{/items}}`
- Inverted sections: `{{^missing}}...{{/missing}}`

Values are read from any `serde::Serialize` context and escaped for HTML text.

## Architecture

The crate is organized into focused modules:

- `http`: byte-level HTTP/1.1 parser and request/response types.
- `router`: trie-based path matching and method dispatch.
- `middleware`: middleware trait and built-ins.
- `body`: JSON, form, and multipart parsing helpers.
- `static_files`: safe static-file serving with ETags.
- `tls`: rustls integration behind a feature flag.
- `session`: pluggable session store and HMAC-SHA256 cookie signing.
- `metrics`, `health`, `template`, `config`, `cli`, `server`, and `error`.

The server uses Tokio. Listener accept loops and per-connection request handling are split so graceful shutdown can stop accepting new sockets while existing request tasks finish within the configured timeout.
