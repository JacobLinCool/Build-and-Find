# nanoweb

`nanoweb` is a small HTTP/1.1 framework and server written in Rust. It ships with:

- a byte-level HTTP/1.1 parser
- a trie router with path params and wildcards
- middleware chaining
- JSON, urlencoded form, and multipart body parsing
- optional TLS via rustls
- static file serving with ETags
- signed session cookies
- Prometheus-style metrics
- a tiny mustache-like template engine

## Quickstart

Build and run the binary:

```bash
cargo run --release -- --config ./nanoweb.toml --listen 127.0.0.1:8080
```

If `./nanoweb.toml` does not exist, the server starts with defaults.

## Configuration

The CLI flags override TOML values.

Supported CLI flags:

- `--listen`
- `--config`
- `--tls-cert`
- `--tls-key`
- `--metrics`
- `--static-dir`
- `--log-level`
- `--version`
- `--help`

Example configuration:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_mount = "/static"
static_dir = "./public"
log_level = "info"
graceful_timeout_s = 30
session_secret = "change-me"
tls_listen = "127.0.0.1:8443"
tls_cert = "./cert.pem"
tls_key = "./key.pem"

[[middleware]]
kind = "logging"

[[middleware]]
kind = "gzip"

[[middleware]]
kind = "cors"
allowed_origins = ["https://example.com"]

[[routes]]
method = "GET"
path = "/"
kind = "text"
body = "hello from nanoweb"

[[routes]]
method = "POST"
path = "/people"
kind = "echo"
```

For route handlers, `kind` can be:

- `text`
- `json`
- `template`
- `echo`
- `file`

## Routing Primer

Routes are stored in a trie.

- Literal path segments match exactly.
- `:name` captures one segment as a parameter.
- `*name` captures the rest of the path as a wildcard.

Examples:

- `/users/:id`
- `/static/*path`

Behavior notes:

- `HEAD` falls back to the matching `GET` route and suppresses the body on the wire.
- `OPTIONS` returns `Allow` for the matched route.

## Middleware

The built-in middleware modules are:

- logging
- gzip compression
- token-bucket rate limiting
- CORS
- CSRF double-submit cookie checks
- basic auth

Middleware runs in the order it is registered.

## Body Parsing

`Request::body_as::<T>()` decodes the request body based on `Content-Type`.

Supported content types:

- `application/json`
- `application/x-www-form-urlencoded`
- `multipart/form-data`

The target type `T` must implement `Deserialize`.

## TLS

TLS support is enabled by default through the `tls` Cargo feature.

To serve HTTPS, set:

- `tls_cert`
- `tls_key`
- `tls_listen`

If the `tls` feature is disabled, the crate still builds, but TLS configuration returns a clear error.

## Metrics

The Prometheus text endpoint is mounted at the configured `metrics` path, defaulting to `/metrics`.

It exports:

- request counters by method, status class, and route
- request duration histogram in seconds

## Template Engine

The template renderer supports:

- `{{name}}`
- `{{#list}}...{{/list}}`
- `{{^absent}}...{{/absent}}`

It renders against any `serde::Serialize` context value.

## Architecture

The crate is organized into focused modules:

- `http`: parser and HTTP types
- `router`: trie-based route lookup
- `middleware`: middleware traits and standard middleware
- `body`: body decoding
- `static_files`: file serving
- `tls`: TLS setup
- `session`: signed cookie sessions and stores
- `metrics`: Prometheus text exposition
- `health`: built-in health handlers
- `template`: mustache-style rendering
- `config`: TOML loading and CLI overrides
- `cli`: clap-based argument parsing
- `server`: accept loop, request handling, graceful shutdown
- `error`: crate-wide error type

## Development

```bash
cargo fmt
cargo test
cargo build --release
```
