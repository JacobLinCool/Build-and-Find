# nanoweb

A tiny, self-contained HTTP/1.1 web framework in Rust, written as a
teaching-scale baseline for experiments. The crate is intentionally
modular: each concept (parser, router, middleware, bodies, session,
metrics, config) lives in its own module and can be reasoned about in
isolation.

## Build

```bash
cargo build --release
```

The binary ends up at `target/release/nanoweb`.

The `tls` Cargo feature (on by default) pulls in `rustls`. To build
without TLS:

```bash
cargo build --release --no-default-features
```

When TLS is disabled at compile time, passing `--tls-cert` or
`--tls-key` on the command line will cause the binary to exit with a
clear `feature 'tls' is not compiled in this build` error.

## Quickstart

```bash
./target/release/nanoweb --listen 127.0.0.1:8080
```

Useful endpoints exposed out-of-the-box:

- `/`             — a small welcome page
- `/hello/:name`  — parameterised greeting
- `/healthz`      — liveness probe
- `/readyz`       — readiness probe (flips to `503` during shutdown)
- `/metrics`      — Prometheus text-format exposition

## CLI flags

| Flag                         | Purpose                                        |
| ---------------------------- | ---------------------------------------------- |
| `--listen HOST:PORT`         | Bind address (default `127.0.0.1:8080`).       |
| `--config PATH`              | Load a config file.                            |
| `--config-format FORMAT`     | `toml` (default) or `ini` (legacy, see below). |
| `--tls-cert PATH`            | PEM certificate chain (requires `tls` feature).|
| `--tls-key PATH`             | PEM private key (requires `tls` feature).      |
| `--metrics {on,off}`         | Enable/disable `/metrics` (default on).        |
| `--static-dir PATH`          | Serve `/static/*path` from a directory.        |
| `--log-level LEVEL`          | `trace|debug|info|warn|error` (default info). |
| `--version`                  | Print the version string and exit.             |
| `--help`                     | Show the help text and exit.                   |

CLI flags always win over values loaded from the config file. That
precedence is centralised in the `config` module.

## Routing primer

The router is a trie with three node kinds:

- **Literal** — exact path segment matches (e.g. `/users`)
- **Param**   — `/users/:id` captures `id` into `request.path_params`
- **Wildcard**— `/files/*rest` captures the remainder of the path

```rust
use nanoweb::{
    http::types::{Method, Response},
    router::{handler, Router},
};

let mut r = Router::new();
r.add(
    Method::Get,
    "/users/:id",
    handler(|req| async move {
        let id = req.param("id").unwrap_or("?").to_string();
        Ok(Response::text(format!("user {}", id)))
    }),
).unwrap();
```

`HEAD` requests fall back to the `GET` handler when no explicit `HEAD`
is registered. `OPTIONS` automatically returns a `204 No Content` with
an `Allow:` header listing the methods registered for that path.

## Middleware

The framework ships with a small catalogue of built-in middleware.
Middleware registration order is preserved; outer middleware see
requests first and responses last.

| Name             | Module                               | Notes                                                   |
| ---------------- | ------------------------------------ | ------------------------------------------------------- |
| `logger`         | `middleware::logger`                 | Structured per-request log, mirrors `X-Request-Id`.     |
| `rate_limit`     | `middleware::rate_limit`             | Token bucket per client IP.                             |
| `cors`           | `middleware::cors`                   | CORS headers and preflight handling.                    |
| `gzip`           | `middleware::gzip`                   | Compresses large responses when `Accept-Encoding` OKs.  |
| `csrf`           | `middleware::csrf`                   | Double-submit cookie + `X-CSRF-Token` header.           |
| `basic_auth`     | `middleware::basic_auth`             | RFC 7617 `Basic` auth.                                  |
| `digest_auth`    | `middleware::digest_auth`            | **Deprecated** — RFC 7616 Digest auth, see below.       |

### Deprecated: digest auth

`digest_auth` ships with the framework so that deployments with legacy
clients can continue to function, but its use is discouraged:

- Digest is weaker than modern alternatives.
- The shipped implementation maintains in-memory nonce state, which
  does not scale horizontally.

New deployments should prefer `basic_auth` over TLS or session cookies.
This middleware is expected to be removed in a future release.

## TLS setup

1. Produce a certificate and key (for local testing you can use
   `openssl`).
2. Run:

```bash
nanoweb --tls-cert server.pem --tls-key server.key --listen 0.0.0.0:443
```

The TLS glue lives in `src/tls.rs` and uses `rustls`. It is guarded by
the `tls` Cargo feature — disabling the feature keeps the crate
compiling and replaces the TLS configuration with a `FeatureDisabled`
error so the CLI always produces an actionable message.

## Metrics

If `--metrics on` is set (the default) the server exposes Prometheus
text-format metrics at `/metrics`:

- `http_requests_total{method,status,route}` (counter)
- `http_request_duration_seconds` (histogram)

The formatter is hand-written and has no runtime dependencies beyond
the standard library.

## Template engine

`nanoweb::template::render` evaluates Mustache-like templates:

- `{{name}}`              — HTML-escaped interpolation
- `{{{name}}}`            — unescaped interpolation
- `{{#section}}...{{/section}}` — visible when the value is truthy; loops over lists
- `{{^inverted}}...{{/inverted}}` — visible when the value is missing or falsy
- `{{! comment }}`        — inline comments

## Health

- `/healthz` — liveness; returns `200 OK` as soon as the accept loop is up.
- `/readyz`  — readiness; flips to `503` during graceful shutdown.

## Configuration files

TOML (preferred):

```toml
[server]
listen = "0.0.0.0:8080"
log_level = "info"
graceful_timeout_s = 30

[rate_limit]
enabled = true
capacity = 100
refill_per_sec = 50

[session]
secret = "please-change-me"
ttl_seconds = 3600

[digest_auth]             # legacy, see below
enabled = false
realm = "nanoweb"
```

### Deprecated: `--config-format ini` and the INI loader

The framework still accepts INI-formatted configuration for backward
compatibility:

```ini
[server]
listen = "0.0.0.0:8080"
log_level = "info"

[digest_auth]
enabled = true
user.alice = secret
```

It is selected via `--config-format ini`. **This path is deprecated.**
The INI schema is less expressive than TOML and is scheduled to be
removed in a future release — please migrate to `--config-format toml`
(the default) as soon as practical.

## Architectural overview

```
+-----------+        +-----------+        +-------------------+
|  parser   | -----> |   chain   | -----> |  router + handler |
+-----------+        +-----------+        +-------------------+
                        |  ^                     |
                        |  |                     v
                        |  |              +-------------+
                        |  +--------------+  metrics    |
                        v                 +-------------+
                  +------------+
                  |  response  |
                  +------------+
```

- `http::parser` is pure: it consumes bytes and produces a `Request`
  or a parse error. It does not know anything about the router, the
  middleware chain, or typed body helpers.
- `middleware::Chain` is object-safe and preserves registration order.
- `router::Router` backs the dispatch table with a trie whose three
  node kinds are literal, `:param`, and `*wildcard`.
- `config` is the single owner of "CLI overrides TOML" precedence.
- `server` wires the above pieces together and handles graceful
  shutdown on `SIGINT` / `SIGTERM`.

## Tests

Every substantial module ships with `#[cfg(test)]` unit tests:

```bash
cargo test
```
