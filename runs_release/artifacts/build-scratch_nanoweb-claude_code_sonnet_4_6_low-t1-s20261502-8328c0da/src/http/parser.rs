/// HTTP/1.1 parser - no dependencies on router/middleware/body modules
use std::collections::HashMap;

#[derive(Debug)]
pub struct ParsedRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum ParseError {
    Incomplete,
    Invalid(String),
    TooLarge,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Incomplete => write!(f, "Incomplete request"),
            ParseError::Invalid(s) => write!(f, "Invalid request: {}", s),
            ParseError::TooLarge => write!(f, "Request too large"),
        }
    }
}

pub struct Parser;

impl Parser {
    const MAX_HEADER_SIZE: usize = 16 * 1024; // 16KB
    const MAX_BODY_SIZE: usize = 64 * 1024 * 1024; // 64MB

    pub fn parse_headers(buf: &[u8]) -> Result<Option<(ParsedRequest, usize)>, ParseError> {
        if buf.len() > Self::MAX_HEADER_SIZE {
            return Err(ParseError::TooLarge);
        }

        // Find end of headers
        let header_end = match find_double_crlf(buf) {
            Some(pos) => pos,
            None => return Ok(None), // Need more data
        };

        let header_section = &buf[..header_end];
        let body_start = header_end + 4;

        // Parse request line
        let line_end = match find_crlf(header_section) {
            Some(pos) => pos,
            None => return Err(ParseError::Invalid("Missing request line".to_string())),
        };

        let request_line = std::str::from_utf8(&header_section[..line_end])
            .map_err(|_| ParseError::Invalid("Invalid UTF-8 in request line".to_string()))?;

        let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
        if parts.len() != 3 {
            return Err(ParseError::Invalid("Malformed request line".to_string()));
        }

        let method = parts[0].to_string();
        let full_path = parts[1].to_string();
        let version = parts[2].to_string();

        if !version.starts_with("HTTP/") {
            return Err(ParseError::Invalid("Invalid HTTP version".to_string()));
        }

        // Split path and query
        let (path, query) = parse_path_query(&full_path);

        // Parse headers
        let mut headers = HashMap::new();
        let mut pos = line_end + 2; // skip CRLF

        while pos < header_section.len() {
            let remaining = &header_section[pos..];
            let (line_end, skip) = match find_crlf(remaining) {
                Some(p) => (p, 2),
                None => {
                    // Last header may not have trailing CRLF (it's included in \r\n\r\n)
                    if remaining.is_empty() {
                        break;
                    }
                    (remaining.len(), 0)
                }
            };

            if line_end == 0 {
                break;
            }

            let header_line = std::str::from_utf8(&remaining[..line_end])
                .map_err(|_| ParseError::Invalid("Invalid UTF-8 in header".to_string()))?;

            if let Some(colon_pos) = header_line.find(':') {
                let name = header_line[..colon_pos].trim().to_lowercase();
                let value = header_line[colon_pos + 1..].trim().to_string();
                headers.insert(name, value);
            }

            pos += line_end + skip;
        }

        Ok(Some((
            ParsedRequest {
                method,
                path,
                query,
                version,
                headers,
                body: Vec::new(),
            },
            body_start,
        )))
    }

