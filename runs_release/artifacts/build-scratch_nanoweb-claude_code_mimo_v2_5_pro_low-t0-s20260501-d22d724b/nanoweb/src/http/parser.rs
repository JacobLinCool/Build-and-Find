use super::types::{Method, Request};
use crate::error::NanowebError;

const MAX_HEADER_SIZE: usize = 8192;
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    RequestLine,
    Headers,
    Body,
    Done,
}

pub struct HttpParser {
    buf: Vec<u8>,
    state: ParseState,
    pending_method: Option<Method>,
    pending_path: Option<String>,
    pending_version: Option<String>,
    pending_headers: Vec<(String, String)>,
}

impl HttpParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
            state: ParseState::RequestLine,
            pending_method: None,
            pending_path: None,
            pending_version: None,
            pending_headers: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.state = ParseState::RequestLine;
        self.pending_method = None;
        self.pending_path = None;
        self.pending_version = None;
        self.pending_headers.clear();
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    pub fn parse(&mut self) -> Result<Option<Request>, NanowebError> {
        loop {
            match self.state {
                ParseState::RequestLine => {
                    if let Some(pos) = find_crlf(&self.buf) {
                        let line = self.buf[..pos].to_vec();
                        self.buf.drain(..pos + 2);
                        let parts: Vec<&[u8]> = line.split(|&b| b == b' ').collect();
                        if parts.len() != 3 {
                            return Err(NanowebError::Parse("Invalid request line".into()));
                        }
                        self.pending_method =
                            Some(Method::from_bytes(parts[0]).ok_or_else(|| {
                                NanowebError::Parse(format!(
                                    "Unknown method: {}",
                                    String::from_utf8_lossy(parts[0])
                                ))
                            })?);
                        self.pending_path =
                            Some(String::from_utf8(parts[1].to_vec()).map_err(|_| {
                                NanowebError::Parse("Invalid UTF-8 in target".into())
                            })?);
                        let ver = String::from_utf8(parts[2].to_vec())
                            .map_err(|_| NanowebError::Parse("Invalid UTF-8 in version".into()))?;
                        if ver != "HTTP/1.1" && ver != "HTTP/1.0" {
                            return Err(NanowebError::Parse(format!(
                                "Unsupported HTTP version: {ver}"
                            )));
                        }
                        self.pending_version = Some(ver);
                        self.state = ParseState::Headers;
                    } else {
                        return Ok(None);
                    }
                }
                ParseState::Headers => loop {
                    if self.buf.len() >= 2 && &self.buf[..2] == b"\r\n" {
                        self.buf.drain(..2);
                        self.state = ParseState::Body;
                        break;
                    }
                    if let Some(pos) = find_crlf(&self.buf) {
                        let line = self.buf[..pos].to_vec();
                        self.buf.drain(..pos + 2);
                        if let Some(colon_pos) = line.iter().position(|&b| b == b':') {
                            let name =
                                String::from_utf8_lossy(&line[..colon_pos]).to_ascii_lowercase();
                            let value = String::from_utf8_lossy(&line[colon_pos + 1..])
                                .trim()
                                .to_string();
                            self.pending_headers.push((name, value));
                        }
                    } else {
                        if self.buf.len() > MAX_HEADER_SIZE {
                            return Err(NanowebError::Parse("Headers too large".into()));
                        }
                        return Ok(None);
                    }
                },
                ParseState::Body => {
                    let content_length: usize = self
                        .pending_headers
                        .iter()
                        .find(|(k, _)| k == "content-length")
                        .and_then(|(_, v)| v.parse().ok())
                        .unwrap_or(0);

                    let is_chunked = self.pending_headers.iter().any(|(k, v)| {
                        k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked")
                    });

                    if is_chunked {
                        match parse_chunked_body(&self.buf) {
                            Ok(Some(body)) => {
                                self.buf.clear();
                                let req = self.build_request(body)?;
                                self.reset();
                                return Ok(req);
                            }
                            Ok(None) => return Ok(None),
                            Err(e) => return Err(e),
                        }
                    } else if content_length > 0 {
                        if content_length > MAX_BODY_SIZE {
                            return Err(NanowebError::Parse("Body too large".into()));
                        }
                        if self.buf.len() >= content_length {
                            let body = self.buf[..content_length].to_vec();
                            self.buf.drain(..content_length);
                            let req = self.build_request(body)?;
                            self.reset();
                            return Ok(req);
                        } else {
                            return Ok(None);
                        }
                    } else {
                        let req = self.build_request(Vec::new())?;
                        self.reset();
                        return Ok(req);
                    }
                }
                ParseState::Done => return Ok(None),
            }
        }
    }

    fn build_request(&self, body: Vec<u8>) -> Result<Option<Request>, NanowebError> {
        let method = self
            .pending_method
            .as_ref()
            .ok_or_else(|| NanowebError::Parse("No method".into()))?;
        let target = self
            .pending_path
            .as_ref()
            .ok_or_else(|| NanowebError::Parse("No path".into()))?;
        let version = self
            .pending_version
            .as_ref()
            .ok_or_else(|| NanowebError::Parse("No version".into()))?;
        let (path, query) = parse_target(target);
        let mut req = Request::new(method.clone(), path, version.clone());
        for (name, value) in &self.pending_headers {
            req.headers.insert(name.clone(), value.clone());
        }
        req.query = query;
        req.set_body(body);
        Ok(Some(req))
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn parse_chunked_body(buf: &[u8]) -> Result<Option<Vec<u8>>, NanowebError> {
    let mut result = Vec::new();
    let mut pos = 0;

    loop {
        if pos >= buf.len() {
            return Ok(None);
        }
        let chunk_end = match buf[pos..].windows(2).position(|w| w == b"\r\n") {
            Some(p) => pos + p,
            None => return Ok(None),
        };
        let size_str = std::str::from_utf8(&buf[pos..chunk_end])
            .map_err(|_| NanowebError::Parse("Invalid chunk size".into()))?;
        let size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|_| NanowebError::Parse(format!("Invalid chunk size: {size_str}")))?;
        pos = chunk_end + 2;

        if size == 0 {
            return Ok(Some(result));
        }

        if pos + size + 2 > buf.len() {
            return Ok(None);
        }
        result.extend_from_slice(&buf[pos..pos + size]);
        pos += size + 2;
    }
}

