# nanoweb

`nanoweb` is a self-contained HTTP/1.1 web framework and server, distributable
as a single binary or used as a library. It ships its own HTTP/1.1 parser, a
trie-based router, a middleware stack, body parsers (JSON / urlencoded form /
multipart), TLS via `rustls`, graceful shutdown, a Prometheus metrics
endpoint, a TOML-configured CLI, and a tiny mustache-style template engine —
no upstream HTTP library wraps any of this.

## Quickstart

Build the binary:

```
cargo build --release
```

Create a `nanoweb.toml`:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
log_level = "info"
graceful_timeout_s = 30
session_secret = "change-me"

[middleware]
logging = true
gzip = true
csrf = false

[middleware.cors]
allowed_origins = ["https://example.com", "*"]

[middleware.rate_limit]
capacity = 60
refill_per_sec = 1.0

[middleware.basic_auth]
realm = "private"
users = [{ username = "alice", password = "secret" }]

[[routes]]
method = "GET"
path = "/"
text = "hello world"

[[routes]]
method = "GET"
path = "/hi/:name"
template = "Hello, {{name}}!"
context = {}
```

Run it:

```
./target/release/nanoweb --config ./nanoweb.toml
```

Hit it:

```
curl -i http://127.0.0.1:8080/
curl -i http://127.0.0.1:8080/metrics
curl -i http://127.0.0.1:8080/healthz
curl -i http://127.0.0.1:8080/readyz
```

## CLI flags

Each flag has a matching key in the TOML file; the CLI value wins when both
are set.

| Flag           | TOML key            | Default            |
|----------------|---------------------|--------------------|
| `--listen`     | `listen`            | `127.0.0.1:8080`   |
| `--config`     | n/a                 | `./nanoweb.toml`   |
| `--tls-cert`   | `tls_cert`          | none               |
| `--tls-key`    | `tls_key`           | none               |
| `--metrics`    | `metrics`           | `/metrics`         |
| `--static-dir` | `static_dir`        | none               |
| `--log-level`  | `log_level`         | `info`             |
| `--version`    | n/a                 | (clap built-in)    |
| `--help`       | n/a                 | (clap built-in)    |

## Routing primer

The router is a trie keyed by URL segments. Patterns support three kinds of
segments:

- Literal: `/users/list`
- Path parameter: `/users/:id` — match any single segment, captured under the
  parameter name on the request.
- Trailing wildcard: `/static/*path` — matches the rest of the URL, captured
  as a slash-joined string. Wildcards must be the final segment.

Method dispatch is explicit per-route: register a handler for one of `GET`,
`POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, or `OPTIONS`. `HEAD` falls back to
the matching `GET` handler with the body suppressed at write time, and
`OPTIONS` auto-lists the allowed methods for the route.

```rust
use nanoweb::{Method, Request, Response};
use nanoweb::router::{Router, Handler};
use std::sync::Arc;

let r = Router::new();
let h: Handler = Arc::new(|req: &mut Request| {
    let id = req.params.get("id").cloned().unwrap_or_default();
    Box::pin(async move { Ok(Response::ok().text(format!("user {}", id))) })
});
r.route(Method::Get, "/users/:id", h);
```

## Middleware

Middleware implement a single object-safe trait:

```rust
pub trait Middleware: Send + Sync {
    fn handle<'a>(&'a self, req: &'a mut Request, next: Next<'a>)
        -> BoxFuture<'a, Result<Response>>;
}
```

`next.run(req)` invokes the rest of the chain (later middleware plus the
handler). A middleware can short-circuit by returning a `Response` without
calling `next`, run logic before and after the handler, or both. The chain
order is the registration order — first-registered runs outermost.

`nanoweb` ships these standard middlewares:

| Module                              | Purpose                                                     |
|-------------------------------------|-------------------------------------------------------------|
| `middleware::logging`               | Structured access log with request id, method, status, ms.  |
| `middleware::compression`           | Gzip responses based on `Accept-Encoding` and content type. |
| `middleware::rate_limit`            | Per-client-IP token bucket; 429 on exhaustion.              |
| `middleware::cors`                  | Configurable allowed origins + preflight handling.          |
| `middleware::csrf`                  | Double-submit cookie CSRF protection.                       |
| `middleware::basic_auth`            | HTTP basic-auth guard with constant-time password compare.  |

Each is enableable via the TOML `[middleware]` table; see the sample
configuration above.

## Body parsers

`Request::body_as::<T>()` decodes the request body based on `Content-Type`:

- `application/json` → `serde_json::from_slice`
- `application/x-www-form-urlencoded` → key/value map
- `multipart/form-data` → field map (file fields are base64-encoded)

```rust
#[derive(serde::Deserialize)]
struct Login { user: String, pass: String }

let body: Login = req.body_as()?;
```

## Static files

`StaticFiles::new(root)` serves a directory:

- Weak ETag derived from `mtime+size`; `If-None-Match` returns `304`.
- Path traversal is rejected (no `..`, no absolute paths).
- A guess-based `Content-Type` is set from the file extension.

When `static_dir` is set in config, `nanoweb` mounts the directory under
`/static/*path` automatically.

## TLS setup

TLS lives behind the `tls` Cargo feature, which is enabled by default. To
enable HTTPS, supply a PEM certificate chain and private key:

```
nanoweb --tls-cert ./certs/server.pem --tls-key ./certs/server.key
```

When the `tls` feature is disabled (`--no-default-features`), the crate still
compiles and the `--tls-cert` flag produces a clear error rather than
silently failing.

## Sessions

A `SessionStore` trait stores per-session data; an in-memory implementation
is bundled. Session IDs are signed with HMAC-SHA256 using the secret from
the configuration:

```rust
use nanoweb::session::{InMemoryStore, SessionManager};
use std::sync::Arc;

let store = Arc::new(InMemoryStore::new());
let mgr = SessionManager::new(b"secret".to_vec(), store);
let id = SessionManager::new_id();
let cookie_value = mgr.sign(&id);
```

## Metrics

The Prometheus text-exposition endpoint is mounted at `--metrics` (default
`/metrics`) and exposes:

- `nanoweb_http_requests_total{method,status,route}` — counter
- `nanoweb_http_request_duration_seconds_*{method,route}` — histogram

Handlers and middleware never reach into a Prometheus registry directly; the
`metrics` module is the single owner.

## Health endpoints

- `GET /healthz` — `200 OK` while the process is live.
- `GET /readyz` — `200 OK` once the server has finished startup and has not
  begun graceful shutdown.

## Template engine

A tiny mustache-like renderer:

```rust
use nanoweb::Renderer;
use serde_json::json;

let r = Renderer::new();
let out = r.render("Hello, {{name}}!", &json!({"name": "world"}))?;
```

Supported tags: `{{var}}` (HTML-escaped), `{{{var}}}` (raw),
`{{#section}} ... {{/section}}` (truthy / iterates arrays / pushes objects),
`{{^section}} ... {{/section}}` (inverted), `{{! comment }}` (skipped).

## Graceful shutdown

`SIGINT` / `SIGTERM` flips the readiness flag, stops accepting new
connections, and waits for in-flight requests to finish — bounded by
`graceful_timeout_s` from configuration (default 30 seconds). The process
exits with status 0.

## Architecture

The crate is organised into one module per concern:

```
src/
  http/           HTTP request types + byte-level parser (parser.rs)
  router/         trie-based router (literals, params, trailing wildcard)
  middleware/     trait + standard middlewares (one sub-module each)
  body/           JSON / urlencoded / multipart parsers
  static_files/   ETag + traversal-safe static server
  tls/            rustls integration (behind `tls` feature)
  session/        SessionStore trait + signed cookies
  metrics/        Prometheus registry + text rendering
  health/         live/ready flags
  template/       mustache-like renderer
  config/         TOML loader; CLI overlay lives only here
  cli/            clap-derived CLI args
  server/         accept loop + per-connection handling + dispatch
  error/          single crate-wide `NanowebError` enum
```

The HTTP parser is intentionally byte-only: it does not depend on the
router, middleware, or body modules, so it can be exercised in isolation
(including by a fuzzer). Routing lookup is O(path-length) and only
allocates a `HashMap` for the captured parameters.

## Building and testing

```
cargo build --release
cargo test
cargo build --no-default-features  # without TLS
```

The crate has both unit tests under each module's `#[cfg(test)]` and
integration tests under `tests/` that start the server on an ephemeral port
and exercise at least one handler end-to-end per standard middleware.
