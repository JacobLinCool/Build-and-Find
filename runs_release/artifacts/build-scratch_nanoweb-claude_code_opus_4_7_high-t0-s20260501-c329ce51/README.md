# nanoweb

`nanoweb` is a self-contained HTTP/1.1 web framework and server. It ships its
own HTTP/1.1 parser, a trie-based router, a middleware stack, body parsers, a
mustache-style template engine, optional TLS via rustls, a Prometheus metrics
endpoint, and a TOML-driven CLI. It is usable both as a single binary and as a
Rust library.

## Quickstart

Build the release binary:

```bash
cargo build --release
```

Run the bundled server (serves an index page and the standard endpoints):

```bash
./target/release/nanoweb --listen 127.0.0.1:8080
```

Out of the box you get:

- `GET /` — small landing page
- `GET /healthz`, `GET /readyz` — liveness / readiness probes
- `GET /metrics` — Prometheus text exposition

## Configuration

Configuration is loaded once at startup from a TOML file (default
`./nanoweb.toml`). Every CLI flag has a matching TOML key, and CLI flags
override TOML values. See [`nanoweb.toml`](./nanoweb.toml) for an example.

CLI:

```
--listen <ADDR:PORT>     plain HTTP listen address
--config <PATH>          configuration file (default ./nanoweb.toml)
--tls-cert <PATH>        PEM TLS certificate
--tls-key <PATH>         PEM TLS private key
--metrics <PATH>         metrics endpoint path
--static-dir <PATH>      directory served at /static/*
--log-level <LEVEL>      trace|debug|info|warn|error
--version                print version
--help                   print help
```

## Routing primer

The router is backed by a trie that matches each request path in
`O(path-length)` and supports literal segments, single parameters
(`:name`), and a trailing wildcard (`*name`). Method dispatch is explicit;
`HEAD` falls back to the matching `GET` (with body suppressed), and
`OPTIONS` reports the allowed methods.

```rust
use nanoweb::{App, Request, Response};
use nanoweb::config::Config;
use nanoweb::cli::CliArgs;

let cfg = Config::load(&CliArgs { listen: None, config: None, tls_cert: None,
    tls_key: None, metrics: None, static_dir: None, log_level: None }).unwrap();
let mut app = App::new(cfg);

app.router.get("/users/:id", |req: Request| async move {
    let id = req.param("id").unwrap_or("").to_string();
    Response::text(format!("user {id}"))
});

app.router.get("/static/*path", |req: Request| async move {
    let p = req.param("path").unwrap_or("").to_string();
    Response::text(format!("path {p}"))
});
```

## Middleware

A middleware implements the single `Middleware` trait with `before` and
`after` hooks; `before` may short-circuit by returning a `Response`. The chain
runs in the registration order: outermost first for `before`, reversed for
`after`.

The standard middleware library:

| Middleware     | Purpose                                          |
| -------------- | ------------------------------------------------ |
| `RequestLogger`| Structured request / response logging.           |
| `Compression`  | gzip the response body when `Accept-Encoding`.   |
| `RateLimit`    | Token-bucket rate limit keyed on the client IP.  |
| `Cors`         | Configurable allowed origins, preflight support. |
| `Csrf`         | Double-submit cookie CSRF protection.            |
| `BasicAuth`    | HTTP Basic-Auth guard.                           |

```rust
use nanoweb::middleware::{Compression, Cors, RateLimit, RequestLogger};

let mut app = App::new(cfg);
app.chain = app.chain
    .push(RequestLogger::new())
    .push(Compression::new())
    .push(RateLimit::new(100.0, 50.0))
    .push(Cors::new(vec!["*".into()]));
```

## Body parsers

Three content types are decoded via `Request::body_as::<T>()` for any
`T: serde::de::DeserializeOwned`:

- `application/json`
- `application/x-www-form-urlencoded`
- `multipart/form-data` (text fields)

## Static files

When `--static-dir` (or the TOML `static_dir`) is set, files under that
directory are served at `/static/*`. Responses include a weak ETag derived
from `mtime` and size, and respect `If-None-Match` (returns `304 Not
Modified`). Path traversal (`..` segments, absolute paths) is rejected.

## TLS

TLS is gated by the `tls` Cargo feature (enabled by default) and uses
`rustls`. Provide `--tls-cert` and `--tls-key` (PEM) and a TLS listener will
start in addition to the plain HTTP listener. With the feature disabled the
crate still builds and the `--tls-cert` flag returns a clear error.

```bash
nanoweb --tls-cert ./certs/cert.pem --tls-key ./certs/key.pem
```

## Metrics

`GET /metrics` returns Prometheus text-exposition output:

- `nanoweb_http_requests_total{method,status_class,route}` (counter)
- `nanoweb_http_request_duration_seconds` (histogram)

Handlers and middleware only interact with metrics through
`nanoweb::metrics::Metrics`.

## Health endpoints

- `GET /healthz` — `200 OK` while the process is up.
- `GET /readyz` — `200 OK` after startup completes; switches to `503` once
  shutdown is signalled so load balancers can drain traffic.

## Sessions

Session storage is pluggable via the `SessionStore` trait. The default
implementation is in-memory. Cookies are signed with HMAC-SHA256 using the
`session_secret` from the configuration.

## Template engine

A small mustache-style engine ships at `nanoweb::template::Renderer`:

- `{{name}}` — HTML-escaped interpolation
- `{{{name}}}` / `{{&name}}` — raw interpolation
- `{{#section}}...{{/section}}` — truthy / iteration
- `{{^absent}}...{{/absent}}` — inverse section
- `{{! comment }}` — comment

```rust
use nanoweb::template::Renderer;
use serde_json::json;

let r = Renderer::new();
let html = r.render(
    "<h1>{{title}}</h1><ul>{{#items}}<li>{{.}}</li>{{/items}}</ul>",
    &json!({"title":"Hello","items":["a","b","c"]}),
)?;
```

## Architecture overview

```
+-------------------+
|        cli        |  --> parses argv
+---------+---------+
          v
+-------------------+
|      config       |  --> merges TOML + CLI into a single Config
+---------+---------+
          v
+-------------------+        +-------------+
|      server       | <----> |   metrics   |
| accept loop, conn |        |   health    |
| lifecycle, write  |        +-------------+
+----+----+----+----+
     |    |    |
     v    v    v
+-------+ +--------+ +-------------+
|  http | | router | | middleware  |
| parser| |  trie  | |   chain     |
+-------+ +--------+ +-------------+
                          |
                          v
                  +---------------+
                  |   handler     | --> body, template, session, static_files
                  +---------------+
```

The HTTP parser (`nanoweb::http::parser`) does no I/O and depends only on
the type module — it can be exercised in isolation via fuzz tests. The
router owns the trie and parameter map but knows nothing about the wire
format. The server module connects the parser to the router and runs the
middleware chain. Configuration is centralised in the `config` module and a
single crate-wide error enum (`NanowebError`) is used at all public
fallible boundaries.

## Graceful shutdown

On `SIGINT` / `SIGTERM` the server stops accepting new connections, flips
`/readyz` to `503`, lets in-flight requests run to completion (up to
`graceful_timeout_s`, default `30`), and exits cleanly with code `0`.

## Testing

Unit tests live next to each module (`#[cfg(test)]`), and integration tests
under `tests/integration.rs` start the server on an ephemeral port and
exercise at least one handler end-to-end per standard middleware. Run:

```bash
cargo test
```

## License

Dual-licensed under MIT or Apache-2.0 at your option.