    pub async fn read_body(
        stream: &mut (impl tokio::io::AsyncRead + Unpin),
        parsed: &mut ParsedRequest,
        already_read: &[u8],
    ) -> Result<(), ParseError> {
        use tokio::io::AsyncReadExt;

        // Check transfer-encoding
        let is_chunked = parsed
            .headers
            .get("transfer-encoding")
            .map(|v| v.to_lowercase().contains("chunked"))
            .unwrap_or(false);

        if is_chunked {
            let mut body = already_read.to_vec();
            // Read chunked body
            loop {
                // Need to read chunk size line
                let chunk_size = read_chunk_size(stream, &mut body).await?;
                if chunk_size == 0 {
                    break;
                }
                if body.len() + chunk_size > Self::MAX_BODY_SIZE {
                    return Err(ParseError::TooLarge);
                }
                let mut chunk_data = vec![0u8; chunk_size];
                stream
                    .read_exact(&mut chunk_data)
                    .await
                    .map_err(|e| ParseError::Invalid(e.to_string()))?;
                body.extend_from_slice(&chunk_data);
                // Read trailing CRLF
                let mut crlf = [0u8; 2];
                stream
                    .read_exact(&mut crlf)
                    .await
                    .map_err(|e| ParseError::Invalid(e.to_string()))?;
            }
            parsed.body = body;
        } else if let Some(content_length) = parsed.headers.get("content-length") {
            let content_length: usize = content_length
                .parse()
                .map_err(|_| ParseError::Invalid("Invalid Content-Length".to_string()))?;

            if content_length > Self::MAX_BODY_SIZE {
                return Err(ParseError::TooLarge);
            }

            let mut body = already_read.to_vec();
            let remaining = content_length.saturating_sub(body.len());

            if remaining > 0 {
                let mut rest = vec![0u8; remaining];
                stream
                    .read_exact(&mut rest)
                    .await
                    .map_err(|e| ParseError::Invalid(e.to_string()))?;
                body.extend_from_slice(&rest);
            }

            parsed.body = body[..content_length].to_vec();
        }
        // Otherwise no body

        Ok(())
    }
}

async fn read_chunk_size(
    stream: &mut (impl tokio::io::AsyncRead + Unpin),
    _buf: &mut Vec<u8>,
) -> Result<usize, ParseError> {
    use tokio::io::AsyncReadExt;

    let mut line = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream
            .read_exact(&mut byte)
            .await
            .map_err(|e| ParseError::Invalid(e.to_string()))?;
        if byte[0] == b'\r' {
            let mut lf = [0u8; 1];
            stream
                .read_exact(&mut lf)
                .await
                .map_err(|e| ParseError::Invalid(e.to_string()))?;
            break;
        }
        line.push(byte[0]);
    }

    let size_str =
        std::str::from_utf8(&line).map_err(|_| ParseError::Invalid("Bad chunk size".to_string()))?;
    // Strip chunk extensions
    let size_str = size_str.split(';').next().unwrap_or("").trim();
    usize::from_str_radix(size_str, 16)
        .map_err(|_| ParseError::Invalid(format!("Invalid chunk size: {}", size_str)))
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_path_query(full_path: &str) -> (String, HashMap<String, String>) {
    if let Some(q_pos) = full_path.find('?') {
        let path = full_path[..q_pos].to_string();
        let query_str = &full_path[q_pos + 1..];
        let query = parse_query_string(query_str);
        (path, query)
    } else {
        (full_path.to_string(), HashMap::new())
    }
}

fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some(eq_pos) = pair.find('=') {
            let key = url_decode(&pair[..eq_pos]);
            let value = url_decode(&pair[eq_pos + 1..]);
            map.insert(key, value);
        } else {
            map.insert(url_decode(pair), String::new());
        }
    }
    map
}

fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'%' => {
                let h1 = chars.next().unwrap_or(b'0');
                let h2 = chars.next().unwrap_or(b'0');
                if let Ok(decoded) = u8::from_str_radix(
                    &format!("{}{}", h1 as char, h2 as char),
                    16,
                ) {
                    result.push(decoded as char);
                }
            }
            b'+' => result.push(' '),
            _ => result.push(b as char),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let raw = b"GET /hello?foo=bar HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n";
        let result = Parser::parse_headers(raw).unwrap();
        assert!(result.is_some());
        let (parsed, body_start) = result.unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.path, "/hello");
        assert_eq!(parsed.query.get("foo"), Some(&"bar".to_string()));
        assert_eq!(body_start, raw.len());
    }

    #[test]
    fn test_incomplete_request() {
        let raw = b"GET /hello HTTP/1.1\r\nHost: localh";
        let result = Parser::parse_headers(raw).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_bad_request() {
        let raw = b"INVALID\r\n\r\n";
        let result = Parser::parse_headers(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("foo+bar"), "foo bar");
    }
}
