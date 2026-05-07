//! HTTP/1.1 primitives: request/response types and a minimal parser.
//!
//! This module is self-contained. The parser does not reach into the
//! router, middleware, or body submodules; it only produces a
//! `Request` value plus the remaining unread body bytes.

pub mod parser;
pub mod types;

pub use parser::{parse_request, ParseOutcome};
pub use types::{
    Headers, Method, Request, RequestId, Response, StatusCode, Version, X_REQUEST_ID_HEADER,
};
