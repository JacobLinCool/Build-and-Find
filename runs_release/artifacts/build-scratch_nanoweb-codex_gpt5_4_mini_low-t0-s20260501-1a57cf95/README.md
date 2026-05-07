# nanoweb

`nanoweb` is a self-contained HTTP/1.1 web framework and server written in Rust.

## Quickstart

```bash
cargo run -- --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

If you do not provide a config file, built-in defaults are used. CLI flags override TOML values.

## Routing

Routes are backed by a trie and support:

- Literal segments: `/about`
- Path parameters: `/users/:id`
- Wildcards: `/static/*path`
- Methods: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`

`HEAD` falls back to `GET` with the body suppressed. `OPTIONS` returns the allowed methods for the route.

## Middleware

Built-in middleware includes:

- Structured request logging
- Gzip compression
- Token-bucket rate limiting
- CORS
- CSRF protection
- Basic auth guard

Middleware runs in registration order.

## TLS

TLS termination is available with rustls when the `tls` feature is enabled by default and PEM certificate/key paths are provided:

```bash
cargo run -- --tls-cert cert.pem --tls-key key.pem
```

If the TLS feature is disabled at build time, the TLS CLI flags return a clear error.

## Metrics

The server exposes Prometheus-style metrics at `/metrics` by default. The endpoint can be moved with the `metrics` config key or `--metrics` flag.

## Template Engine

The built-in template renderer supports:

- `{{name}}`
- `{{#list}}...{{/list}}`
- `{{^absent}}...{{/absent}}`

## Architecture

The crate is organized into modules for:

- `http` request parsing and HTTP types
- `router` trie-based dispatch
- `middleware` and standard middleware implementations
- `body` parsers
- `static_files`
- `tls`
- `session`
- `metrics`
- `health`
- `template`
- `config`
- `cli`
- `server`
- `error`

The HTTP parser is isolated from router and middleware code so it can be fuzz-tested independently.
