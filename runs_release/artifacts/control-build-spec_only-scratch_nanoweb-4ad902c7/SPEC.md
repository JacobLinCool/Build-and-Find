spec_version: "0.1"

task_id: scratch_nanoweb
task_family: build_from_scratch
title: nanoweb - an HTTP/1.1 web application framework and server
objective: >
  Build `nanoweb`, a self-contained HTTP/1.1 web framework and server
  usable as a single binary or as a library. The framework must provide
  its own HTTP/1.1 parser, a trie-based router with path parameters
  and wildcards, a middleware stack, body parsers (JSON / urlencoded
  form / multipart), TLS via rustls, graceful shutdown, a Prometheus
  metrics endpoint, a TOML-configured CLI, and a tiny mustache-style
  template engine. The deliverable is a real framework, not a toy
  wrapper around an existing HTTP library.

functional_requirements:
  - id: FR1
    statement: >
      The `nanoweb` binary must start an HTTP/1.1 server on the address
      given by `--listen <ADDR:PORT>` (default `127.0.0.1:8080`),
      serving the routes configured in a TOML file passed via
      `--config <PATH>` (default `./nanoweb.toml`).
  - id: FR2
    statement: >
      The framework must include its own HTTP/1.1 request parser that
      reads the request line (method, target, version), headers, and
      body. It must support chunked transfer encoding and
      Content-Length bodies, and reject requests that violate the
      protocol with a 400 response.
  - id: FR3
    statement: >
      The router must support path parameters (e.g. `/users/:id`),
      wildcard segments (e.g. `/static/*path`), and explicit method
      dispatch for GET, POST, PUT, DELETE, PATCH, HEAD, and OPTIONS.
      HEAD falls back to the matching GET handler with the body
      suppressed; OPTIONS auto-lists the allowed methods for the
      route.
  - id: FR4
    statement: >
      The framework must provide a middleware stack where each
      middleware implements a single `Middleware` trait and can run
      logic before the handler (short-circuiting with a response) and
      after the handler (observing or mutating the response).
      Middleware order is deterministic: configured middleware run in
      the order they are registered.
  - id: FR5
    statement: >
      The standard library of middleware must include: request
      logging (structured), response gzip compression (controlled by
      Accept-Encoding), rate limiting (token bucket per client IP),
      CORS (configurable allowed origins), CSRF protection (double-
      submit cookie), and a basic-auth guard.
  - id: FR6
    statement: >
      Body parsers must include `application/json`, `application/x-
      www-form-urlencoded`, and `multipart/form-data`. Parsers expose
      typed access via a `Request::body_as::<T>()` style API where
      `T: DeserializeOwned`.
  - id: FR7
    statement: >
      The framework must serve static files from a configured
      directory with ETag (weak, based on mtime+size) and If-None-Match
      support, returning 304 when appropriate. Directory traversal
      must be rejected.
  - id: FR8
    statement: >
      TLS termination must be available via rustls when
      `--tls-cert <PATH>` and `--tls-key <PATH>` are supplied (PEM).
      Plain HTTP and HTTPS listeners must be independent; starting
      both simultaneously is allowed.
  - id: FR9
    statement: >
      Session cookies: a pluggable `SessionStore` trait stores
      per-session data; the framework ships an in-memory default and
      signs cookies with HMAC-SHA256 using a secret configured in
      the TOML file.
  - id: FR10
    statement: >
      Graceful shutdown: on SIGINT/SIGTERM the server must stop
      accepting new connections, let in-flight requests complete
      (bounded by `graceful_timeout_s` from config, default 30s), and
      exit cleanly with code 0.
  - id: FR11
    statement: >
      A Prometheus text-exposition metrics endpoint must be mounted at
      `/metrics` (configurable). Counters at minimum: HTTP requests by
      method, status class, and route; histogram: request duration in
      seconds.
  - id: FR12
    statement: >
      Health endpoints: `/healthz` returns 200 OK iff the server is
      up; `/readyz` returns 200 OK iff the server is ready to accept
      traffic (post-startup, pre-graceful-shutdown).
  - id: FR13
    statement: >
      A minimal mustache-like template engine (`{{name}}`, `{{#list}}`
      `{{/list}}`, `{{^absent}}` `{{/absent}}`) must be provided. The
      engine exposes a `Renderer` with `render(template, context)`;
      handlers can use it to render HTML responses.
  - id: FR14
    statement: >
      Structured logging: a single `tracing`-compatible or equivalent
      facade is used across the framework. Log records include a
      per-request id that is also exposed as the response header
      `X-Request-Id`.
  - id: FR15
    statement: >
      CLI flags: `--listen`, `--config`, `--tls-cert`, `--tls-key`,
      `--metrics`, `--static-dir`, `--log-level`, `--version`,
      `--help`. Every CLI flag has a matching TOML key; CLI flags
      override TOML.