fn parse_target(target: &str) -> (String, std::collections::HashMap<String, String>) {
    if let Some(qpos) = target.find('?') {
        let path = target[..qpos].to_string();
        let query_str = &target[qpos + 1..];
        let query: std::collections::HashMap<String, String> =
            serde_urlencoded::from_str(query_str).unwrap_or_default();
        (path, query)
    } else {
        (target.to_string(), std::collections::HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let mut parser = HttpParser::new();
        parser.feed(b"GET /hello HTTP/1.1\r\nHost: example.com\r\n\r\n");
        let req = parser.parse().unwrap().unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/hello");
        assert_eq!(req.header("host"), Some("example.com"));
    }

    #[test]
    fn test_parse_post_with_body() {
        let mut parser = HttpParser::new();
        let body = b"key=value";
        let request = format!(
            "POST /submit HTTP/1.1\r\nHost: example.com\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        parser.feed(request.as_bytes());
        parser.feed(body);
        let req = parser.parse().unwrap().unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body_ref(), body);
    }

    #[test]
    fn test_parse_chunked_body() {
        let mut parser = HttpParser::new();
        parser.feed(
            b"POST /data HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        );
        let req = parser.parse().unwrap().unwrap();
        assert_eq!(req.body_ref(), b"hello");
    }

    #[test]
    fn test_parse_query_string() {
        let mut parser = HttpParser::new();
        parser.feed(b"GET /search?q=rust&lang=en HTTP/1.1\r\nHost: example.com\r\n\r\n");
        let req = parser.parse().unwrap().unwrap();
        assert_eq!(req.path, "/search");
        assert_eq!(req.query.get("q").unwrap(), "rust");
        assert_eq!(req.query.get("lang").unwrap(), "en");
    }

    #[test]
    fn test_parse_invalid_method() {
        let mut parser = HttpParser::new();
        parser.feed(b"INVALID /path HTTP/1.1\r\nHost: example.com\r\n\r\n");
        assert!(parser.parse().is_err());
    }

    #[test]
    fn test_parse_multiple_requests() {
        let mut parser = HttpParser::new();
        parser.feed(b"GET /first HTTP/1.1\r\nHost: a.com\r\n\r\nGET /second HTTP/1.1\r\nHost: b.com\r\n\r\n");
        let r1 = parser.parse().unwrap().unwrap();
        assert_eq!(r1.path, "/first");
        let r2 = parser.parse().unwrap().unwrap();
        assert_eq!(r2.path, "/second");
    }
}
