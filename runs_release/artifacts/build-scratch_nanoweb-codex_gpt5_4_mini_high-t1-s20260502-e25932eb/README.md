# nanoweb

`nanoweb` is a small HTTP/1.1 framework and server written in Rust. It ships as both a binary and a library, with its own request parser, trie router, middleware stack, body parsing, static file service, metrics endpoint, sessions, templates, and optional TLS.

## Quickstart

```bash
cargo run --release -- --config ./nanoweb.toml --listen 127.0.0.1:8080
```

The binary accepts these flags:

- `--listen`
- `--config`
- `--tls-cert`
- `--tls-key`
- `--metrics`
- `--static-dir`
- `--log-level`
- `--version`
- `--help`

Each flag has a matching TOML key. CLI values override the TOML file.

## Configuration

`nanoweb` reads a TOML file at startup. A route is declared with `method`, `path`, and a handler `kind`.

Example:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_mount = "/static"
graceful_timeout_s = 30
session_secret = "change-me"

[[routes]]
method = "GET"
path = "/hello"
kind = "text"
body = "hello"

[[routes]]
method = "GET"
path = "/users/:id"
kind = "template"
template = "<h1>User {{id}}</h1>"
context = { title = "Users" }
```

Supported route handler kinds:

- `text`
- `json`
- `template`
- `echo_body`
- `empty`

## Routing

Routes are stored in a trie with:

- literal segments: `/posts/latest`
- path parameters: `/users/:id`
- trailing wildcards: `/static/*path`

Supported methods:

- `GET`
- `POST`
- `PUT`
- `DELETE`
- `PATCH`
- `HEAD`
- `OPTIONS`

`HEAD` falls back to `GET` when no explicit `HEAD` handler exists. `OPTIONS` returns the allowed methods for the matched route.

## Middleware

Middleware runs in registration order and can short-circuit before the handler or mutate the response after the handler.

Built-in middleware:

- request logging
- gzip compression
- token-bucket rate limiting per client IP
- CORS with allowed origins
- CSRF protection with a double-submit cookie
- basic authentication

## TLS

TLS is enabled by default through the `tls` Cargo feature.

Use PEM files with:

```bash
cargo run --release -- --config ./nanoweb.toml --tls-cert ./cert.pem --tls-key ./key.pem
```

For a separate HTTPS listener, set `tls_listen` in TOML. If the feature is disabled, TLS flags return a clear error.

## Metrics

`/metrics` exposes Prometheus text format with:

- request counters by method, status class, and route
- request duration histogram in seconds

The metrics path is configurable with the `metrics` key or `--metrics`.

## Templates

The template engine supports:

- `{{name}}`
- `{{#list}}...{{/list}}`
- `{{^absent}}...{{/absent}}`

Use `template::Renderer::render(template, context)` with any serializable context.

## Architecture

The crate is organized into focused modules:

- `http` for parsing and core HTTP types
- `router` for trie-based route lookup
- `middleware` for cross-cutting request/response behavior
- `body` for typed body parsing
- `static_files` for file serving with ETags
- `tls` for rustls setup
- `session` for signed session cookies and in-memory storage
- `metrics` for Prometheus exposition
- `health` for readiness and liveness endpoints
- `template` for the mustache-like renderer
- `config` and `cli` for startup configuration
- `server` for connection handling and graceful shutdown
- `error` for the crate-wide error type

## Testing

```bash
cargo test
```

The test suite includes unit tests for each module and an integration test that starts the server on an ephemeral port and exercises the full request pipeline.