structural_requirements:
  - id: SR1
    statement: >
      The source tree must be organised as a Cargo workspace OR a
      single crate with clearly separated modules: `http` (parser,
      types), `router`, `middleware` (plus one sub-module per
      standard middleware), `body` (parsers), `static_files`, `tls`,
      `session`, `metrics`, `health`, `template`, `config`, `cli`,
      `server`, and `error`.
  - id: SR2
    statement: >
      The HTTP parser in `http::parser` must not depend on the
      router, middleware, or body modules. Byte-level parsing is
      isolated so it can be fuzz-tested in isolation.
  - id: SR3
    statement: >
      Routing is backed by a trie that supports literal segments,
      single-parameter segments, and a trailing wildcard. Route
      lookup must be O(path-length) and must not allocate per
      request beyond the parameter map.
  - id: SR4
    statement: >
      Middleware is composed via a single `Middleware` trait. The
      trait's object-safe form is what goes into the per-route chain;
      the concrete registration order is preserved and drives the
      outer-to-inner call order.
  - id: SR5
    statement: >
      TLS support must live behind a Cargo feature `tls` (enabled by
      default). When the feature is disabled, the crate must still
      compile and the `--tls-cert` flag must produce a clear error.
  - id: SR6
    statement: >
      The server uses an async runtime (tokio is acceptable). The
      accept loop and per-connection handling are explicitly split so
      graceful shutdown can be reasoned about locally.
  - id: SR7
    statement: >
      Configuration is loaded once at startup through a `config`
      module that produces a single `Config` struct. CLI flags
      override TOML values inside this module, not scattered across
      the crate.
  - id: SR8
    statement: >
      Metrics live in a dedicated `metrics` module. Handlers and
      middleware only interact with metrics through that module's
      public API; nothing else owns Prometheus registries.
  - id: SR9
    statement: >
      A single crate-wide error enum (`NanowebError`) is the return
      type of all fallible top-level entry points. Lower-level
      modules can define their own internal errors but must convert
      at the public boundary.
  - id: SR10
    statement: >
      Every module carries unit tests under `#[cfg(test)]`. The
      crate additionally provides integration tests under `tests/`
      that start the server on an ephemeral port and exercise at
      least one handler end-to-end per standard middleware.
  - id: SR11
    statement: >
      A `README.md` documents the framework: quickstart, routing
      primer, middleware list, TLS setup, metrics, template engine,
      and a short architectural overview.

edit_requirements: []

constraints:
  language: Rust
  frontend_allowed: false
  terminal_allowed: true
  network_allowed: true
  external_crates_allowed: true
  target_os: linux
  rust_toolchain: stable
  external_hosted_services_allowed: false
  build_time_budget_s: 5400

deliverables:
  - cargo_project
  - readme

validation:
  notes:
    - "cargo build --release must succeed at validation time."
    - "Scale target: several thousand lines of Rust across the listed modules."

intentions:
  functional:
    - id: I-FR2
      source: FR2
      kind: functional
      statement: >
        The framework ships its own HTTP/1.1 parser that handles
        chunked and Content-Length bodies and rejects malformed
        requests.
    - id: I-FR3
      source: FR3
      kind: functional
      statement: >
        The router supports path parameters, wildcard segments, and
        dispatch for GET/POST/PUT/DELETE/PATCH/HEAD/OPTIONS.
    - id: I-FR5
      source: FR5
      kind: functional
      statement: >
        Standard middleware ships for logging, gzip, rate limiting,
        CORS, CSRF (double-submit), and basic auth.
    - id: I-FR8
      source: FR8
      kind: functional
      statement: >
        TLS termination is provided via rustls and is activated by
        supplying cert and key PEM files.
    - id: I-FR10
      source: FR10
      kind: functional
      statement: >
        SIGINT/SIGTERM triggers graceful shutdown with a bounded
        timeout before forced exit.
    - id: I-FR11
      source: FR11
      kind: functional
      statement: >
        Prometheus metrics are exposed at a configurable endpoint,
        including counters by method/status/route and a request-
        duration histogram.
    - id: I-FR13
      source: FR13
      kind: functional
      statement: >
        A mustache-like template engine is included (sections,
        inverted sections, simple variable interpolation).
    - id: I-FR14
      source: FR14
      kind: functional
      statement: >
        Structured logs carry a per-request id that is also written
        back as the X-Request-Id response header.
  structural:
    - id: I-SR2
      source: SR2
      kind: structural
      statement: >
        The HTTP parser is a self-contained module with no inbound
        dependency on the router or middleware layers.
    - id: I-SR3
      source: SR3
      kind: structural
      statement: >
        Routing uses a trie with literal, parameter, and wildcard
        segment types.
    - id: I-SR4
      source: SR4
      kind: structural
      statement: >
        Middleware composes via a single Middleware trait; registration
        order drives outer-to-inner call order.
    - id: I-SR5
      source: SR5
      kind: structural
      statement: >
        TLS is behind a Cargo feature (tls) so the crate is usable
        without rustls.
    - id: I-SR7
      source: SR7
      kind: structural
      statement: >
        Configuration flows through a dedicated config module; CLI
        overrides TOML inside that module rather than elsewhere.
    - id: I-SR9
      source: SR9
      kind: structural
      statement: >
        A single crate-wide error enum is the return type of
        top-level fallible entry points.

tags: [rust, http, web, framework, tls, tokio, prometheus]
notes: >
  The parser-isolation (SR2), trie-router (SR3), middleware-trait
  (SR4), and TLS-behind-a-feature (SR5) signals are the load-bearing
  structural intentions. Agents that vendor an existing HTTP crate
  will fail the parser-isolation signal.
