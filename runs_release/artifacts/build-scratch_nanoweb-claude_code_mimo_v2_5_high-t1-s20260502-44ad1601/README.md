# nanoweb

A self-contained HTTP/1.1 web framework and server written in Rust. No external HTTP library dependency — nanoweb includes its own HTTP/1.1 parser, trie-based router, middleware stack, and all the features you need to build production web services.

## Quickstart

```bash
# Build
cargo build --release

# Run with defaults (serves on 127.0.0.1:8080, reads ./nanoweb.toml)
./target/release/nanoweb

# Run with a config file
./target/release/nanoweb --config myapp.toml

# Run with TLS
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

## Configuration

nanoweb uses TOML configuration. CLI flags override TOML values.

```toml
listen = "127.0.0.1:8080"
log_level = "info"
graceful_timeout_s = 30
metrics_path = "/metrics"
static_dir = "./public"
session_secret = "change-this-to-a-random-secret"

[cors]
allowed_origins = ["*"]

[rate_limit]
requests_per_second = 100
burst = 200

[[routes]]
method = "GET"
path = "/hello"
handler = "hello"

[[routes]]
method = "POST"
path = "/api/data"
handler = "echo"

[[routes]]
method = "GET"
path = "/page"
handler = "page"
template = "<html><body><h1>{{title}}</h1></body></html>"
```

### CLI Flags

| Flag | TOML Key | Default | Description |
|------|----------|---------|-------------|
| `--listen` | `listen` | `127.0.0.1:8080` | Address:port to bind |
| `--config` | — | `./nanoweb.toml` | Path to config file |
| `--tls-cert` | `tls_cert` | — | TLS certificate PEM path |
| `--tls-key` | `tls_key` | — | TLS private key PEM path |
| `--metrics` | `metrics_path` | `/metrics` | Prometheus metrics endpoint |
| `--static-dir` | `static_dir` | — | Static file directory |
| `--log-level` | `log_level` | `info` | Log level |
| `--version` | — | — | Print version |
| `--help` | — | — | Print help |

## Routing

nanoweb uses a trie-based router supporting:

- **Literal segments**: `/users/list`
- **Path parameters**: `/users/:id` — captures `id` into `request.params`
- **Wildcards**: `/static/*path` — captures everything after `/static/` into `path`
- **Method dispatch**: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS
- **HEAD fallback**: If no HEAD handler is registered, HEAD requests fall back to GET with the body suppressed
- **OPTIONS auto-response**: OPTIONS requests return `405 Method Not Allowed` with an `Allow` header listing available methods

Route lookup is O(path-length) and allocates only the parameter map per request.

## Middleware

Middleware implements the `Middleware` trait:

```rust
pub trait Middleware: Send + Sync {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response;
}
```

### Built-in Middleware

| Middleware | Description |
|------------|-------------|
| **LoggingMiddleware** | Structured request logging with timing |
| **CompressionMiddleware** | Gzip response compression (respects `Accept-Encoding`) |
| **RateLimitMiddleware** | Token-bucket rate limiting per client IP |
| **CorsMiddleware** | Configurable CORS headers |
| **CsrfMiddleware** | Double-submit cookie CSRF protection |
| **BasicAuthMiddleware** | HTTP Basic Authentication guard |

Middleware executes in registration order (outer to inner). The global middleware chain runs before route handlers.

## Body Parsers

```rust
// JSON
let user: User = request.body_as::<User>()?;

// URL-encoded form
let form: LoginForm = request.body_as::<LoginForm>()?;

// Multipart
let fields = MultipartParser::parse_fields(content_type, body)?;
let form: FormData = MultipartParser::parse_as_json(content_type, body)?;
```

Supported content types: `application/json`, `application/x-www-form-urlencoded`, `multipart/form-data`.

## Static Files

Configure with `static_dir` in TOML or `--static-dir` on the CLI. Features:

- ETag generation (weak, based on mtime + size)
- `If-None-Match` / 304 Not Modified support
- Directory traversal prevention
- MIME type detection from file extensions

## TLS

nanoweb supports TLS via rustls:

```bash
./target/release/nanoweb --tls-cert cert.pem --tls-key key.pem
```

Both PEM certificate and private key files are required. TLS is enabled by default via the `tls` Cargo feature. To build without TLS:

```bash
cargo build --no-default-features
```

## Session Management

```rust
let session_mgr = SessionManager::new(Arc::new(InMemorySessionStore::new()), "secret-key");

// Sign data with HMAC-SHA256
let signature = session_mgr.sign("user_id=42");
let valid = session_mgr.verify("user_id=42", &signature);

// Session store
session_mgr.set_session("session-id", data);
let data = session_mgr.get_session("session-id");
```

## Prometheus Metrics

The `/metrics` endpoint (configurable) exposes Prometheus text format:

- `nanoweb_http_requests_total` — Total HTTP requests counter
- `nanoweb_http_request_duration_seconds` — Request duration histogram
- `nanoweb_http_responses_by_status_total` — Responses by status class

## Template Engine

A minimal mustache-like template engine:

```
Hello, {{name}}!
{{#items}}<li>{{.}}</li>{{/items}}
{{^missing}}Default text{{/missing}}
```

Supports: variable interpolation, sections (arrays/objects), inverted sections (missing/false), dot-notation for nested values.

```rust
let renderer = Renderer::new();
let context = serde_json::json!({"name": "World"});
let html = renderer.render("Hello, {{name}}!", &context)?;
```

## Health Endpoints

- `GET /healthz` — Returns 200 OK if the server is running
- `GET /readyz` — Returns 200 OK if ready (post-startup, pre-shutdown), 503 otherwise

## Graceful Shutdown

On SIGINT or SIGTERM, nanoweb:
1. Stops accepting new connections
2. Waits for in-flight requests to complete (bounded by `graceful_timeout_s`, default 30s)
3. Exits cleanly with code 0

## Architecture

```
src/
├── main.rs              # Binary entry point
├── lib.rs               # Library root
├── error.rs             # Crate-wide NanowebError enum
├── config.rs            # TOML + CLI configuration
├── cli.rs               # CLI argument parsing
├── http/
│   ├── mod.rs
│   ├── parser.rs        # Zero-copy HTTP/1.1 parser (independent of other modules)
│   ├── request.rs       # Request type with body parsing
│   └── response.rs      # Response builder
├── router/
│   └── mod.rs           # Trie-based router with params/wildcards
├── middleware/
│   ├── mod.rs           # Middleware trait and chain executor
│   ├── logging.rs       # Request logging
│   ├── compression.rs   # Gzip compression
│   ├── rate_limit.rs    # Token-bucket rate limiter
│   ├── cors.rs          # CORS headers
│   ├── csrf.rs          # CSRF double-submit cookie
│   └── basic_auth.rs    # HTTP Basic Auth
├── body/
│   ├── mod.rs
│   ├── json.rs          # JSON body parser
│   ├── urlencoded.rs    # Form body parser
│   └── multipart.rs     # Multipart body parser
├── session.rs           # Session store + HMAC-SHA256 signing
├── static_files.rs      # Static file serving with ETag
├── tls.rs               # TLS via rustls (feature-gated)
├── metrics.rs           # Prometheus metrics
├── health.rs            # Health/readiness checks
├── template.rs          # Mustache-like template engine
└── server.rs            # Async TCP server with graceful shutdown
```

## License

MIT
