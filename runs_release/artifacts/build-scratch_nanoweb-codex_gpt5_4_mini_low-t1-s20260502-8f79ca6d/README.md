# nanoweb

`nanoweb` is a small HTTP/1.1 web framework and server written in Rust. It includes:

- a custom request parser
- trie-based routing with path params and wildcards
- middleware hooks
- JSON, form, and multipart body helpers
- static file serving
- metrics and health endpoints
- optional TLS via `rustls`
- a minimal template renderer

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run -- --listen 127.0.0.1:8080 --config ./nanoweb.toml
```

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

## Configuration

Configuration is loaded from TOML once at startup. CLI flags override TOML keys. Example:

```toml
listen = "127.0.0.1:8080"
metrics = "/metrics"
static_dir = "./public"
graceful_timeout_s = 30
session_secret = "change-me"
```

## Routing

Routes are stored in a trie and support:

- literal paths like `/hello`
- path params like `/users/:id`
- trailing wildcards like `/static/*path`

Methods supported: `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, and `OPTIONS`.

## Middleware

Included middleware:

- structured request logging
- gzip compression
- token-bucket rate limiting
- CORS
- CSRF protection
- basic auth

## TLS

Enable HTTPS by passing PEM files:

```bash
cargo run -- --tls-cert cert.pem --tls-key key.pem
```

TLS support is behind the `tls` feature and enabled by default.

## Metrics

Prometheus text exposition is served at `/metrics` by default. Metrics include request counters and request duration histograms.

## Template engine

`template::Renderer` performs a minimal mustache-style substitution. It is intended for simple HTML responses.

## Architecture

The crate is organized into focused modules:

- `http`
- `router`
- `middleware`
- `body`
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
