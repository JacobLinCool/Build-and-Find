# nanoweb

A self-contained HTTP/1.1 web framework written in Rust. This is NOT a wrapper around hyper/actix/etc - it has its own HTTP/1.1 parser built from scratch.

## Features

- **HTTP/1.1 parser** - Pure byte-level parser for request line, headers, Content-Length and chunked transfer encoding bodies
- **Trie-based router** - O(path-length) lookup with path parameters (`:id`), wildcards (`*path`), and method dispatch
- **Middleware system** - Object-safe async middleware trait composed in registration order
- **Body parsers** - JSON, URL-encoded, and multipart form data
- **Static files** - Serve files with ETag/304 support and directory traversal prevention
- **TLS** - rustls TLS support (behind `tls` feature flag, enabled by default)
- **Session management** - SessionStore trait + in-memory impl with HMAC-SHA256 signed cookies
- **Prometheus metrics** - Counter and histogram in text exposition format
- **Health checks** - `/healthz` and `/readyz` endpoints with pluggable checks
- **Template engine** - Mustache-style: `{{name}}`, `{{#list}}{{/list}}`, `{{^absent}}{{/absent}}`
- **Configuration** - TOML config + CLI flags override
- **Graceful shutdown** - SIGINT/SIGTERM handling with configurable timeout
- **Async** - Built on Tokio async runtime

## Quick Start

```rust
use nanoweb::{App, Config, Request, Response, NanowebError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = App::new()
        .get("/", |_req: Request| async {
            Ok::<Response, NanowebError>(Response::ok().with_html("<h1>Hello, World!</h1>"))
        })
        .get("/users/:id", |req: Request| async move {
            let id = req.params.get("id").cloned().unwrap_or_default();
            Ok::<Response, NanowebError>(
                Response::ok().with_json(format!(r#"{{"id":"{}"}}"#, id))
            )
        })
        .with_metrics();

    let server = app.into_server(Config::default());
    server.run().await?;
    Ok(())
}
```

## CLI

```
nanoweb [OPTIONS]

Options:
  -c, --config <FILE>      Path to config file
      --listen <ADDR>      Address to listen on
      --tls-cert <FILE>    TLS certificate file
      --tls-key <FILE>     TLS key file
      --metrics <PATH>     Enable metrics endpoint (path)
      --static-dir <DIR>   Serve static files from directory
      --log-level <LEVEL>  Log level (error, warn, info, debug, trace)
  -V, --version            Print version information
  -h, --help               Print help
```

## Configuration

```toml
listen = "127.0.0.1:8080"
graceful_timeout_s = 30
log_level = "info"
metrics_path = "/metrics"
static_dir = "./static"

[session]
secret = "my-secret-key"

[cors]
allowed_origins = ["*"]

[rate_limit]
requests_per_second = 100
burst = 200

[basic_auth]
username = "admin"
password = "secret"
```

## Middleware

```rust
use nanoweb::middleware::{LoggingMiddleware, CorsMiddleware, GzipMiddleware, RateLimitMiddleware};
use nanoweb::server::App;

let app = App::new()
    .middleware(LoggingMiddleware)
    .middleware(CorsMiddleware::default())
    .middleware(GzipMiddleware::default())
    .middleware(RateLimitMiddleware::new(100.0, 200.0))
    // ... routes
```

## Template Engine

```rust
use nanoweb::template::{Template, Value};
use std::collections::HashMap;

let tmpl = Template::new("Hello, {{name}}! {{#items}}{{item}} {{/items}}");
let mut ctx = HashMap::new();
ctx.insert("name".to_string(), Value::from("World"));
ctx.insert("items".to_string(), Value::List(vec![
    [("item".to_string(), Value::from("foo"))].into_iter().collect(),
    [("item".to_string(), Value::from("bar"))].into_iter().collect(),
]));
let rendered = tmpl.render(&ctx)?;
// "Hello, World! foo bar "
```

## Modules

| Module | Description |
|--------|-------------|
| `http` | HTTP/1.1 parser and types (Request, Response, Method, Status) |
| `router` | Trie-based router with path params and wildcards |
| `middleware` | Middleware trait + logging, gzip, rate_limiting, cors, csrf, basic_auth |
| `body` | JSON, URL-encoded, multipart parsers |
| `static_files` | Static file server with ETag/304 support |
| `tls` | rustls TLS acceptor (feature-gated) |
| `session` | Session store trait + in-memory impl |
| `metrics` | Prometheus text format metrics |
| `health` | Health and readiness check endpoints |
| `template` | Mustache-style template engine |
| `config` | TOML configuration + CLI overrides |
| `cli` | clap-based CLI |
| `server` | Async server with graceful shutdown |
| `error` | NanowebError enum |

## Cargo Features

- `tls` (default) - Enable TLS support via rustls

## Building

```bash
cargo build
cargo test
```

## License

MIT
